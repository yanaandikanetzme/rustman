//! Reads every importable resource out of a git repository, for the
//! repo-load paths (clone, open folder, pull, switch branch, restore).
//!
//! A repo is not required to hold rustman-native files: a clone of a Postman
//! export (or an OpenAPI spec) populates the collection list too, by reusing
//! the shared import pipeline. Foreign resources get a deterministic id so a
//! re-scan updates in place instead of duplicating the collection.

use std::path::{Path, PathBuf};

use crate::domain::environment::AppEnvironment;
use crate::domain::group::ImportedCollection;
use crate::services::import::resources::{self, Resource};

/// Extensions the importer understands. Kept in sync with the import
/// dialog's folder scan (`resources::collect_files`).
const CANDIDATE_EXTENSIONS: &[&str] = &["json", "yaml", "yml", "txt", "curl", "sh"];

/// Upper bound on entries visited, so a huge repo (or node_modules) cannot
/// make a repo load run away.
const MAX_ENTRIES: usize = 2048;

/// Everything a repo load produces.
#[derive(Debug, Default)]
pub struct RepoScan {
    pub collections: Vec<ImportedCollection>,
    pub environments: Vec<AppEnvironment>,
    /// True when at least one collection came from a rustman-native file, so
    /// the repo is managed (write-back allowed) rather than import-only.
    pub saw_native: bool,
}

impl RepoScan {
    /// A repo holding only foreign (Postman/OpenAPI/…) collections is an
    /// import source: its working tree must never be rewritten.
    pub fn is_protected(&self) -> bool {
        !self.saw_native && !self.collections.is_empty()
    }
}

/// Scan a repository working directory. `repo_key` disambiguates the ids of
/// foreign files that carry no stable id of their own.
pub fn scan_dir(dir: &Path, repo_key: &str) -> RepoScan {
    let mut out = RepoScan::default();
    let mut budget = 0usize;
    for path in collect_files(dir) {
        let relative = relative_key(dir, &path);
        let file_key = format!("{repo_key}/{relative}");
        let Some(text) = read_text(&path, &mut budget) else {
            continue;
        };
        absorb(&text, &file_key, &mut out);
    }
    out
}

