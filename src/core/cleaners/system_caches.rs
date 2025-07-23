use std::path::PathBuf;
use super::Cleaner; // Import the Cleaner trait from the parent module

pub struct SystemCachesCleaner;

impl SystemCachesCleaner {
    pub fn new() -> Self {
        SystemCachesCleaner // This simply returns an instance of the struct
    }
}

impl Cleaner for SystemCachesCleaner {
    fn name(&self) -> &str {
        "System Caches"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        vec![
            PathBuf::from("/Library/Caches"),
            PathBuf::from("/System/Library/Caches"),
        ]
    }
}