//! In-app self-update from GitHub releases (https://github.com/rust-man-org/rustman).
//!
//! Uses the app's existing async `reqwest` to fetch the latest release and the
//! matching platform asset, then extracts the binary and atomically swaps the
//! running executable. The asset tokens / binary name must match the artefacts
//! produced by `.github/workflows/release.yml`.
//!
//! Compiled only with the `self-update` feature (on by default). Package
//! managers build with `--no-default-features`, which removes this whole
//! module plus the boot-time check, the Settings → Updates card and the update
//! banner: the package manager owns the installed binary and delivers updates.

// Build-time trust root only. An unconfigured build cannot install updates.
const UPDATE_PUBLIC_KEY: Option<&str> = option_env!("RUSTMAN_UPDATE_PUBLIC_KEY");
const MAX_DOWNLOAD: usize = 128 * 1024 * 1024;
const MAX_METADATA: usize = 1024 * 1024;
const MAX_BINARY: u64 = 256 * 1024 * 1024;
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/rust-man-org/rustman/releases/latest";

/// Asset token matched against the release asset names for the running
/// platform and CPU architecture.
///
/// Returns an error on platforms or architectures the release pipeline does
/// not build for. A hardcoded per-OS token used to mean an arm64 Linux user
/// checking for updates would be offered — and then install — the x86_64
/// binary.
fn asset_target() -> Result<&'static str, String> {
    #[cfg(target_os = "macos")]
    {
        // Universal binary: one artefact covers both Intel and Apple Silicon.
        Ok("macos-universal")
    }
    #[cfg(target_os = "linux")]
    {
        match std::env::consts::ARCH {
            "x86_64" => Ok("linux-x86_64"),
            "aarch64" => Ok("linux-aarch64"),
            other => Err(format!(
                "no Linux release builds for the '{other}' architecture — build from source instead"
            )),
        }
    }
    #[cfg(target_os = "windows")]
    {
        match std::env::consts::ARCH {
            "x86_64" => Ok("windows-x86_64"),
            "aarch64" => Ok("windows-aarch64"),
            other => Err(format!(
                "no Windows release builds for the '{other}' architecture — build from source instead"
            )),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err("self-update is not supported on this platform".to_owned())
    }
}

/// Extension the self-update asset must end with. The macOS release uploads
/// *two* assets whose names both contain the platform token — the installer
/// DMG and the tarball this self-updater actually wants — so the substring
/// match alone isn't enough; without this, picking the `.dmg` by accident
/// feeds a disk image into the gzip decoder and fails with "invalid Gzip
/// header".
#[cfg(not(target_os = "windows"))]
const ASSET_EXTENSION: &str = ".tar.gz";
#[cfg(target_os = "windows")]
const ASSET_EXTENSION: &str = ".zip";

/// Name of the executable inside the downloaded archive.
#[cfg(not(target_os = "windows"))]
const BIN_NAME: &str = "rustman";
#[cfg(target_os = "windows")]
const BIN_NAME: &str = "rustman.exe";

/// The current build's version, baked in at compile time.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// The newer version available, e.g. `"0.4.0"`.
    pub version: String,
    /// The running version.
    pub current: String,
}

#[derive(serde::Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(serde::Deserialize, Debug)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

fn trusted_public_key() -> Result<minisign_verify::PublicKey, String> {
    let key = UPDATE_PUBLIC_KEY.ok_or(
        "Secure updates are unavailable: this build has no pinned release signing public key",
    )?;
    minisign_verify::PublicKey::from_base64(key)
        .map_err(|_| "Invalid pinned release signing public key".to_owned())
}

