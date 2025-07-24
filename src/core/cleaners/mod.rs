use crate::{log_debug, log_warn};
use colored::Colorize;
use rayon::prelude::*; // Used for parallel iteration over collections.
use std::{
    fs, // File system operations (e.g., metadata, read_dir).
    path::PathBuf, // Represents file system paths.
    process::Command, // For executing external commands (e.g., csrutil).
    sync::{Arc, Mutex}, // For shared, thread-safe access to data.
};
use tabled::Tabled; // Trait for generating formatted tables.

/// Represents an entry in the successful cleanup summary table.
/// This struct is derived with `Tabled` to automatically generate table rows.
#[derive(Tabled, Clone)]
pub struct CleanupEntry {
    // `#[tabled(rename = "Type")]` renames the column header in the output table.
    // This field stores the name of the cleaner that performed the cleanup.
    #[tabled(rename = "Type")]
    pub cleaner_name: String,
    // The file system path that was cleaned.
    #[tabled(rename = "Path")]
    pub path: String,
    // The size of the cleaned path, formatted as a human-readable string (e.g., "1.2 GB").
    #[tabled(rename = "Size")]
    pub size: String,
}

/// Represents an entry for paths that failed to be cleaned.
/// This struct is also `Tabled` for displaying failure reports.
#[derive(Tabled, Clone)]
pub struct FailedEntry {
    // The path that could not be cleaned.
    #[tabled(rename = "Path")]
    pub path: String,
    // The error message explaining why the cleaning failed.
    #[tabled(rename = "Error")]
    pub error: String,
}

/// Represents an entry for paths that were skipped during initial processing.
/// This could be due to unreadable permissions, active directories, or other reasons.
#[derive(Tabled, Clone)]
pub struct SkippedEntry {
    // The path that was skipped.
    #[tabled(rename = "Path")]
    pub path: String,
    // The reason why the path was skipped.
    #[tabled(rename = "Reason")]
    pub reason: String,
}

/// A temporary struct to hold information about paths identified in the first pass,
/// before actual cleaning attempts. This struct is used internally by the orchestrator.
pub struct PathToCheck {
    pub path: PathBuf, // The actual file system path.
    pub initial_size: u64, // The size of the path in bytes.
    pub formatted_size: String, // The human-readable formatted size.
    pub cleaner_name: String, // The name of the cleaner that identified this path.
}

/// Defines a common interface for any entity or component that can perform a specific cleaning task.
/// The `Send` and `Sync` traits are required for objects to be safely sent between threads
/// and accessed concurrently, which is necessary for `rayon`'s parallel processing.
pub trait Cleaner: Send + Sync {
    /// Returns the user-friendly name of the cleaner (e.g., "System Caches").
    fn name(&self) -> &str;

    /// Discovers and returns a list of file system paths that this cleaner targets.
    /// Each concrete `Cleaner` implementation must provide its own logic for this method.
    fn find_paths(&self) -> Vec<PathBuf>;

    /// Executes the cleaning logic for this specific cleaner.
    /// This method now primarily focuses on identifying paths, calculating their sizes,
    /// applying ignore filters, and logging the "Checking" phase.
    /// It returns a vector of `PathToCheck` which the main orchestrator will then use
    /// to perform the actual file deletion.
    ///
    /// # Arguments
    /// * `checking_logs` - An `Arc<Mutex<Vec<String>>>` to store logs related to path checking.
    ///   This allows multiple parallel cleaners to append their checking messages safely.
    /// * `skipped_entries` - An `Arc<Mutex<Vec<SkippedEntry>>>` to record paths that were
    ///   skipped during the size check (e.g., due to permission issues).
    /// * `ignore` - A slice of `String`s representing patterns to ignore.
    ///
    /// # Returns
    /// A `Result` containing `Vec<PathToCheck>` on success, or a `Box<dyn std::error::Error>` on failure.
    fn clean(
        &self,
        checking_logs: &Arc<Mutex<Vec<String>>>,
        skipped_entries: &Arc<Mutex<Vec<SkippedEntry>>>,
        ignore: &[String],
    ) -> Result<Vec<PathToCheck>, Box<dyn std::error::Error>> {
        log_debug!("🚀 Starting {} cleanup...", self.name());

        // Call the cleaner-specific `find_paths` method to get initial candidates.
        let mut paths = self.find_paths();

        // Apply the ignore filter to the paths found by this cleaner.
        let initial_count = paths.len();
        paths.retain(|p| {
            let path_str = p.to_string_lossy();
            // Iterate through ignore patterns and check if any pattern is contained in the path string.
            // `trim()` is used to remove leading/trailing whitespace from ignore patterns.
            !ignore.iter().any(|i| path_str.contains(i.trim()))
        });
        if paths.len() < initial_count {
            log_debug!("Filtered {} paths from {} due to ignore list.", initial_count - paths.len(), self.name());
        }


        // `paths_to_process` will collect `PathToCheck` structs, indicating paths
        // that passed initial checks and are ready for potential cleaning.
        let paths_to_process: Arc<Mutex<Vec<PathToCheck>>> = Arc::new(Mutex::new(Vec::new()));

        // Process the found paths in parallel.
        paths.par_iter().for_each(|path| {
            // Log the "Checking" message for each path. This message is added to a shared, locked vector.
            checking_logs.lock().unwrap().push(format!("🔍 Checking: {}", path.display().to_string().blue()));

            // Attempt to calculate the size of the path.
            match calculate_size(path) {
                Some((size, formatted_size)) => {
                    // If size is successfully determined, add the path to the `paths_to_process` list.
                    paths_to_process.lock().unwrap().push(PathToCheck {
                        path: path.clone(),
                        initial_size: size,
                        formatted_size,
                        cleaner_name: self.name().to_string(),
                    });
                }
                None => {
                    // If size calculation fails, log a warning and add the path to skipped entries.
                    log_warn!("⚠️ Could not determine size for path: {}", path.display());
                    skipped_entries.lock().unwrap().push(SkippedEntry {
                        path: path.display().to_string(),
                        reason: "Could not determine size or access path.".to_string(),
                    });
                }
            }
        });

        log_debug!("✅ Finished {} cleanup.", self.name());

        // Return the collected `PathToCheck` entries. `drain(..).collect()` efficiently moves
        // the contents out of the `Vec` within the `Mutex`.
        Ok(paths_to_process.lock().unwrap().drain(..).collect())
    }
}

