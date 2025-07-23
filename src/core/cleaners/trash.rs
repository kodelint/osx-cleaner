use crate::core::cleaners::Cleaner;
use glob::glob;
use std::env;
use std::path::PathBuf;

/// Represents a cleaner for Trash bins (user's and mounted volumes).
pub struct TrashCleaner;

impl TrashCleaner {
    pub fn new() -> Self {
        TrashCleaner // This simply returns an instance of the struct
    }
}

impl Cleaner for TrashCleaner {
    fn name(&self) -> &str {
        "Trash Bins"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(home) = env::var("HOME") {
            paths.push(PathBuf::from(format!("{}/.Trash", home)));
        }
        if let Ok(volumes) = glob("/Volumes/*/.Trashes") {
            for entry in volumes.flatten() {
                paths.push(entry);
            }
        }
        paths
    }
}