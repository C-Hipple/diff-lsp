use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::SystemTime;
use tokio::process::Command;

use log::{info, warn};

/// Prefix of the initialization tempfiles written by editor integrations.
pub const INIT_FILE_PREFIX: &str = "diff_lsp_";
/// Environment variable that can point directly at an initialization file.
pub const INIT_FILE_ENV_VAR: &str = "DIFF_LSP_INIT_FILE";

pub fn get_unique_elements<T: Eq + std::hash::Hash + Copy>(vec: &Vec<T>) -> Vec<T> {
    let mut set = HashSet::new();
    let mut unique_vec = Vec::new();
    for &element in vec {
        if set.insert(element) {
            unique_vec.push(element);
        }
    }
    unique_vec
}

/// Spawns `git fetch origin` in the background.
///
/// All stdio is nulled: diff-lsp's own stdin/stdout are the LSP channel, so the
/// child must never inherit them.  Returns `None` (and logs) if git could not be
/// spawned, e.g. because it is not installed or the directory doesn't exist.
pub fn fetch_origin_nonblocking(repo_path: &str) -> Option<tokio::process::Child> {
    info!("Running git fetch for repo {:?}", repo_path);
    match Command::new("git")
        .current_dir(repo_path)
        .arg("fetch")
        .arg("origin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => Some(child),
        Err(e) => {
            warn!("Failed to spawn git fetch in {:?}: {}", repo_path, e);
            None
        }
    }
}

/// Where the initialization file should come from.
#[derive(Debug, PartialEq, Eq)]
pub enum InitFileSource {
    /// Explicitly given on the command line: `--init <path>`, `--init=<path>`,
    /// or a bare positional path.
    Arg(PathBuf),
    /// Explicitly given via the `DIFF_LSP_INIT_FILE` environment variable.
    Env(PathBuf),
    /// Not given; scan the temp directories for the newest `diff_lsp_*` file.
    Scan,
}

/// Decides where to read the init file from.  `args` are the process args
/// *excluding* the program name.  `--init` wins over a bare positional path
/// (the older form, which existing clients may already pass), and either wins
/// over the env var.  Other flags are ignored (editors sometimes pass e.g.
/// `--stdio`).
pub fn init_file_source(args: &[String], env_value: Option<String>) -> InitFileSource {
    let mut positional: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--init" {
            if let Some(path) = iter.next() {
                return InitFileSource::Arg(PathBuf::from(path));
            }
        } else if let Some(path) = arg.strip_prefix("--init=") {
            return InitFileSource::Arg(PathBuf::from(path));
        } else if positional.is_none() && !arg.starts_with('-') && !arg.trim().is_empty() {
            positional = Some(PathBuf::from(arg));
        }
    }
    if let Some(path) = positional {
        return InitFileSource::Arg(path);
    }
    match env_value {
        Some(v) if !v.trim().is_empty() => InitFileSource::Env(PathBuf::from(v)),
        _ => InitFileSource::Scan,
    }
}

/// Directories scanned for `diff_lsp_*` files: the platform temp dir (honors
/// `$TMPDIR`, which is what Emacs uses on macOS) and `/tmp`, deduplicated.
pub fn init_file_scan_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![std::env::temp_dir()];
    let tmp = PathBuf::from("/tmp");
    if !dirs.iter().any(|d| same_dir(d, &tmp)) {
        dirs.push(tmp);
    }
    dirs
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Like [`get_most_recent_file`], but picks the newest matching file across
/// several directories.  Directories that can't be read are skipped.
pub fn get_most_recent_file_in_dirs(dirs: &[PathBuf], prefix: &str) -> Option<PathBuf> {
    dirs.iter()
        .filter_map(
            |dir| match get_most_recent_file(&dir.to_string_lossy(), prefix) {
                Ok(found) => found,
                Err(e) => {
                    info!("Skipping init file scan of {:?}: {}", dir, e);
                    None
                }
            },
        )
        .filter_map(|path| {
            let modified = fs::metadata(&path).and_then(|m| m.modified()).ok()?;
            Some((path, modified))
        })
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

pub fn get_most_recent_file(dir_path: &str, prefix: &str) -> io::Result<Option<PathBuf>> {
    let dir = Path::new(dir_path);
    let mut most_recent: Option<(PathBuf, SystemTime)> = None;

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
            if file_name.starts_with(prefix) {
                // The file may vanish between read_dir and metadata; skip it.
                let Ok(metadata) = fs::metadata(&path) else {
                    continue;
                };
                if metadata.is_file() {
                    let modified = metadata.modified()?;
                    if most_recent.is_none() || modified > most_recent.as_ref().unwrap().1 {
                        most_recent = Some((path, modified));
                    }
                }
            }
        }
    }

    Ok(most_recent.map(|(path, _)| path))
}
