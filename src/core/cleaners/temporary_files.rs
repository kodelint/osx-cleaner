use crate::core::cleaners::Cleaner;
use colored::Colorize;
use std::{
    fs,
    path::PathBuf,
    env,
};
use crate::{log_debug, log_warn}; // Import logging macros

/// Represents a cleaner for temporary directories.
pub struct TemporaryFilesCleaner;

impl TemporaryFilesCleaner {
    pub fn new() -> Self {
        TemporaryFilesCleaner
    }
}

impl Cleaner for TemporaryFilesCleaner {
    fn name(&self) -> &str {
        "Temporary Files"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // Define common temporary directories to scan.
        // On macOS, /tmp is usually a symlink to /private/tmp.
        // We'll iterate the contents of these directories.
        let temp_dirs_to_scan = vec![
            PathBuf::from("/tmp"),
            PathBuf::from("/private/tmp"),
            PathBuf::from("/var/tmp"),
        ];

        // Get the current user's active temporary directory, which should *not* be deleted.
        // This is usually where applications store their active temporary files.
        let current_tmpdir = env::var("TMPDIR")
            .map(|s| PathBuf::from(s))
            .ok();

        for dir in temp_dirs_to_scan {
            let canonical_dir = dir.canonicalize().unwrap_or_else(|_| {
                log_warn!("Failed to canonicalize path: {}", dir.display());
                dir.clone() // Use the original path if canonicalization fails
            });

            if let Ok(entries) = fs::read_dir(&canonical_dir) {
                for entry_result in entries {
                    if let Ok(entry) = entry_result {
                        let path = entry.path();

                        // Crucial safety checks:
                        // 1. Never add the root temporary directory itself to the list.
                        if path == canonical_dir {
                            log_debug!("Skipping root temp dir: {}", path.display());
                            continue;
                        }

                        // 2. Never add the active TMPDIR or its canonicalized version.
                        if let Some(ref active_tmp) = current_tmpdir {
                            if path == *active_tmp || path.canonicalize().map_or(false, |p| p == *active_tmp) {
                                log_debug!("Skipping active TMPDIR entry: {}", path.display());
                                continue;
                            }
                        }

                        paths.push(path);
                    }
                }
            } else {
                log_warn!("Could not read temporary directory: {}", canonical_dir.display());
            }
        }
        paths
    }
}