fn allowed_redirect(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com")
        )
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("rustman/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 5 || !allowed_redirect(attempt.url()) {
                attempt.error("Untrusted release redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| e.to_string())
}

async fn bounded_response(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err("Release response exceeds size limit".to_owned());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err("Release response exceeds size limit".to_owned());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn fetch_latest() -> Result<GhRelease, String> {
    // Credentials are scoped to this API request, never default client headers.
    let mut request = client()?
        .get(LATEST_RELEASE_API)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let bytes = bounded_response(response, MAX_METADATA).await?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn validate_asset_url(asset: &GhAsset, tag: &str) -> Result<(), String> {
    let expected = format!(
        "https://github.com/rust-man-org/rustman/releases/download/{tag}/{}",
        asset.name
    );
    if asset.browser_download_url != expected {
        return Err("Release asset URL does not match the trusted repository and tag".to_owned());
    }
    Ok(())
}

async fn download(asset: &GhAsset, tag: &str, limit: usize) -> Result<Vec<u8>, String> {
    validate_asset_url(asset, tag)?;
    let response = client()?
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    bounded_response(response, limit).await
}

/// Query the latest release; returns `Some` only when it is newer than the
/// running build.
///
/// Fails on architectures with no published release artefact, so the UI can
/// say why there is no updater rather than offering an update that could
/// never be installed.
pub async fn check() -> Result<Option<UpdateInfo>, String> {
    asset_target()?;
    let release = fetch_latest().await?;
    let latest = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_owned();
    let current = current_version();
    Ok(version_gt(&latest, current).then(|| UpdateInfo {
        version: latest,
        current: current.to_owned(),
    }))
}

/// Picks the self-update asset out of a release's asset list whose name
/// contains `target` and ends with `extension`. Matching on `target` alone
/// isn't enough on macOS, where the DMG installer and the tarball both have
/// it in their name — see `ASSET_EXTENSION`'s doc comment.
fn pick_asset<'a>(
    assets: &'a [GhAsset],
    release: &str,
    target: &str,
    extension: &str,
) -> Result<&'a GhAsset, String> {
    let name = if target == "macos-universal" {
        format!("Rustman-{target}{extension}")
    } else {
        format!("rustman-{target}{extension}")
    };
    let mut matching = assets.iter().filter(|a| a.name == name);
    let asset = matching
        .next()
        .ok_or_else(|| format!("release {release} has no '{target}*{extension}' asset"))?;
    if matching.next().is_some() {
        return Err("Duplicate release asset name".to_owned());
    }
    Ok(asset)
}

fn named_asset<'a>(assets: &'a [GhAsset], name: &str) -> Result<&'a GhAsset, String> {
    let mut found = assets.iter().filter(|asset| asset.name == name);
    let asset = found
        .next()
        .ok_or_else(|| format!("Release is missing signed verification asset {name}"))?;
    if found.next().is_some() {
        return Err("Duplicate verification asset".to_owned());
    }
    Ok(asset)
}

fn verify_manifest_signature(
    key: &minisign_verify::PublicKey,
    manifest: &[u8],
    signature: &[u8],
) -> Result<minisign_verify::Signature, String> {
    let signature = std::str::from_utf8(signature).map_err(|e| e.to_string())?;
    let signature = minisign_verify::Signature::decode(signature).map_err(|e| e.to_string())?;
    key.verify(manifest, &signature, false)
        .map_err(|e| format!("Release signature verification failed: {e}"))?;
    Ok(signature)
}

fn verify_checksum(manifest: &[u8], name: &str, bytes: &[u8]) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let manifest = std::str::from_utf8(manifest).map_err(|e| e.to_string())?;
    let mut expected = None;
    for line in manifest.lines() {
        let Some((hash, file)) = line.split_once("  ") else {
            return Err("Malformed SHA256SUMS line".to_owned());
        };
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Malformed SHA256 checksum".to_owned());
        }
        if file == name && expected.replace(hash.to_ascii_lowercase()).is_some() {
            return Err("Duplicate asset checksum".to_owned());
        }
    }
    let actual = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if expected.as_deref() != Some(actual.as_str()) {
        return Err("Release archive checksum mismatch or missing checksum".to_owned());
    }
    Ok(())
}

