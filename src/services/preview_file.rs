//! Private, exclusive browser-preview files with shared lifecycle ownership.
//! Cleanup is best effort on process exit/eviction; crashes and browser-held
//! Windows handles may leave a private file, so this is not encrypted export.
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

pub const MAX_PREVIEW_BYTES: usize = 10_000_000;
#[derive(Debug, Clone)]
pub struct PreviewFile(Arc<OwnedPreview>);
#[derive(Debug)]
struct OwnedPreview {
    path: PathBuf,
    directory: PathBuf,
}
impl AsRef<Path> for PreviewFile {
    fn as_ref(&self) -> &Path {
        &self.0.path
    }
}
impl Drop for OwnedPreview {
    fn drop(&mut self) {
        // Only exact owned paths; never sweep unrelated temp files recursively.
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn create(root: &Path, bytes: &[u8]) -> Result<PreviewFile, String> {
    if bytes.len() > MAX_PREVIEW_BYTES {
        return Err("Browser preview exceeds 10 MB limit".into());
    }
    let directory = root.join(format!("rustman-preview-{}", uuid::Uuid::new_v4()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory).map_err(|e| e.to_string())?;
    let preview = PreviewFile(Arc::new(OwnedPreview {
        path: directory.join("body.html"),
        directory,
    }));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(preview.as_ref()).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    Ok(preview)
}
// Serialize creation with restart cleanup so pending writers cannot recreate files.
#[derive(Default)]
struct Registry {
    shutting_down: bool,
    records: Vec<Record>,
}
struct Record {
    path: PathBuf,
    directory: PathBuf,
    owner: Weak<OwnedPreview>,
}
pub struct ShutdownGuard(Arc<Mutex<Registry>>);
impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock() {
            state.shutting_down = false;
        }
    }
}
fn registry() -> &'static Arc<Mutex<Registry>> {
    static REGISTRY: OnceLock<Arc<Mutex<Registry>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Arc::new(Mutex::new(Registry::default())))
}
fn cleanup(record: &Record) -> bool {
    let removed = match std::fs::remove_file(&record.path) {
        Ok(()) => true,
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    };
    if removed {
        let _ = std::fs::remove_dir(&record.directory);
    }
    removed
}
pub fn write(root: &Path, bytes: &[u8]) -> Result<PreviewFile, String> {
    write_for(registry(), root, bytes)
}
fn write_for(
    registry: &Arc<Mutex<Registry>>,
    root: &Path,
    bytes: &[u8],
) -> Result<PreviewFile, String> {
    let mut state = registry
        .lock()
        .map_err(|_| "Preview registry unavailable")?;
    if state.shutting_down {
        return Err("Application is restarting".into());
    }
    state
        .records
        .retain(|r| r.owner.strong_count() > 0 || !cleanup(r));
    if state.records.len() >= 16 {
        return Err("Too many active browser previews".into());
    }
    let preview = create(root, bytes)?;
    state.records.push(Record {
        path: preview.0.path.clone(),
        directory: preview.0.directory.clone(),
        owner: Arc::downgrade(&preview.0),
    });
    Ok(preview)
}
pub fn prepare_shutdown() -> Result<ShutdownGuard, String> {
    prepare_shutdown_for(registry())
}
fn prepare_shutdown_for(registry: &Arc<Mutex<Registry>>) -> Result<ShutdownGuard, String> {
    let mut state = registry
        .lock()
        .map_err(|_| "Preview registry unavailable")?;
    state.shutting_down = true;
    state.records.retain(|r| !cleanup(r));
    if !state.records.is_empty() {
        state.shutting_down = false;
        return Err("Close browser preview files before restarting".into());
    }
    Ok(ShutdownGuard(registry.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn security_publication_shutdown_cleans_live_previews_and_blocks_late_writes() {
        let root =
            std::env::temp_dir().join(format!("rustman-shutdown-preview-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let registry = Arc::new(std::sync::Mutex::new(Registry::default()));
        let preview = write_for(&registry, &root, b"synthetic-live-response").unwrap();
        let cloned = preview.clone();
        let path = preview.as_ref().to_path_buf();
        let shutdown = prepare_shutdown_for(&registry).unwrap();
        assert!(
            !path.exists(),
            "explicit restart cleanup must not depend on Arc destructor timing"
        );
        assert!(write_for(&registry, &root, b"late-preview").is_err());
        drop(shutdown);
        let retry = write_for(&registry, &root, b"retry-after-failed-restart").unwrap();
        drop(retry);
        drop(preview);
        drop(cloned);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn security_publication_preview_is_unique_private_and_preserves_unrelated_files() {
        let root =
            std::env::temp_dir().join(format!("rustman-preview-unique-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("unrelated.txt"), b"preserve").unwrap();
        let first = write(&root, b"synthetic-one").unwrap();
        let second = write(&root, b"synthetic-two").unwrap();
        assert_ne!(first.as_ref(), second.as_ref());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(first.as_ref())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(first.as_ref().parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        drop(first);
        drop(second);
        assert_eq!(
            std::fs::read(root.join("unrelated.txt")).unwrap(),
            b"preserve"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn security_publication_oversized_preview_writes_nothing() {
        let root =
            std::env::temp_dir().join(format!("rustman-preview-limit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(write(&root, &vec![b'x'; MAX_PREVIEW_BYTES + 1]).is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn security_publication_preview_lives_until_last_owner_then_is_removed() {
        let root =
            std::env::temp_dir().join(format!("rustman-preview-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let preview = write(&root, b"synthetic-private-response").unwrap();
        let path: &Path = preview.as_ref();
        let path = path.to_path_buf();
        let retained = preview.clone();
        drop(preview);
        assert_eq!(std::fs::read(&path).unwrap(), b"synthetic-private-response");
        drop(retained);
        assert!(
            !path.exists(),
            "plaintext preview must be removed after final owner drops"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
