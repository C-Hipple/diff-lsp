use std::collections::HashSet;
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