pub async fn install() -> Result<String, String> {
    // Fail closed before network requests, decompression or filesystem writes.
    let key = trusted_public_key()?;
    let release = fetch_latest().await?;
    let latest = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_owned();
    if !version_gt(&latest, current_version()) {
        return Err("Release is not a newer stable version".to_owned());
    }
    let target = asset_target()?;
    let asset = pick_asset(&release.assets, &latest, target, ASSET_EXTENSION)?;
    let manifest = download(
        named_asset(&release.assets, "SHA256SUMS")?,
        &release.tag_name,
        MAX_METADATA,
    )
    .await?;
    let signature = download(
        named_asset(&release.assets, "SHA256SUMS.minisig")?,
        &release.tag_name,
        16 * 1024,
    )
    .await?;
    let signature = verify_manifest_signature(&key, &manifest, &signature)?;
    // Prevent replaying a valid old manifest under a newer release tag.
    if signature.trusted_comment() != format!("rustman-release:{}", release.tag_name) {
        return Err("Signed release tag does not match requested release".to_owned());
    }
    let bytes = download(asset, &release.tag_name, MAX_DOWNLOAD).await?;
    verify_checksum(&manifest, &asset.name, &bytes)?;
    tokio::task::spawn_blocking(move || extract_and_replace(bytes))
        .await
        .map_err(|e| e.to_string())??;
    Ok(latest)
}

/// Resolves the path of the freshly-updated executable. On Linux,
/// `self_replace` swaps the running binary out from under the process by
/// renaming the new file onto its own path — an atomic `rename(2)` that
/// unlinks the old directory entry while the process still has the old
/// inode mapped. `std::env::current_exe()` reads `/proc/self/exe`, which
/// keeps resolving to that now-unlinked old inode, with a literal
/// `" (deleted)"` suffix appended to the path string by the kernel. The new
/// binary is a perfectly real file sitting at that same path *without* the
/// suffix, so strip it before checking existence or spawning — otherwise
/// `exe.exists()` is false and any error message names an unrunnable path.
fn resolved_exe_path() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    return Ok(strip_deleted_suffix(exe));
    #[cfg(not(target_os = "linux"))]
    Ok(exe)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn strip_deleted_suffix(exe: std::path::PathBuf) -> std::path::PathBuf {
    match exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(stripped) => std::path::PathBuf::from(stripped),
        None => exe,
    }
}

/// Relaunch the freshly-updated executable and exit the current process.
pub fn restart() -> Result<std::convert::Infallible, String> {
    let exe = resolved_exe_path()?;

    if !exe.exists() {
        return Err(format!(
            "Binary not found at {path}. The update replaced the file but the current \
             process may be reading a stale inode. Try running {path} manually.",
            path = exe.display(),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, a downloaded binary is often quarantined.  Try to remove the
        // quarantine attribute so Gatekeeper doesn't block the relaunch.  If this
        // fails we still attempt to spawn — the error is propagated if it fails.
        let _ = std::process::Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(&exe)
            .output();
    }

    let _preview_shutdown = crate::services::preview_file::prepare_shutdown()?;
    match std::process::Command::new(&exe).spawn() {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            let hint = if cfg!(target_os = "macos") {
                "The binary may need code-signing or the quarantine flag removed. \
                 Try: xattr -dr com.apple.quarantine "
            } else if cfg!(target_os = "linux") {
                "If you are running via cargo run, the binary was swapped but the \
                 old inode may still be cached. Try running the binary directly: "
            } else {
                ""
            };
            Err(format!(
                "Failed to restart from {path}: {e}. {hint}{path}",
                path = exe.display(),
                hint = hint,
            ))
        }
    }
}

/// Compare strictly parsed stable `MAJOR.MINOR.PATCH` versions.
fn version_gt(a: &str, b: &str) -> bool {
    matches!((parse_ver(a), parse_ver(b)), (Some(a), Some(b)) if a > b)
}

fn parse_ver(v: &str) -> Option<(u64, u64, u64)> {
    let fields = v.split('.').collect::<Vec<_>>();
    if fields.len() != 3 {
        return None;
    }
    let parse = |field: &str| {
        if field.is_empty()
            || !field.bytes().all(|b| b.is_ascii_digit())
            || (field.len() > 1 && field.starts_with('0'))
        {
            return None;
        }
        field.parse::<u64>().ok()
    };
    Some((parse(fields[0])?, parse(fields[1])?, parse(fields[2])?))
}

struct TemporaryBinary(std::path::PathBuf);
impl Drop for TemporaryBinary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Called only after authenticating the complete archive and its release tag.
fn extract_and_replace(bytes: Vec<u8>) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = current.parent().ok_or("current exe has no parent dir")?;
    let path = dir.join(format!(".rustman-update-{}.tmp", uuid::Uuid::new_v4()));
    extract_binary(&bytes, &path)?;
    let tmp = TemporaryBinary(path);
    verify_binary(&tmp.0)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp.0, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    self_replace::self_replace(&tmp.0).map_err(|e| e.to_string())
}

