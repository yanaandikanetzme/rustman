use std::collections::HashSet;
use std::path::{Path, PathBuf};

use git2::{
    BranchType, DiffFormat, Repository, Signature, Sort, StatusOptions, build::CheckoutBuilder,
};
use serde_json;

use crate::domain::collection::{Collection, SavedRequest};
use crate::domain::group::{ImportedCollection, ImportedGroup};

fn open_at(path: &PathBuf) -> Result<Repository, String> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    match Repository::open(path) {
        Ok(repo) => Ok(repo),
        Err(_) => Repository::init(path).map_err(|e| e.to_string()),
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CollectionFile {
    collection: Collection,
    requests: Vec<SavedRequest>,
    /// Folder tree (issue #53). Defaulted so repos committed by older
    /// rustman versions — which wrote no `groups` key — still load.
    #[serde(default)]
    groups: Vec<ImportedGroup>,
}

/// Filename the global script pair is stored under, at the repo root
/// (sibling to the per-collection `{id}.json` files). Its `.json`
/// extension means it would otherwise be picked up by `load_collections`'s
/// blanket `*.json` scan — but that scan only keeps files that successfully
/// deserialize as `CollectionFile`, so it's silently skipped there.
const GLOBAL_SCRIPT_FILE: &str = "global.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct GlobalScriptFile {
    pre_request_script: String,
    test_script: String,
}

/// Reads the global script pair out of a repo's current working tree.
/// `None` if the repo has none saved (never committed, or this is a repo
/// that predates the global script feature).
pub fn read_global_script(data_dir: &PathBuf) -> Option<(String, String)> {
    let repo = open_at(data_dir).ok()?;
    let workdir = repo.workdir()?;
    let json = read_regular_json(&workdir.join(GLOBAL_SCRIPT_FILE)).ok()?;
    let file: GlobalScriptFile = serde_json::from_str(&json).ok()?;
    Some((file.pre_request_script, file.test_script))
}

