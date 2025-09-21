use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::process::Command;

use log::info;

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

pub fn fetch_origin_nonblocking(repo_path: &str) -> tokio::process::Child {
    info!("Running git fetch for repo {:?}", repo_path);
    Command::new("git")
        .current_dir(repo_path)
        .arg("fetch")
        .arg("origin")
        .spawn()
        .expect("Failed to spawn git fetch command")
}

pub fn get_most_recent_file(dir_path: &str, prefix: &str) -> io::Result<Option<PathBuf>> {
    let dir = Path::new(dir_path);
    let mut most_recent: Option<(PathBuf, SystemTime)> = None;

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
            if file_name.starts_with(prefix) {
                let metadata = fs::metadata(&path)?;
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
