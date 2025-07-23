use super::Cleaner;
use std::{env, fs, path::PathBuf};
use walkdir::WalkDir;


/// Represents a cleaner for large files in common user directories.
pub struct LargeFilesCleaner;

impl LargeFilesCleaner {
    pub fn new() -> Self {
        LargeFilesCleaner // This simply returns an instance of the struct
    }
}

impl Cleaner for LargeFilesCleaner {
    fn name(&self) -> &str {
        "Large Files"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        let mut large_files = Vec::new();
        let home = env::var("HOME").unwrap_or_default();
        let common_user_dirs = vec![
            "Downloads", "Desktop", "Documents", "Movies", "Music", "Pictures",
        ];
        // Define what constitutes a "large file" (e.g., 100 MB)
        const LARGE_FILE_THRESHOLD_BYTES: u64 = 100 * 1024 * 1024; // 100 MB

        for dir_name in common_user_dirs {
            let current_dir = PathBuf::from(&home).join(dir_name);
            if !current_dir.exists() {
                continue;
            }

            // Recursively walk the directory, filtering out errors
            let walkdir = WalkDir::new(&current_dir)
                .into_iter()
                .filter_map(|e| e.ok());

            for entry in walkdir {
                let path = entry.path().to_path_buf();
                if path.is_file() { // Only check individual files for being large
                    if let Ok(metadata) = fs::metadata(&path) {
                        if metadata.len() >= LARGE_FILE_THRESHOLD_BYTES {
                            large_files.push(path);
                        }
                    }
                }
            }
        }
        large_files
    }
}