/// Scan a specific commit's tree, for restoring a commit into the app.
pub fn scan_commit(repo_path: &Path, commit_id: &str, repo_key: &str) -> Result<RepoScan, String> {
    let repo = git2::Repository::open(repo_path).map_err(|e| e.to_string())?;
    let oid = git2::Oid::from_str(commit_id).map_err(|e| e.to_string())?;
    let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
    let tree = commit.tree().map_err(|e| e.to_string())?;

    let mut blobs = Vec::new();
    let mut visited = 0usize;
    let mut remaining = crate::consts::IMPORT_TOTAL_LIMIT;
    collect_tree(&repo, &tree, "", &mut visited, &mut blobs, &mut remaining);

    let mut out = RepoScan::default();
    let mut budget = 0usize;
    for (path, bytes) in blobs {
        budget += bytes.len();
        if budget > crate::consts::IMPORT_TOTAL_LIMIT {
            break;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        absorb(text, &format!("{repo_key}/{path}"), &mut out);
    }
    Ok(out)
}

/// Parse one file's text and merge whatever it yields into `out`. Errors are
/// ignored: one unreadable file must not abort a whole repo load.
fn absorb(text: &str, file_key: &str, out: &mut RepoScan) {
    let Ok(resources) = resources::parse_all(text) else {
        return;
    };
    let native = is_native_collection(text);
    if native {
        out.saw_native = true;
    }
    for resource in resources {
        match resource {
            Resource::Collections(collections) => {
                for (index, mut imported) in collections.into_iter().enumerate() {
                    if !native {
                        imported.collection.id = stable_id(text, file_key, index);
                    }
                    out.collections.push(imported);
                }
            }
            Resource::Environments(environments) => out.environments.extend(environments),
            // Command files (`.curl`, `.sh`) are not collections.
            Resource::Command(..) | Resource::Commands(..) => {}
        }
    }
}

/// A deterministic id for a foreign collection so re-scanning the same file
/// keeps updating the same collection. A Postman export carries its own
/// `_postman_id`, which survives a file rename; anything else falls back to
/// the file's path inside the repo.
fn stable_id(text: &str, file_key: &str, index: usize) -> String {
    postman_id(text).unwrap_or_else(|| format!("repo:{file_key}#{index}"))
}

fn postman_id(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let id = value.get("info")?.get("_postman_id")?.as_str()?.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// The rustman-native collection shape: `{collection, requests, groups}`.
fn is_native_collection(text: &str) -> bool {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    map.get("collection").is_some_and(|c| c.is_object())
        && map.get("requests").is_some_and(|r| r.is_array())
}

fn collect_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut visited = 0usize;
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_ENTRIES {
                files.sort();
                return files;
            }
            let path = entry.path();
            // Hidden entries cover `.git`, `.idea`, `.github`, `.gitignore`.
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if candidate(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn collect_tree(
    repo: &git2::Repository,
    tree: &git2::Tree,
    prefix: &str,
    visited: &mut usize,
    blobs: &mut Vec<(String, Vec<u8>)>,
    remaining: &mut usize,
) {
    for entry in tree.iter() {
        *visited += 1;
        if *visited > MAX_ENTRIES || *remaining == 0 {
            return;
        }
        let name = entry.name().unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        let path = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                if let Ok(sub) = repo.find_tree(entry.id()) {
                    collect_tree(repo, &sub, &path, visited, blobs, remaining);
                }
            }
            Some(git2::ObjectType::Blob) => {
                if !candidate(Path::new(name)) {
                    continue;
                }
                // Read the object header before loading/decompressing its body.
                // The same remaining budget is shared through every subtree.
                let Ok(odb) = repo.odb() else {
                    continue;
                };
                let Ok((size, kind)) = odb.read_header(entry.id()) else {
                    continue;
                };
                if kind != git2::ObjectType::Blob
                    || size > crate::consts::IMPORT_FILE_LIMIT
                    || size > *remaining
                {
                    continue;
                }
                if let Ok(blob) = repo.find_blob(entry.id()) {
                    // Defend the copy boundary even if a custom ODB disagrees.
                    if blob.size() != size {
                        continue;
                    }
                    *remaining -= size;
                    blobs.push((path, blob.content().to_vec()));
                }
            }
            _ => {}
        }
    }
}

fn candidate(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| CANDIDATE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn relative_key(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Read a candidate file as UTF-8 within the shared import budgets. Returns
/// `None` when it is too large, unreadable, or not UTF-8.
fn read_text(path: &Path, budget: &mut usize) -> Option<String> {
    use std::io::Read;

    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > crate::consts::IMPORT_FILE_LIMIT as u64 {
        return None;
    }
    let remaining = crate::consts::IMPORT_TOTAL_LIMIT
        .saturating_sub(*budget)
        .min(crate::consts::IMPORT_FILE_LIMIT);
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(remaining as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > crate::consts::IMPORT_FILE_LIMIT {
        return None;
    }
    *budget += bytes.len();
    if *budget > crate::consts::IMPORT_TOTAL_LIMIT {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    Some(
        text.strip_prefix('\u{feff}')
            .map(str::to_owned)
            .unwrap_or(text),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustman-repo-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn postman(id: &str, name: &str) -> String {
        format!(
            r#"{{"info":{{"_postman_id":"{id}","name":"{name}","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"}},"item":[],"variable":[{{"key":"accessToken","value":"t"}}]}}"#
        )
    }

    #[test]
    fn security_commit_budget_is_shared_by_subtrees_and_skips_nonfitting_blobs() {
        let dir = temp_dir("nested-budget");
        let repo = git2::Repository::init(&dir).unwrap();
        let small = repo.blob(&[b'x'; 32]).unwrap();
        let large = repo.blob(&[b'y'; 80]).unwrap();
        let mut child = repo.treebuilder(None).unwrap();
        child.insert("a-large.json", large, 0o100644).unwrap();
        child.insert("b-small.json", small, 0o100644).unwrap();
        let child_id = child.write().unwrap();
        let mut root = repo.treebuilder(None).unwrap();
        for name in ["first", "second", "third"] {
            root.insert(name, child_id, 0o040000).unwrap();
        }
        let tree_id = root.write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let mut blobs = Vec::new();
        let mut remaining = 64;
        collect_tree(&repo, &tree, "", &mut 0, &mut blobs, &mut remaining);
        assert_eq!(
            blobs
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["first/b-small.json", "second/b-small.json"]
        );
        assert_eq!(
            blobs.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
            64
        );
        assert_eq!(remaining, 0);
        drop(tree);
        drop(root);
        drop(child);
        drop(repo);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn security_bounded_commit_scan_still_restores_foreign_collections() {
        let dir = temp_dir("restore-bounded");
        let repo = git2::Repository::init(&dir).unwrap();
        let mut builder = repo.treebuilder(None).unwrap();
        for (id, name) in [("id-a", "A"), ("id-b", "B")] {
            let blob = repo.blob(postman(id, name).as_bytes()).unwrap();
            builder
                .insert(format!("{name}.json"), blob, 0o100644)
                .unwrap();
        }
        let tree_id = builder.write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Synthetic QA", "synthetic@example.invalid").unwrap();
        let commit = repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                "synthetic collections",
                &tree,
                &[],
            )
            .unwrap();
        let scan = scan_commit(&dir, &commit.to_string(), "synthetic").unwrap();
        assert_eq!(
            scan.collections
                .iter()
                .map(|c| c.collection.id.as_str())
                .collect::<Vec<_>>(),
            ["id-a", "id-b"]
        );
        assert_eq!(scan.environments.len(), 2);
        assert!(scan.is_protected());
        drop(tree);
        drop(builder);
        drop(repo);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn security_commit_blob_budget_is_enforced_before_copying() {
        let dir = temp_dir("blob-budget");
        let repo = git2::Repository::init(&dir).unwrap();
        // Nine references to one tiny blob reproduce aggregate growth without
        // allocating hundreds of MiB or attempting to exhaust the host.
        let oid = repo.blob(&[b'x'; 32]).unwrap();
        let mut builder = repo.treebuilder(None).unwrap();
        for i in 0..9 {
            builder.insert(format!("{i}.json"), oid, 0o100644).unwrap();
        }
        let tree_id = builder.write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let mut blobs = Vec::new();
        let mut remaining = 128;
        collect_tree(&repo, &tree, "", &mut 0, &mut blobs, &mut remaining);
        let retained: usize = blobs.iter().map(|(_, bytes)| bytes.len()).sum();
        assert!(
            retained <= 128,
            "retained {retained} bytes before the aggregate budget check"
        );
        assert_eq!(blobs.len(), 4);
        assert_eq!(remaining, 0);
        drop(tree);
        drop(builder);
        drop(repo);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn postman_files_yield_stable_collections_and_environments() {
        let dir = temp_dir("postman");
        std::fs::write(dir.join("A.postman_collection.json"), postman("id-a", "A")).unwrap();
        std::fs::write(dir.join("B.postman_collection.json"), postman("id-b", "B")).unwrap();
        // Ignored: hidden dir, non-candidate extension, malformed JSON.
        std::fs::create_dir_all(dir.join(".idea")).unwrap();
        std::fs::write(dir.join(".idea/misc.xml"), "<project/>").unwrap();
        std::fs::write(dir.join("README.md"), "# repo").unwrap();
        std::fs::write(dir.join("broken.json"), "{ not json").unwrap();

        let scan = scan_dir(&dir, "https://example.com/repo.git");
        let ids: Vec<&str> = scan
            .collections
            .iter()
            .map(|c| c.collection.id.as_str())
            .collect();
        assert_eq!(ids, ["id-a", "id-b"], "sorted, malformed file skipped");
        assert_eq!(scan.environments.len(), 2);
        assert!(scan.is_protected(), "foreign repo is import-only");

        // Re-scanning yields the same ids: re-import updates, never duplicates.
        let again = scan_dir(&dir, "https://example.com/repo.git");
        let ids_again: Vec<&str> = again
            .collections
            .iter()
            .map(|c| c.collection.id.as_str())
            .collect();
        assert_eq!(ids, ids_again);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_files_keep_their_ids_and_mark_the_repo_managed() {
        let dir = temp_dir("native");
        std::fs::write(
            dir.join("col-1.json"),
            r#"{"collection":{"id":"col-1","name":"C","created_at":0},"requests":[]}"#,
        )
        .unwrap();
        let scan = scan_dir(&dir, "local");
        assert_eq!(scan.collections.len(), 1);
        assert_eq!(scan.collections[0].collection.id, "col-1");
        assert!(scan.saw_native);
        assert!(!scan.is_protected());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn non_postman_foreign_files_get_a_path_based_id() {
        let dir = temp_dir("swagger");
        std::fs::create_dir_all(dir.join("specs")).unwrap();
        std::fs::write(
            dir.join("specs/api.json"),
            r#"{"openapi":"3.0.0","info":{"title":"X","version":"1"},"paths":{}}"#,
        )
        .unwrap();
        let scan = scan_dir(&dir, "key");
        assert_eq!(scan.collections.len(), 1);
        assert_eq!(
            scan.collections[0].collection.id,
            "repo:key/specs/api.json#0"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
