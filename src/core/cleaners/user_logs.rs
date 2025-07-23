use crate::core::cleaners::Cleaner;
use std::env;
use std::path::PathBuf;

/// Represents a cleaner for user-specific logs.
pub struct UserLogsCleaner;

impl UserLogsCleaner {
    pub fn new() -> Self {
        UserLogsCleaner // This simply returns an instance of the struct
    }
}

impl Cleaner for UserLogsCleaner {
    fn name(&self) -> &str {
        "User Logs"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        let home = env::var("HOME").unwrap_or_default();
        vec![PathBuf::from(format!("{}/Library/Logs", home))]
    }
}