// Helper function to calculate the size of a path (file or directory).
// It recursively calculates directory sizes.
pub fn calculate_size(path: &PathBuf) -> Option<(u64, String)> {
    let mut size = 0;
    if path.is_file() {
        // If it's a file, get its metadata and length.
        if let Ok(metadata) = fs::metadata(path) {
            size = metadata.len();
        } else {
            log_warn!(
                "⚠️ Failed to get metadata for file: {}",
                path.display()
            );
            return None; // Return None if metadata cannot be retrieved.
        }
    } else if path.is_dir() {
        // If it's a directory, read its contents.
        // `ok()?.flatten()` handles potential errors during `read_dir` and flattens `Result<Option<...>>`
        for entry in fs::read_dir(path).ok()?.flatten() {
            let entry_path = entry.path();
            if entry_path.is_file() {
                // If it's a file inside the directory, add its size.
                if let Ok(metadata) = fs::metadata(&entry_path) {
                    size += metadata.len();
                } else {
                    log_warn!(
                        "⚠️ Failed to get metadata for file in dir: {}",
                        entry_path.display()
                    );
                }
            } else if entry_path.is_dir() {
                // If it's a subdirectory, recursively call `calculate_size`.
                if let Some((subdir_size, _)) = calculate_size(&entry_path) {
                    size += subdir_size;
                }
            }
        }
    } else {
        // Path is neither a file nor a directory (e.g., a broken symlink).
        log_warn!("⚠️ Path is neither file nor directory: {}", path.display());
        return None;
    }
    // Return the total size and its human-readable formatted string.
    Some((size, format_bytes(size)))
}

// Formats a given number of bytes into a human-readable string (e.g., KB, MB, GB).
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    match bytes {
        b if b >= GB => format!("{:.2} GB", b as f64 / GB as f64),
        b if b >= MB => format!("{:.2} MB", b as f64 / MB as f64),
        b if b >= KB => format!("{:.2} KB", b as f64 / KB as f64),
        b => format!("{} bytes", b), // For sizes less than 1 KB.
    }
}

/// Checks if System Integrity Protection (SIP) is enabled on macOS.
/// This is done by executing the `csrutil status` command and checking its output.
pub fn is_sip_enabled() -> bool {
    Command::new("csrutil") // Creates a new command to run `csrutil`.
        .arg("status") // Adds the "status" argument.
        .output() // Executes the command and captures its output.
        .ok() // Converts `Result` to `Option`, `None` if command fails.
        .map_or(false, |output| { // If `output` is `Some`, process it; otherwise, return `false`.
            // Convert stdout bytes to a lossy UTF-8 string and check if it contains "enabled".
            String::from_utf8_lossy(&output.stdout).contains("enabled")
        })
}

// Re-export individual cleaner implementations.
// This makes the specific cleaner structs (e.g., `SystemCachesCleaner`) directly accessible
// from `crate::core::cleaners` without needing to specify their sub-modules.
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
mod browser_caches; // This module is declared but not `pub` re-exported directly.
pub use self::browser_caches::BrowserCachesCleaner; // Only the struct is re-exported.