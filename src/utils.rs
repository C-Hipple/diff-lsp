use std::collections::HashSet;

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