/// A structural sanity check; authenticity comes from the signed archive hash.
fn verify_binary(path: &std::path::Path) -> Result<(), String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if !(64..=MAX_BINARY).contains(&size) {
        return Err("Extracted binary size is invalid".to_owned());
    }
    let mut header = Vec::new();
    file.take(4096)
        .read_to_end(&mut header)
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    let valid = if header.starts_with(b"MZ") {
        let offset = u32::from_le_bytes(header[60..64].try_into().unwrap()) as usize;
        offset >= 64 && header.get(offset..offset.saturating_add(4)) == Some(b"PE\0\0".as_slice())
    } else {
        false
    };
    #[cfg(target_os = "linux")]
    let valid =
        header.starts_with(b"\x7fELF") && header[4] == 2 && header[5] == 1 && header[6] == 1;
    #[cfg(target_os = "macos")]
    let valid = matches!(
        &header[..4],
        [0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] | [0xbe, 0xba, 0xfe, 0xca]
    );
    if valid {
        Ok(())
    } else {
        Err("Extracted binary has invalid native executable header".to_owned())
    }
}

fn write_binary(out: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // create_new rejects both pre-existing files and symlinks atomically.
    let mut file = options.open(out).map_err(|e| e.to_string())?;
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    if let Err(error) = result {
        drop(file);
        let _ = std::fs::remove_file(out);
        return Err(error.to_string());
    }
    Ok(())
}

