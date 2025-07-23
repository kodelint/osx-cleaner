use crate::core::cleaners::Cleaner;
use std::env;
use std::path::PathBuf;

/// Represents a cleaner for user-specific caches.
pub struct UserCachesCleaner;

impl UserCachesCleaner {
    pub fn new() -> Self {
        UserCachesCleaner // This simply returns an instance of the struct
    }
}

impl Cleaner for UserCachesCleaner {
    fn name(&self) -> &str {
        "User Caches"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        let home = env::var("HOME").unwrap_or_default();
        vec![PathBuf::from(format!("{}/Library/Caches", home))]
    }
}