/// Reads the global script pair as it existed at a specific past commit —
/// the global-script counterpart to a restore, used when restoring a
/// commit rather than reading the live working tree.
pub fn read_commit_global_script(data_dir: &PathBuf, commit_id: &str) -> Option<(String, String)> {
    let repo = open_at(data_dir).ok()?;
    let oid = git2::Oid::from_str(commit_id).ok()?;
    let commit = repo.find_commit(oid).ok()?;
    let tree = commit.tree().ok()?;
    let entry = tree.get_path(Path::new(GLOBAL_SCRIPT_FILE)).ok()?;
    let object = entry.to_object(&repo).ok()?;
    let blob = object.as_blob()?;
    let file: GlobalScriptFile = serde_json::from_slice(blob.content()).ok()?;
    Some((file.pre_request_script, file.test_script))
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub id: String,
    pub message: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub state: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RepoStatus {
    pub branch: String,
    pub changes: Vec<FileChange>,
    pub remote_url: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

pub fn set_identity(data_dir: &PathBuf, name: &str, email: &str) -> Result<(), String> {
    let repo = open_at(data_dir)?;
    let mut cfg = repo.config().map_err(|e| e.to_string())?;
    cfg.set_str("user.name", name).map_err(|e| e.to_string())?;
    cfg.set_str("user.email", email)
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn resolve_identity(data_dir: &PathBuf) -> Option<GitIdentity> {
    if let Ok(repo) = open_at(data_dir) {
        if let Ok(cfg) = repo.config() {
            let name = cfg.get_string("user.name").ok().filter(|s| !s.is_empty());
            let email = cfg.get_string("user.email").ok().filter(|s| !s.is_empty());
            if let (Some(name), Some(email)) = (name, email) {
                return Some(GitIdentity { name, email });
            }
        }
    }
    None
}

pub fn status(data_dir: &PathBuf) -> Result<RepoStatus, String> {
    let repo = open_at(data_dir)?;
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    let statuses = repo.statuses(Some(&mut opts)).map_err(|e| e.to_string())?;

    let mut changes = Vec::new();
    for entry in statuses.iter() {
        let s = entry.status();
        let state = if s.is_wt_new() || s.is_index_new() {
            "new"
        } else if s.is_wt_deleted() || s.is_index_deleted() {
            "deleted"
        } else if s.is_wt_renamed() || s.is_index_renamed() {
            "renamed"
        } else if s.is_wt_modified() || s.is_index_modified() {
            "modified"
        } else {
            "changed"
        };
        if let Ok(path) = entry.path() {
            changes.push(FileChange {
                path: path.to_owned(),
                state: state.to_owned(),
            });
        }
    }

    let branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().ok().map(|s| s.to_owned()))
        .unwrap_or_else(|| "main".to_owned());
    let remote_url = repo
        .find_remote("origin")
        .ok()
        .and_then(|r| r.url().ok().map(|s| s.to_owned()));
    let (ahead, behind) = ahead_behind(&repo, &branch).unwrap_or((0, 0));

    Ok(RepoStatus {
        branch,
        changes,
        remote_url,
        ahead,
        behind,
    })
}

fn ahead_behind(repo: &Repository, branch: &str) -> Option<(usize, usize)> {
    let local = repo.refname_to_id(&format!("refs/heads/{branch}")).ok()?;
    let upstream = repo
        .refname_to_id(&format!("refs/remotes/origin/{branch}"))
        .ok()?;
    repo.graph_ahead_behind(local, upstream).ok()
}

pub fn commit(data_dir: &PathBuf, identity: &GitIdentity, message: &str) -> Result<String, String> {
    let repo = open_at(data_dir)?;
    let workdir = repo
        .workdir()
        .ok_or("Repository has no working directory")?;
    let mut working_index = repo.index().map_err(|e| e.to_string())?;
    if working_index.has_conflicts() {
        return Err("Resolve Git conflicts before committing".into());
    }
    // Construct a tree independent of the user's staging area. Pre-staged
    // unrelated credentials must never become part of a Rustman commit.
    let mut index = git2::Index::new().map_err(|e| e.to_string())?;
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let mut managed = HashSet::new();
    if let Some(parent) = &parent {
        let tree = parent.tree().map_err(|e| e.to_string())?;
        index.read_tree(&tree).map_err(|e| e.to_string())?;
        let odb = repo.odb().map_err(|e| e.to_string())?;
        for entry in tree.iter() {
            let Ok(name) = entry.name() else {
                continue;
            };
            let collection_id = name
                .strip_suffix(".json")
                .filter(|id| safe_collection_id(id));
            if name != GLOBAL_SCRIPT_FILE && collection_id.is_none() {
                continue;
            }
            if !matches!(entry.filemode(), 0o100644 | 0o100755) {
                continue;
            }
            let (size, kind) = odb.read_header(entry.id()).map_err(|e| e.to_string())?;
            if size as u64 > MAX_COLLECTION_FILE_BYTES || kind != git2::ObjectType::Blob {
                continue;
            }
            let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
            let recognized = if let Some(id) = collection_id {
                serde_json::from_slice::<CollectionFile>(blob.content())
                    .is_ok_and(|f| f.collection.id == id)
            } else {
                serde_json::from_slice::<GlobalScriptFile>(blob.content()).is_ok()
            };
            if recognized {
                managed.insert(name.to_owned());
                index
                    .remove_path(Path::new(name))
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    for entry in std::fs::read_dir(workdir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let recognized = if name == GLOBAL_SCRIPT_FILE {
            if let Ok(global) = serde_json::from_str::<GlobalScriptFile>(&read_regular_json(&path)?)
            {
                crate::services::secrets::validate_shared_script(&global.pre_request_script)?;
                crate::services::secrets::validate_shared_script(&global.test_script)?;
                true
            } else {
                false
            }
        } else if let Some(file) = managed_collection(&path)? {
            crate::services::secrets::validate_shared_requests(&file.requests, &file.groups)?;
            true
        } else {
            false
        };
        if !recognized {
            continue;
        }
        let content = read_regular_json(&path)?;
        let oid = repo.blob(content.as_bytes()).map_err(|e| e.to_string())?;
        index
            .add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o100644,
                uid: 0,
                gid: 0,
                file_size: content.len() as u32,
                id: oid,
                flags: 0,
                flags_extended: 0,
                path: name.as_bytes().to_vec(),
            })
            .map_err(|e| e.to_string())?;
        managed.insert(name.to_owned());
    }
    let tree_id = index.write_tree_to(&repo).map_err(|e| e.to_string())?;
    // Update only managed staging entries; preserve every unrelated staged file.
    for name in &managed {
        let path = Path::new(name);
        if working_index.get_path(path, 0).is_some() {
            working_index.remove_path(path).map_err(|e| e.to_string())?;
        }
        if let Some(entry) = index.get_path(path, 0) {
            working_index.add(&entry).map_err(|e| e.to_string())?;
        }
    }
    working_index.write().map_err(|e| e.to_string())?;
    let tree = repo.find_tree(tree_id).map_err(|e| e.to_string())?;
    let sig = Signature::now(&identity.name, &identity.email).map_err(|e| e.to_string())?;
    let parents: Vec<_> = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .map_err(|e| e.to_string())?;
    Ok(oid.to_string())
}

pub fn full_log(data_dir: &PathBuf, limit: usize) -> Vec<CommitInfo> {
    let repo = match open_at(data_dir) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let mut revwalk = match repo.revwalk() {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    if revwalk.push_head().is_err() {
        return vec![];
    }
    let _ = revwalk.set_sorting(Sort::TIME);
    revwalk
        .filter_map(|oid| {
            let oid = oid.ok()?;
            let commit = repo.find_commit(oid).ok()?;
            Some(CommitInfo {
                id: oid.to_string(),
                message: commit.message().unwrap_or("").to_owned(),
                timestamp: commit.time().seconds(),
            })
        })
        .take(limit)
        .collect()
}

pub fn working_diff(data_dir: &PathBuf) -> Result<String, String> {
    let repo = open_at(data_dir)?;
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let diff = repo
        .diff_tree_to_workdir_with_index(head_tree.as_ref(), None)
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
        let origin = line.origin();
        if origin == '+' || origin == '-' || origin == ' ' {
            out.push(origin);
        }
        out.push_str(&String::from_utf8_lossy(line.content()));
        true
    })
    .map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn branches(data_dir: &PathBuf) -> Result<Vec<BranchInfo>, String> {
    let repo = open_at(data_dir)?;
    let iter = repo
        .branches(Some(BranchType::Local))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for item in iter {
        let (branch, _) = item.map_err(|e| e.to_string())?;
        let is_head = branch.is_head();
        if let Ok(Some(name)) = branch.name() {
            out.push(BranchInfo {
                name: name.to_owned(),
                is_head,
            });
        }
    }
    Ok(out)
}

pub fn create_branch(data_dir: &PathBuf, name: &str) -> Result<(), String> {
    let repo = open_at(data_dir)?;
    let commit = repo
        .head()
        .map_err(|e| e.to_string())?
        .peel_to_commit()
        .map_err(|e| e.to_string())?;
    repo.branch(name, &commit, false)
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn checkout_branch(data_dir: &PathBuf, name: &str) -> Result<(), String> {
    let repo = open_at(data_dir)?;
    let refname = format!("refs/heads/{name}");
    let object = repo.revparse_single(&refname).map_err(|e| e.to_string())?;
    let mut checkout = CheckoutBuilder::new();
    checkout.force();
    repo.checkout_tree(&object, Some(&mut checkout))
        .map_err(|e| e.to_string())?;
    repo.set_head(&refname).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn remote_url(data_dir: &PathBuf) -> Option<String> {
    let repo = open_at(data_dir).ok()?;
    repo.find_remote("origin")
        .ok()
        .and_then(|r| r.url().ok().map(|s| s.to_owned()))
}

pub fn set_remote(data_dir: &PathBuf, url: &str) -> Result<(), String> {
    let repo = open_at(data_dir)?;
    match repo.find_remote("origin") {
        Ok(_) => repo
            .remote_set_url("origin", url)
            .map_err(|e| e.to_string())?,
        Err(_) => {
            repo.remote("origin", url).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("could not run git (is it installed?): {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

pub fn clone_repo(url: &str, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if dest.join(".git").is_dir() {
        return Ok(());
    }
    let output = std::process::Command::new("git")
        .arg("clone")
        .arg(url)
        .arg(dest)
        .output()
        .map_err(|e| format!("could not run git (is it installed?): {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

pub fn fetch(data_dir: &PathBuf) -> Result<(), String> {
    run_git(data_dir, &["fetch", "--all"]).map(|_| ())
}

// Preflight seam for local-only publication-history regression coverage.
// Validate all reachable managed snapshots, including credentials removed at HEAD.
// Bounds are fail-closed and checked before allocating each Git object.
fn validate_push_history(repo: &Repository) -> Result<(), String> {
    let head = repo
        .head()
        .map_err(|e| e.to_string())?
        .target()
        .ok_or("Unresolved HEAD")?;
    let odb = repo.odb().map_err(|e| e.to_string())?;
    let mut pending = vec![head];
    let mut commits = HashSet::new();
    let mut trees = HashSet::new();
    let mut blobs = HashSet::new();
    let mut total = 0usize;
    while let Some(oid) = pending.pop() {
        if !commits.insert(oid) {
            continue;
        }
        if commits.len() > 2048 {
            return Err(
                "Push security scan exceeds 2048 commits; review history separately".into(),
            );
        }
        let (size, kind) = odb.read_header(oid).map_err(|e| e.to_string())?;
        if size > 1024 * 1024 || kind != git2::ObjectType::Commit {
            return Err("Invalid or oversized Git commit".into());
        }
        total = total.saturating_add(size);
        let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
        for parent in commit.parent_ids() {
            if pending.len() >= 4096 {
                return Err("Push security scan history queue exceeded".into());
            }
            pending.push(parent);
        }
        if !trees.insert(commit.tree_id()) {
            continue;
        }
        let (size, kind) = odb
            .read_header(commit.tree_id())
            .map_err(|e| e.to_string())?;
        if size > 4 * 1024 * 1024 || kind != git2::ObjectType::Tree {
            return Err("Invalid or oversized Git tree".into());
        }
        total = total.saturating_add(size);
        let tree = commit.tree().map_err(|e| e.to_string())?;
        for entry in tree.iter() {
            let Ok(name) = entry.name() else {
                continue;
            };
            if !name.ends_with(".json")
                || !matches!(entry.filemode(), 0o100644 | 0o100755)
                || !blobs.insert(entry.id())
            {
                continue;
            }
            if blobs.len() > 4096 {
                return Err("Push security scan exceeds 4096 snapshots".into());
            }
            let (size, kind) = odb.read_header(entry.id()).map_err(|e| e.to_string())?;
            total = total.saturating_add(size);
            if size as u64 > MAX_COLLECTION_FILE_BYTES
                || total > 256 * 1024 * 1024
                || kind != git2::ObjectType::Blob
            {
                return Err("Push security scan exceeds object or total byte budget".into());
            }
            let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
            // A blob can match multiple schemas; validate each independently so
            // deduplication cannot hide a global script under a collection name.
            if let Ok(file) = serde_json::from_slice::<GlobalScriptFile>(blob.content()) {
                crate::services::secrets::validate_shared_script(&file.pre_request_script)?;
                crate::services::secrets::validate_shared_script(&file.test_script)?;
            }
            if let Ok(file) = serde_json::from_slice::<CollectionFile>(blob.content()) {
                crate::services::secrets::validate_shared_requests(&file.requests, &file.groups)?;
            } else if let Ok(files) = serde_json::from_slice::<Vec<CollectionFile>>(blob.content())
            {
                for file in files {
                    crate::services::secrets::validate_shared_requests(
                        &file.requests,
                        &file.groups,
                    )?;
                }
            }
        }
        if total > 256 * 1024 * 1024 {
            return Err("Push security scan exceeds byte budget".into());
        }
    }
    Ok(())
}

pub fn push(data_dir: &PathBuf) -> Result<(), String> {
    let repo = open_at(data_dir)?;
    validate_push_history(&repo)?;
    run_git(data_dir, &["push", "-u", "origin", "HEAD"]).map(|_| ())
}

pub fn pull(data_dir: &PathBuf) -> Result<String, String> {
    let output = run_git(data_dir, &["pull", "--ff-only"])?;
    let trimmed = output.trim();
    Ok(if trimmed.is_empty() {
        "Up to date".to_owned()
    } else {
        trimmed.to_owned()
    })
}

const MAX_COLLECTION_FILE_BYTES: u64 = 32 * 1024 * 1024;

fn safe_collection_id(id: &str) -> bool {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return false;
    }
    let upper = id.to_ascii_uppercase();
    !matches!(upper.as_str(), "GLOBAL" | "CON" | "PRN" | "AUX" | "NUL")
        && !(upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
}

fn snapshot_exists(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            Err("Refusing a snapshot destination that is a link or non-regular file".into())
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn read_regular_bytes(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Collection snapshots must be regular files, not links".into());
    }
    if metadata.len() > MAX_COLLECTION_FILE_BYTES {
        return Err("Collection snapshot exceeds 32 MiB".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_COLLECTION_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_COLLECTION_FILE_BYTES {
        return Err("Collection snapshot exceeds 32 MiB".into());
    }
    Ok(bytes)
}

fn read_regular_json(path: &Path) -> Result<String, String> {
    String::from_utf8(read_regular_bytes(path)?).map_err(|_| "Snapshot is not UTF-8 JSON".into())
}

fn managed_collection(path: &Path) -> Result<Option<CollectionFile>, String> {
    if path.extension().and_then(|s| s.to_str()) != Some("json") {
        return Ok(None);
    }
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return Ok(None);
    };
    if !safe_collection_id(stem) {
        return Ok(None);
    }
    // Discovery must not read or reject arbitrary oversized/binary root JSON.
    // Explicit snapshot destinations are separately checked before overwriting.
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > MAX_COLLECTION_FILE_BYTES
        || metadata.file_type().is_symlink()
        || !metadata.is_file()
    {
        return Ok(None);
    }
    let bytes = read_regular_bytes(path)?;
    let Ok(file) = serde_json::from_slice::<CollectionFile>(&bytes) else {
        return Ok(None);
    };
    if file.collection.id != stem {
        return Err("Collection snapshot ID does not match its filename".into());
    }
    Ok(Some(file))
}

fn write_snapshot(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let staging = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&staging).map_err(|e| e.to_string())?;
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&staging, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result
}

pub fn write_working_tree(
    data_dir: &PathBuf,
    collections: &[ImportedCollection],
    global_pre_request_script: &str,
    global_test_script: &str,
) -> Result<(), String> {
    let repo = open_at(data_dir)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| "repository has no working directory".to_owned())?
        .to_path_buf();

    // Validate the complete batch and collisions before any write or deletion.
    let mut current = HashSet::new();
    let mut portable_ids = HashSet::new();
    let mut snapshots = Vec::new();
    for imported in collections {
        crate::services::secrets::validate_shared_requests(&imported.requests, &imported.groups)?;
        let id = &imported.collection.id;
        if !safe_collection_id(id) || !portable_ids.insert(id.to_ascii_lowercase()) {
            return Err("Unsafe, reserved or duplicate collection ID".into());
        }
        current.insert(id.clone());
        let path = workdir.join(format!("{id}.json"));
        if snapshot_exists(&path)? && managed_collection(&path)?.is_none() {
            return Err("Refusing to overwrite an unrelated JSON file".into());
        }
        let file = CollectionFile {
            collection: imported.collection.clone(),
            requests: imported.requests.clone(),
            groups: imported.groups.clone(),
        };
        let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        if text.len() as u64 > MAX_COLLECTION_FILE_BYTES {
            return Err("Collection snapshot exceeds 32 MiB".into());
        }
        snapshots.push((path, text));
    }
    let global_path = workdir.join(GLOBAL_SCRIPT_FILE);
    if snapshot_exists(&global_path)? {
        let text = read_regular_json(&global_path)?;
        serde_json::from_str::<GlobalScriptFile>(&text)
            .map_err(|_| "Refusing to overwrite unrelated global.json".to_owned())?;
    }
    crate::services::secrets::validate_shared_script(global_pre_request_script)?;
    crate::services::secrets::validate_shared_script(global_test_script)?;
    let global = if global_pre_request_script.is_empty() && global_test_script.is_empty() {
        None
    } else {
        let text = serde_json::to_string_pretty(&GlobalScriptFile {
            pre_request_script: global_pre_request_script.into(),
            test_script: global_test_script.into(),
        })
        .map_err(|e| e.to_string())?;
        if text.len() as u64 > MAX_COLLECTION_FILE_BYTES {
            return Err("Global script snapshot exceeds 32 MiB".into());
        }
        Some(text)
    };
    let mut remove = Vec::new();
    for entry in std::fs::read_dir(&workdir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        let path = entry.path();
        if let Some(file) = managed_collection(&path)? {
            if !current.contains(&file.collection.id) {
                remove.push(path);
            }
        }
    }
    for (path, text) in snapshots {
        write_snapshot(&path, &text)?;
    }
    if let Some(text) = global {
        write_snapshot(&global_path, &text)?;
    } else if snapshot_exists(&global_path)? {
        std::fs::remove_file(&global_path).map_err(|e| e.to_string())?;
    }
    for path in remove {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[cfg(test)]
mod prune_tests {
    use super::*;

    #[test]
    fn write_working_tree_prunes_only_rustman_files() {
        let dir = std::env::temp_dir().join(format!("rustman-vcs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let postman = dir.join("A.postman_collection.json");
        std::fs::write(
            &postman,
            r#"{"info":{"_postman_id":"x","name":"A"},"item":[]}"#,
        )
        .unwrap();
        let stale = dir.join("stale.json");
        std::fs::write(
            &stale,
            r#"{"collection":{"id":"stale","name":"S","created_at":0},"requests":[]}"#,
        )
        .unwrap();

        write_working_tree(&dir, &[], "", "").unwrap();

        assert!(postman.exists(), "a foreign Postman file must survive");
        assert!(!stale.exists(), "a stale rustman file is pruned");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[cfg(test)]
mod security_tests {
    use super::*;
    fn collection(id: &str) -> ImportedCollection {
        ImportedCollection {
            collection: Collection {
                id: id.into(),
                name: "한국 ไทย 一".into(),
                created_at: 0,
            },
            requests: vec![],
            groups: vec![],
        }
    }
    fn raw_snapshot_commit(repo: &Repository, dir: &Path, file: &CollectionFile) {
        std::fs::write(dir.join("c1.json"), serde_json::to_vec(file).unwrap()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("c1.json")).unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let signature = Signature::now("Test", "test@example.test").unwrap();
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<_> = parent.iter().collect();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "synthetic fixture",
            &tree,
            &parents,
        )
        .unwrap();
    }
    #[test]
    fn security_publication_sensitive_parent_applies_to_nested_objects_and_arrays() {
        for body in [
            r#"{"credentials":{"value":"synthetic-nested"}}"#,
            r#"{"password":{"value":"synthetic-nested"}}"#,
            r#"{"credentials":[{"value":"synthetic-nested"}]}"#,
        ] {
            let mut req = SavedRequest::new_in("c".into(), "Synthetic".into());
            req.body = body.into();
            assert!(crate::services::secrets::validate_shared_requests(&[req], &[]).is_err());
        }
    }

    #[test]
    fn security_publication_history_checks_secret_ancestor_even_when_head_is_clean() {
        let dir =
            std::env::temp_dir().join(format!("rustman-history-guard-{}", uuid::Uuid::new_v4()));
        let repo = open_at(&dir).unwrap();
        let mut req = SavedRequest::new_in("c1".into(), "Synthetic".into());
        req.bearer_token = "synthetic-historic-token".into();
        let mut file = CollectionFile {
            collection: collection("c1").collection,
            requests: vec![req],
            groups: vec![],
        };
        raw_snapshot_commit(&repo, &dir, &file);
        file.requests.clear();
        raw_snapshot_commit(&repo, &dir, &file);
        let head = repo.head().unwrap().target().unwrap();
        let result = validate_push_history(&repo);
        assert!(
            result.is_err(),
            "a clean HEAD must not hide a reachable secret-bearing ancestor"
        );
        assert!(!result.unwrap_err().contains("synthetic-historic-token"));
        assert_eq!(repo.head().unwrap().target().unwrap(), head);
        assert!(
            repo.remotes().unwrap().is_empty(),
            "this regression must have no network target"
        );
        drop(repo);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn security_publication_nested_and_inactive_credentials_are_blocked() {
        use crate::domain::request::KeyValue;
        let dir =
            std::env::temp_dir().join(format!("rustman-publish-nested-{}", uuid::Uuid::new_v4()));
        let base = SavedRequest::new_in("c1".into(), "Synthetic".into());
        let mut cases = Vec::new();
        let mut req = base.clone();
        req.basic_pass = "synthetic-inactive-password".into();
        cases.push(req);
        let mut req = base.clone();
        let mut h = KeyValue::new_empty();
        h.key = "Authorization".into();
        h.value = "Bearer synthetic-header".into();
        h.enabled = false;
        req.headers.push(h);
        cases.push(req);
        let mut req = base.clone();
        req.url = "https://example.test/?%61pi_key=synthetic-url".into();
        cases.push(req);
        let mut req = base.clone();
        req.body = r#"{"credentials":{"password":"synthetic-json"}}"#.into();
        cases.push(req);
        let mut req = base.clone();
        req.graphql_variables = r#"{"client_secret":"synthetic-graphql"}"#.into();
        cases.push(req);
        let mut req = base;
        req.pre_request_script =
            "-----BEGIN PRIVATE KEY-----\nsynthetic\n-----END PRIVATE KEY-----".into();
        cases.push(req);
        for req in cases {
            let mut imported = collection("c1");
            imported.groups.push(ImportedGroup {
                id: "g".into(),
                parent_id: "c1".into(),
                parent_is_collection: true,
                name: "Nested".into(),
                sort_order: 0,
                requests: vec![req],
                groups: vec![],
            });
            assert!(write_working_tree(&dir, &[imported], "", "").is_err());
            assert!(!dir.join("c1.json").exists());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn security_publication_templates_can_be_shared_and_explicit_export_is_lossless() {
        use crate::domain::request::KeyValue;
        let dir =
            std::env::temp_dir().join(format!("rustman-publish-ref-{}", uuid::Uuid::new_v4()));
        let mut imported = collection("c1");
        let mut req = SavedRequest::new_in("c1".into(), "Login".into());
        req.bearer_token = "{{TOKEN}}".into();
        req.basic_pass = "{{PASSWORD}}".into();
        req.api_key_value = "{{TOKEN}}".into();
        req.cookie_string = "sid={{SESSION}}".into();
        req.body = r#"{"password":"{{PASSWORD}}","user":"public"}"#.into();
        let mut h = KeyValue::new_empty();
        h.key = "Authorization".into();
        h.value = "Bearer {{TOKEN}}".into();
        req.headers.push(h);
        imported.requests.push(req.clone());
        let before = serde_json::to_string(&imported).unwrap();
        write_working_tree(&dir, &[imported.clone()], "", "").unwrap();
        let identity = GitIdentity {
            name: "Test".into(),
            email: "test@example.test".into(),
        };
        commit(&dir, &identity, "share references").unwrap();
        assert_eq!(serde_json::to_string(&imported).unwrap(), before);
        req.bearer_token = "synthetic-explicit-export".into();
        let exported = crate::services::import::native_export(&imported.collection, &[req], &[]);
        assert!(
            exported.contains("synthetic-explicit-export"),
            "explicit unmasked export must retain original values"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn security_publication_rejects_inline_credentials_before_any_snapshot_write() {
        let dir = std::env::temp_dir().join(format!("rustman-publish-{}", uuid::Uuid::new_v4()));
        write_working_tree(&dir, &[collection("existing")], "", "").unwrap();
        let original = std::fs::read(dir.join("existing.json")).unwrap();
        let mut imported = collection("unsafe");
        let mut request = SavedRequest::new_in("unsafe".into(), "Login".into());
        request.bearer_token = "synthetic-inline-token".into();
        imported.requests.push(request);
        let result = write_working_tree(&dir, &[collection("safe"), imported.clone()], "", "");
        assert!(
            result.is_err(),
            "literal credential must not enter a shared snapshot"
        );
        assert!(!result.unwrap_err().contains("synthetic-inline-token"));
        assert!(!dir.join("safe.json").exists());
        assert!(!dir.join("unsafe.json").exists());
        assert_eq!(std::fs::read(dir.join("existing.json")).unwrap(), original);
        assert_eq!(imported.requests[0].bearer_token, "synthetic-inline-token");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn security_publication_commit_rejects_manually_inserted_secret_snapshot() {
        let dir = std::env::temp_dir().join(format!("rustman-publish-{}", uuid::Uuid::new_v4()));
        let repo = open_at(&dir).unwrap();
        let mut request = SavedRequest::new_in("c1".into(), "Login".into());
        request.cookie_string = "sid=synthetic-cookie".into();
        let file = CollectionFile {
            collection: collection("c1").collection,
            requests: vec![request],
            groups: vec![],
        };
        std::fs::write(dir.join("c1.json"), serde_json::to_vec(&file).unwrap()).unwrap();
        let identity = GitIdentity {
            name: "Test".into(),
            email: "test@example.test".into(),
        };
        assert!(commit(&dir, &identity, "must not publish").is_err());
        assert!(
            repo.head().is_err(),
            "no commit may be created on rejection"
        );
        assert!(
            repo.index().unwrap().is_empty(),
            "index must remain unchanged"
        );
        drop(repo);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn vcs_security_rejects_unsafe_ids_before_mutation() {
        let root = std::env::temp_dir().join(format!("rustman-vcs-{}", uuid::Uuid::new_v4()));
        let dir = root.join("repo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.join("outside.json"), "preserve").unwrap();
        for id in [
            "../outside",
            "..\\outside",
            "x:y",
            "global",
            "CON",
            "一",
            "",
            ".",
            "..",
            "/absolute",
            "C:\\outside",
            "//server/share",
            "%2e%2e%2foutside",
            "c1/../../outside",
            "c1.json",
            "AUX",
            "com1",
            "LPT9",
            "trailing ",
            "．．",
            "a\0b",
        ] {
            assert!(
                write_working_tree(&dir, &[collection(id)], "", "").is_err(),
                "accepted unsafe collection ID"
            );
            assert_eq!(
                std::fs::read_to_string(root.join("outside.json")).unwrap(),
                "preserve"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn vcs_security_validates_batch_before_writing_and_preserves_collision() {
        let dir = std::env::temp_dir().join(format!("rustman-vcs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(
            write_working_tree(
                &dir,
                &[collection("safe"), collection("../outside")],
                "",
                ""
            )
            .is_err()
        );
        assert!(!dir.join("safe.json").exists());
        assert!(
            write_working_tree(&dir, &[collection("Safe"), collection("safe")], "", "").is_err()
        );
        assert!(!dir.join("Safe.json").exists());
        std::fs::write(dir.join("safe.json"), "unrelated data").unwrap();
        assert!(write_working_tree(&dir, &[collection("safe")], "", "").is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("safe.json")).unwrap(),
            "unrelated data"
        );
        std::fs::remove_file(dir.join("safe.json")).unwrap();
        write_working_tree(
            &dir,
            &[
                collection("c1"),
                collection("550e8400-e29b-41d4-a716-446655440000"),
            ],
            "",
            "",
        )
        .unwrap();
        let loaded = crate::services::repo_import::scan_dir(&dir, "security-test").collections;
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().all(|c| c.collection.name == "한국 ไทย 一"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn vcs_security_ignores_unrelated_oversized_and_binary_json() {
        let dir = std::env::temp_dir().join(format!("rustman-vcs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let large = std::fs::File::create(dir.join("large.json")).unwrap();
        large.set_len(MAX_COLLECTION_FILE_BYTES + 1).unwrap();
        drop(large);
        std::fs::write(dir.join("binary.json"), [0xff, 0xfe]).unwrap();
        write_working_tree(&dir, &[collection("c1")], "", "").unwrap();
        assert_eq!(
            crate::services::repo_import::scan_dir(&dir, "security-test")
                .collections
                .len(),
            1
        );
        let identity = GitIdentity {
            name: "Test".into(),
            email: "test@example.test".into(),
        };
        commit(&dir, &identity, "managed collection only").unwrap();
        assert_eq!(
            std::fs::metadata(dir.join("large.json")).unwrap().len(),
            MAX_COLLECTION_FILE_BYTES + 1
        );
        assert_eq!(
            std::fs::read(dir.join("binary.json")).unwrap(),
            [0xff, 0xfe]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn vcs_security_preserves_unrelated_files_and_staging() {
        let dir = std::env::temp_dir().join(format!("rustman-vcs-{}", uuid::Uuid::new_v4()));
        let repo = open_at(&dir).unwrap();
        std::fs::write(
            dir.join("unrelated.json"),
            "{\"token\":\"synthetic-secret\"}",
        )
        .unwrap();
        std::fs::write(dir.join("private.txt"), "synthetic-secret").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("private.txt")).unwrap();
        index.write().unwrap();
        write_working_tree(&dir, &[collection("c1")], "", "").unwrap();
        assert!(dir.join("unrelated.json").exists());
        let identity = GitIdentity {
            name: "Test".into(),
            email: "test@example.test".into(),
        };
        let oid = commit(&dir, &identity, "safe snapshot").unwrap();
        let tree = repo
            .find_commit(git2::Oid::from_str(&oid).unwrap())
            .unwrap()
            .tree()
            .unwrap();
        assert!(tree.get_name("c1.json").is_some());
        assert!(tree.get_name("private.txt").is_none());
        assert!(tree.get_name("unrelated.json").is_none());
        assert!(
            repo.index()
                .unwrap()
                .get_path(Path::new("private.txt"), 0)
                .is_some()
        );
        drop(tree);
        drop(index);
        drop(repo);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