fn bounded_binary(reader: impl std::io::Read, size: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if size > MAX_BINARY {
        return Err("Archive binary exceeds size limit".to_owned());
    }
    let mut bytes = Vec::new();
    reader
        .take(MAX_BINARY + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BINARY || bytes.len() as u64 != size {
        return Err("Archive binary size is invalid".to_owned());
    }
    Ok(bytes)
}

#[cfg(not(target_os = "windows"))]
fn extract_binary(bytes: &[u8], out: &std::path::Path) -> Result<(), String> {
    extract_tar_binary(bytes, out)
}

#[cfg(any(not(target_os = "windows"), test))]
fn extract_tar_binary(bytes: &[u8], out: &std::path::Path) -> Result<(), String> {
    use std::io::Read;
    let gz = flate2::read::GzDecoder::new(bytes).take(MAX_UNPACKED + 1);
    let mut archive = tar::Archive::new(gz);
    let mut binary = None;
    let mut total = 0u64;
    for (index, entry) in archive.entries().map_err(|e| e.to_string())?.enumerate() {
        if index >= MAX_ENTRIES {
            return Err("Too many archive entries".to_owned());
        }
        let mut entry = entry.map_err(|e| e.to_string())?;
        total = total
            .checked_add(entry.size())
            .ok_or("Archive size overflow")?;
        if total > MAX_UNPACKED {
            return Err("Archive exceeds unpacked size limit".to_owned());
        }
        let path = entry.path().map_err(|e| e.to_string())?;
        if path == std::path::Path::new("rustman")
            || path == std::path::Path::new("rustman/rustman")
        {
            if entry.header().entry_type().is_dir() {
                continue;
            }
            if !entry.header().entry_type().is_file() {
                return Err("Archive binary is not a regular file".to_owned());
            }
            if binary.is_some() {
                return Err("Duplicate archive binary".to_owned());
            }
            let size = entry.size();
            binary = Some(bounded_binary(&mut entry, size)?);
        }
    }
    write_binary(
        out,
        &binary.ok_or_else(|| format!("'{BIN_NAME}' not found in archive"))?,
    )
}

#[cfg(target_os = "windows")]
fn extract_binary(bytes: &[u8], out: &std::path::Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if zip.len() > MAX_ENTRIES {
        return Err("Too many archive entries".to_owned());
    }
    let mut binary = None;
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        total = total
            .checked_add(entry.size())
            .ok_or("Archive size overflow")?;
        if total > MAX_UNPACKED {
            return Err("Archive exceeds unpacked size limit".to_owned());
        }
        // Never unpack archive paths; only the exact root-level binary is read.
        if entry.name().map_err(|e| e.to_string())? == BIN_NAME {
            if !entry.is_file() || entry.is_symlink() {
                return Err("Archive binary is not a regular file".to_owned());
            }
            if binary.is_some() {
                return Err("Duplicate archive binary".to_owned());
            }
            let size = entry.size();
            binary = Some(bounded_binary(&mut entry, size)?);
        }
    }
    write_binary(
        out,
        &binary.ok_or_else(|| format!("'{BIN_NAME}' not found in archive"))?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_confusable_and_malformed_versions() {
        for bad in [
            "一.999.0",
            "1.2.3.extra",
            "01.2.3",
            "1. 2.3",
            "1.2.3-dev.1",
            "1.2",
        ] {
            assert!(!version_gt(bad, "0.0.0"), "accepted {bad}");
        }
    }

    #[test]
    fn rejects_asset_substring_spoofing() {
        let assets = vec![asset("evil-rustman-windows-x86_64.zip")];
        assert!(pick_asset(&assets, "v1.2.3", "windows-x86_64", ".zip").is_err());
    }

    #[test]
    fn extraction_never_overwrites_existing_file() {
        let path = std::env::temp_dir().join(format!("rustman-security-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"untouched").unwrap();
        #[cfg(target_os = "windows")]
        let archive = release_zip(BIN_NAME, b"MZ harmless fixture");
        #[cfg(not(target_os = "windows"))]
        let archive = release_tarball(None, b"\x7fELF harmless fixture");
        let result = extract_binary(&archive, &path);
        let contents = std::fs::read(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(
            result.is_err(),
            "existing extraction target was overwritten"
        );
        assert_eq!(contents, b"untouched");
    }

    #[test]
    fn rejects_magic_byte_script_polyglots() {
        for bytes in [
            b"MZ#!/bin/sh\necho payload".as_slice(),
            b"\x7fELF#!/bin/sh\necho payload",
        ] {
            let path =
                std::env::temp_dir().join(format!("rustman-security-{}", uuid::Uuid::new_v4()));
            std::fs::write(&path, bytes).unwrap();
            let result = verify_binary(&path);
            std::fs::remove_file(&path).unwrap();
            assert!(result.is_err());
        }
    }

    #[cfg(target_os = "windows")]
    fn release_zip(name: &str, bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn requires_pinned_signing_key_before_installing() {
        if UPDATE_PUBLIC_KEY.is_none() {
            assert!(trusted_public_key().is_err());
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            let error = runtime.block_on(install()).unwrap_err();
            assert!(error.contains("no pinned release signing public key"));
        }
    }

    #[test]
    fn pins_asset_origin_tag_and_redirect_hosts() {
        let mut asset = asset("rustman-windows-x86_64.zip");
        let good = "https://github.com/rust-man-org/rustman/releases/download/v1.2.3/rustman-windows-x86_64.zip";
        for url in [
            "http://github.com/rust-man-org/rustman/releases/download/v1.2.3/rustman-windows-x86_64.zip",
            "https://github.com.evil.test/asset",
            "https://github.com/other/repo/releases/download/v1.2.3/rustman-windows-x86_64.zip",
            "https://127.0.0.1/payload",
        ] {
            asset.browser_download_url = url.to_owned();
            assert!(validate_asset_url(&asset, "v1.2.3").is_err());
        }
        asset.browser_download_url = good.to_owned();
        assert!(validate_asset_url(&asset, "v1.2.3").is_ok());
        assert!(validate_asset_url(&asset, "v1.2.4").is_err());
        assert!(allowed_redirect(
            &reqwest::Url::parse("https://release-assets.githubusercontent.com/fixture").unwrap()
        ));
        for url in [
            "http://release-assets.githubusercontent.com/fixture",
            "https://release-assets.githubusercontent.com.evil.test/fixture",
            "https://release-assets.githubusercontent.com:444/fixture",
            "https://user@release-assets.githubusercontent.com/fixture",
            "https://127.0.0.1/fixture",
        ] {
            assert!(!allowed_redirect(&reqwest::Url::parse(url).unwrap()));
        }
    }

    #[test]
    fn signed_checksum_requires_exact_name_unique_hash_and_unchanged_bytes() {
        use sha2::{Digest, Sha256};
        let name = "rustman-windows-x86_64.zip";
        let hash = Sha256::digest(b"fixture")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let manifest = format!("{hash}  {name}\n");
        assert!(verify_checksum(manifest.as_bytes(), name, b"fixture").is_ok());
        assert!(verify_checksum(manifest.as_bytes(), name, b"tampered").is_err());
        assert!(verify_checksum(manifest.as_bytes(), "한글-日本語.zip", b"fixture").is_err());
        assert!(
            verify_checksum(format!("{manifest}{manifest}").as_bytes(), name, b"fixture").is_err()
        );
        assert!(verify_checksum(b"invalid checksum", name, b"fixture").is_err());
        assert!(verify_checksum(b"", name, b"fixture").is_err());
    }

    #[test]
    fn minisign_rejects_modified_manifest_and_modified_trusted_comment() {
        // Public, non-secret upstream minisign-verify test vector for b"test".
        let key = minisign_verify::PublicKey::from_base64(
            "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3",
        )
        .unwrap();
        let signature = b"untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
        assert!(verify_manifest_signature(&key, b"test", signature).is_ok());
        assert!(verify_manifest_signature(&key, b"Test", signature).is_err());
        let modified = String::from_utf8(signature.to_vec())
            .unwrap()
            .replace("file:test", "file:evil");
        assert!(verify_manifest_signature(&key, b"test", modified.as_bytes()).is_err());
        assert!(verify_manifest_signature(&key, b"test", b"not a signature").is_err());
    }

    #[test]
    fn decompression_enforces_declared_size_before_reading() {
        assert!(bounded_binary(std::io::empty(), MAX_BINARY + 1).is_err());
        assert!(bounded_binary(b"abc".as_slice(), 1).is_err());
        assert_eq!(bounded_binary(b"abc".as_slice(), 3).unwrap(), b"abc");
    }

    #[test]
    fn verifies_a_structural_native_fixture() {
        let mut bytes = vec![0u8; 128];
        #[cfg(target_os = "windows")]
        {
            bytes[..2].copy_from_slice(b"MZ");
            bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
            bytes[64..68].copy_from_slice(b"PE\0\0");
        }
        #[cfg(target_os = "linux")]
        bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        #[cfg(target_os = "macos")]
        bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        let path = std::env::temp_dir().join(format!("rustman-native-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, bytes).unwrap();
        let result = verify_binary(&path);
        std::fs::remove_file(&path).unwrap();
        result.unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn rejects_traversal_ads_and_unicode_nested_binary_names() {
        for name in [
            "../rustman.exe",
            "C:\\rustman.exe",
            "한글/日本語/rustman.exe",
            "rustman.exe:script",
            "rustmаn.exe",
        ] {
            let path =
                std::env::temp_dir().join(format!("rustman-archive-{}", uuid::Uuid::new_v4()));
            let result = extract_binary(&release_zip(name, b"fixture"), &path);
            let _ = std::fs::remove_file(&path);
            assert!(result.is_err(), "accepted {name}");
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn rejects_archive_symlink_and_excessive_entries() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.add_symlink(
            BIN_NAME,
            "harmless-target",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let path = std::env::temp_dir().join(format!("rustman-archive-{}", uuid::Uuid::new_v4()));
        let result = extract_binary(&archive, &path);
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err());
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(BIN_NAME, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"fixture").unwrap();
        for index in 0..MAX_ENTRIES {
            zip.start_file(
                format!("한글-日本語-{index}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        }
        let archive = zip.finish().unwrap().into_inner();
        let result = extract_binary(&archive, &path);
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn extracts_exact_root_binary_with_unicode_unrelated_files() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, contents) in [
            ("한글-日本語.txt", b"unrelated".as_slice()),
            (BIN_NAME, b"fixture".as_slice()),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(contents).unwrap();
        }
        let path = std::env::temp_dir().join(format!("rustman-archive-{}", uuid::Uuid::new_v4()));
        extract_binary(&zip.finish().unwrap().into_inner(), &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"fixture");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn tar_rejects_links_and_duplicate_binary_entries() {
        for link in [false, true] {
            let mut builder = tar::Builder::new(Vec::new());
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o755);
            if link {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                header.set_link_name("harmless-target").unwrap();
                builder
                    .append_data(&mut header, "rustman", std::io::empty())
                    .unwrap();
            } else {
                for _ in 0..2 {
                    header.set_size(7);
                    builder
                        .append_data(&mut header, "rustman", b"fixture".as_slice())
                        .unwrap();
                }
            }
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            std::io::Write::write_all(&mut gz, &builder.into_inner().unwrap()).unwrap();
            let path = std::env::temp_dir().join(format!("rustman-tar-{}", uuid::Uuid::new_v4()));
            let result = extract_tar_binary(&gz.finish().unwrap(), &path);
            let _ = std::fs::remove_file(&path);
            assert!(result.is_err());
        }
    }

    #[test]
    fn version_compare() {
        assert!(version_gt("0.4.0", "0.3.2"));
        assert!(version_gt("1.0.0", "0.9.9"));
        assert!(version_gt("0.3.10", "0.3.2"));
        assert!(!version_gt("0.3.2", "0.3.2"));
        assert!(!version_gt("0.3.1", "0.3.2"));
        // pre-release suffix is ignored, so a dev tag of the current version
        // is not treated as an upgrade.
        assert!(!version_gt("0.3.2-dev.5", "0.3.2"));
    }

    fn asset(name: &str) -> GhAsset {
        GhAsset {
            name: name.to_owned(),
            browser_download_url: format!("https://example.test/{name}"),
        }
    }

    #[test]
    fn picks_the_tarball_not_the_dmg_when_both_contain_the_target_substring() {
        // Regression test for the real "invalid Gzip header" self-update crash
        // on macOS: `Rustman-macos-universal.dmg` and
        // `Rustman-macos-universal.tar.gz` both contain "macos-universal", so
        // a plain substring match could grab whichever the API happens to
        // list first — including the DMG, whose raw bytes aren't gzip at all.
        let assets = vec![
            asset("Rustman-macos-universal.dmg"),
            asset("Rustman-macos-universal.tar.gz"),
        ];
        let picked = pick_asset(&assets, "v1.2.3", "macos-universal", ".tar.gz")
            .expect("should find the tarball");
        assert_eq!(picked.name, "Rustman-macos-universal.tar.gz");
    }

    #[test]
    fn picks_the_tarball_even_when_it_is_listed_before_the_dmg() {
        let assets = vec![
            asset("Rustman-macos-universal.tar.gz"),
            asset("Rustman-macos-universal.dmg"),
        ];
        let picked = pick_asset(&assets, "v1.2.3", "macos-universal", ".tar.gz")
            .expect("should find the tarball");
        assert_eq!(picked.name, "Rustman-macos-universal.tar.gz");
    }

    #[test]
    fn errors_clearly_when_no_matching_asset_exists() {
        let assets = vec![asset("Rustman-macos-universal.dmg")];
        let err = pick_asset(&assets, "v1.2.3", "macos-universal", ".tar.gz").unwrap_err();
        assert!(
            err.contains("v1.2.3"),
            "error should name the release: {err}"
        );
    }

    #[test]
    fn never_picks_a_nokit_asset() {
        // The old no-WebKit build uploaded a second Linux tarball whose name
        // also contained `linux-x86_64`; an update must install the standard
        // build, not that variant, when both are present on an older release.
        let assets = vec![
            asset("rustman-linux-x86_64-nokit.tar.gz"),
            asset("rustman-linux-x86_64.tar.gz"),
        ];
        let picked = pick_asset(&assets, "v1.2.3", "linux-x86_64", ".tar.gz")
            .expect("should find the standard tarball");
        assert_eq!(picked.name, "rustman-linux-x86_64.tar.gz");
    }

    #[test]
    fn asset_target_matches_the_running_platform() {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        assert_eq!(asset_target().unwrap(), "linux-x86_64");
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        assert_eq!(asset_target().unwrap(), "linux-aarch64");
        #[cfg(target_os = "macos")]
        assert_eq!(asset_target().unwrap(), "macos-universal");
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        assert_eq!(asset_target().unwrap(), "windows-x86_64");
        #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
        assert_eq!(asset_target().unwrap(), "windows-aarch64");
    }

    #[test]
    fn strips_deleted_suffix_left_by_proc_self_exe_after_self_replace() {
        // Regression test for the real "Binary not found ... (deleted)" crash:
        // after self_replace renames the new binary onto the running
        // process's own path, /proc/self/exe keeps resolving to the old,
        // now-unlinked inode with this literal suffix appended.
        let deleted =
            std::path::PathBuf::from("/home/anime/work/rustman/target/debug/rustman (deleted)");
        let real = strip_deleted_suffix(deleted);
        assert_eq!(
            real,
            std::path::PathBuf::from("/home/anime/work/rustman/target/debug/rustman")
        );
    }

    #[test]
    fn leaves_a_normal_path_untouched() {
        let normal = std::path::PathBuf::from("/usr/local/bin/rustman");
        assert_eq!(strip_deleted_suffix(normal.clone()), normal);
    }

    /// A tar.gz laid out the way release.yml packs it: `dir` (if any) as its
    /// own directory entry first, as GNU tar writes it, then the binary.
    fn release_tarball(dir: Option<&str>, binary: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let path = match dir {
            Some(dir) => {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
                builder
                    .append_data(&mut header, format!("{dir}/"), std::io::empty())
                    .unwrap();
                format!("{dir}/rustman")
            }
            None => "rustman".to_owned(),
        };
        let mut header = tar::Header::new_gnu();
        header.set_size(binary.len() as u64);
        header.set_mode(0o755);
        builder.append_data(&mut header, path, binary).unwrap();
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar).unwrap();
        gz.finish().unwrap()
    }

    fn assert_extracts_from(dir: Option<&str>) {
        let binary = b"\x7fELF and the rest of a binary";
        let out = std::env::temp_dir().join(format!(
            "rustman-extract-test-{}-{}",
            uuid::Uuid::new_v4(),
            dir.unwrap_or("root")
        ));
        let result = extract_tar_binary(&release_tarball(dir, binary), &out);
        let extracted = std::fs::read(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        result.unwrap();
        assert_eq!(extracted, binary);
    }

    /// Regression test: the Linux tarball opens with a `rustman/` directory
    /// entry, whose file name is also `rustman`, and extracting that gave an
    /// empty file ("extracted binary is too small").
    #[test]
    fn extracts_the_binary_from_the_linux_tarball_layout() {
        assert_extracts_from(Some("rustman"));
    }

    #[test]
    fn extracts_the_binary_from_the_macos_tarball_layout() {
        assert_extracts_from(None);
    }
}
