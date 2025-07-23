// src/core/cleaners/mod.rs

use crate::{log_debug, log_warn};
use colored::Colorize;
use rayon::prelude::*;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};
use tabled::Tabled;

/// Represents an entry in the successful cleanup summary table.
#[derive(Tabled, Clone)]
pub struct CleanupEntry {
    #[tabled(rename = "Path")]
    pub path: String,
    #[tabled(rename = "Size")]
    pub size: String,
    // Add this line to hide it from default table output
    #[tabled(skip)]
    pub cleaner_name: String,
}

/// Represents an entry for paths that failed to be cleaned.
#[derive(Tabled, Clone)]
pub struct FailedEntry {
    #[tabled(rename = "Path")]
    pub path: String,
    #[tabled(rename = "Error")]
    pub error: String,
}

/// Represents an entry for paths that were skipped during initial processing (e.g., unreadable, active TMPDIR).
#[derive(Tabled, Clone)]
pub struct SkippedEntry {
    #[tabled(rename = "Path")]
    pub path: String,
    #[tabled(rename = "Reason")]
    pub reason: String,
}

/// A temporary struct to hold information about paths identified in the first pass,
/// before actual cleaning attempts.
pub struct PathToCheck {
    pub path: PathBuf,
    pub initial_size: u64,
    pub formatted_size: String,
    pub cleaner_name: String,
}

/// Defines a common interface for any entity or component that can perform a specific cleaning task.
pub trait Cleaner: Send + Sync {
    /// Returns the user-friendly name of the cleaner (e.g., "System Caches").
    fn name(&self) -> &str;

    /// Discovers and returns a list of file system paths that this cleaner targets.
    fn find_paths(&self) -> Vec<PathBuf>;

    /// Executes the cleaning logic for this specific cleaner.
    /// This method will now perform path finding, size checking, and log the "Checking" phase.
    /// It returns a vector of `PathToCheck` that the orchestrator will then process for cleaning.
    fn clean(
        &self,
        checking_logs: &Arc<Mutex<Vec<String>>>,
        skipped_entries: &Arc<Mutex<Vec<SkippedEntry>>>,
        ignore: &[String],
    ) -> Result<Vec<PathToCheck>, Box<dyn std::error::Error>> {
        log_debug!("🚀 Starting {} cleanup...", self.name());

        let mut paths = self.find_paths();

        // Apply the ignore filter to the paths found by this cleaner
        let initial_count = paths.len();
        paths.retain(|p| {
            let path_str = p.to_string_lossy();
            // MODIFIED: Trim whitespace from each ignore pattern before using it in `contains`
            !ignore.iter().any(|i| path_str.contains(i.trim()))
        });
        if paths.len() < initial_count {
            log_debug!("Filtered {} paths from {} due to ignore list.", initial_count - paths.len(), self.name());
        }


        let paths_to_process: Arc<Mutex<Vec<PathToCheck>>> = Arc::new(Mutex::new(Vec::new()));

        paths.par_iter().for_each(|path| {
            checking_logs.lock().unwrap().push(format!("🔍 Checking: {}", path.display().to_string().blue()));

            match calculate_size(path) {
                Some((size, formatted_size)) => {
                    paths_to_process.lock().unwrap().push(PathToCheck {
                        path: path.clone(),
                        initial_size: size,
                        formatted_size,
                        cleaner_name: self.name().to_string(),
                    });
                }
                None => {
                    log_warn!("⚠️ Could not determine size for path: {}", path.display());
                    skipped_entries.lock().unwrap().push(SkippedEntry {
                        path: path.display().to_string(),
                        reason: "Could not determine size or access path.".to_string(),
                    });
                }
            }
        });

        log_debug!("✅ Finished {} cleanup.", self.name());

        Ok(paths_to_process.lock().unwrap().drain(..).collect())
    }
}

// Helper to calculate the size of a path (file or directory)
pub fn calculate_size(path: &PathBuf) -> Option<(u64, String)> {
    let mut size = 0;
    if path.is_file() {
        if let Ok(metadata) = fs::metadata(path) {
            size = metadata.len();
        } else {
            log_warn!(
                "⚠️ Failed to get metadata for file: {}",
                path.display()
            );
            return None;
        }
    } else if path.is_dir() {
        for entry in fs::read_dir(path).ok()?.flatten() {
            let entry_path = entry.path();
            if entry_path.is_file() {
                if let Ok(metadata) = fs::metadata(&entry_path) {
                    size += metadata.len();
                } else {
                    log_warn!(
                        "⚠️ Failed to get metadata for file in dir: {}",
                        entry_path.display()
                    );
                }
            } else if entry_path.is_dir() {
                if let Some((subdir_size, _)) = calculate_size(&entry_path) {
                    size += subdir_size;
                }
            }
        }
    } else {
        log_warn!("⚠️ Path is neither file nor directory: {}", path.display());
        return None;
    }
    Some((size, format_bytes(size)))
}

// Formats a given number of bytes into a human-readable string.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    match bytes {
        b if b >= GB => format!("{:.2} GB", b as f64 / GB as f64),
        b if b >= MB => format!("{:.2} MB", b as f64 / MB as f64),
        b if b >= KB => format!("{:.2} KB", b as f64 / KB as f64),
        b => format!("{} bytes", b),
    }
}

/// Checks if System Integrity Protection (SIP) is enabled on macOS.
pub fn is_sip_enabled() -> bool {
    Command::new("csrutil")
        .arg("status")
        .output()
        .ok()
        .map_or(false, |output| {
            String::from_utf8_lossy(&output.stdout).contains("enabled")
        })
}

// Re-export individual cleaner implementations
pub mod system_caches;
pub use self::system_caches::SystemCachesCleaner;
pub mod user_caches;
pub use self::user_caches::UserCachesCleaner;
pub mod temporary_files;
pub use self::temporary_files::TemporaryFilesCleaner;
pub mod user_logs;
pub use self::user_logs::UserLogsCleaner;
pub mod crash_reporter_logs;
pub use self::crash_reporter_logs::CrashReporterLogsCleaner;
pub mod trash;
pub use self::trash::TrashCleaner;
pub mod large_files;
pub use self::large_files::LargeFilesCleaner;
mod browser_caches;
pub use self::browser_caches::BrowserCachesCleaner;
