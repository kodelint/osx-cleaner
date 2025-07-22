use crate::utils::filesystem::remove_path;
use crate::{log_debug, log_info, log_warn};
use colored::Colorize;
use glob::glob;
use rayon::prelude::*;
use std::{
    env,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex},
};

use tabled::{settings::Style, Table, Tabled};
use crate::logger::is_debug_enabled;

/// Represents an entry in the successful cleanup summary table.
#[derive(Tabled, Clone)]
pub struct CleanupEntry {
    #[tabled(rename = "Path")]
    path: String,
    #[tabled(rename = "Size")]
    size: String,
}

/// Represents an entry for paths that failed to be cleaned.
#[derive(Tabled, Clone)]
pub struct FailedEntry {
    #[tabled(rename = "Path")]
    path: String,
    #[tabled(rename = "Error")]
    error: String,
}

/// Represents an entry for paths that were skipped during initial processing (e.g., unreadable, active TMPDIR).
#[derive(Tabled, Clone)]
pub struct SkippedEntry {
    #[tabled(rename = "Path")]
    path: String,
    #[tabled(rename = "Reason")]
    reason: String,
}

/// A temporary struct to hold information about paths identified in the first pass,
/// before actual cleaning attempts.
struct PathToCheck {
    path: PathBuf,
    initial_size: u64,
}

/// Defines a common interface for any entity or category of files that can be cleaned.
/// This trait provides a default `clean` method that leverages the `find_paths`
/// method to identify and process all associated files.
pub trait Cleaner {
    /// Returns a user-friendly name for the category of files being cleaned (e.g., "Caches", "Logs").
    fn name(&self) -> &str;

    /// Discovers and returns a list of file system paths associated with this cleaner category.
    /// These paths represent files and directories that should be considered for cleanup.
    fn find_paths(&self) -> Vec<PathBuf>;

    /// Executes the cleanup process for the paths identified by `find_paths`.
    ///
    /// It performs a two-pass approach:
    /// 1. **First Pass (Checking)**: Identifies existing paths, calculates their sizes,
    ///    and skips active temporary directories or unreadable paths.
    /// 2. **Second Pass (Cleaning/Dry Run)**: Either performs actual deletion or
    ///    reports what *would* be deleted based on the `dry_run` flag.
    ///
    /// The deletion/reporting is performed in parallel using Rayon for efficiency.
    ///
    /// # Arguments
    /// * `dry_run` - A boolean flag. If `true`, the cleaner will only log which files *would* be deleted
    ///               without actually performing any deletions. If `false`, actual deletion occurs.
    /// * `ignore` - A `Vec<String>` of substrings. Any path containing one of these substrings
    ///              will be excluded from the cleanup process.
    /// * `successful_entries`, `failed_entries`, `skipped_during_size_check` - Shared, mutable
    ///   vectors (protected by `Arc<Mutex>`) to collect the outcomes of the cleanup operations
    ///   for final reporting.
    /// * `total_freed` - An `Arc<AtomicU64>` to safely accumulate the total bytes freed across threads.
    /// * `checking_logs`, `would_clean_logs`, `cleaning_logs` - Shared, mutable vectors
    ///   to collect log messages for sequential printing at a higher level.
    /// * `warning_logs` - Shared, mutable vector to collect specific warning messages for stacking.
    /// * `ignored_paths_logs` - Shared, mutable vector to collect paths explicitly ignored by user.
    ///
    /// # Returns
    /// A `Result` indicating success (`Ok(())`) or failure (`Err(Box<dyn std::error::Error>)`).
    /// Errors are typically related to underlying file system operations.
    fn clean(
        &self,
        dry_run: bool,
        ignore: &Vec<String>,
        successful_entries: Arc<Mutex<Vec<CleanupEntry>>>,
        failed_entries: Arc<Mutex<Vec<FailedEntry>>>,
        skipped_during_size_check: Arc<Mutex<Vec<SkippedEntry>>>,
        total_freed: Arc<AtomicU64>,
        checking_logs: Arc<Mutex<Vec<String>>>,
        would_clean_logs: Arc<Mutex<Vec<String>>>,
        cleaning_logs: Arc<Mutex<Vec<String>>>,
        warning_logs: Arc<Mutex<Vec<String>>>,
        ignored_paths_logs: Arc<Mutex<Vec<String>>>, // New argument
    ) -> Result<(), Box<dyn std::error::Error>> {
        log_debug!("Starting cleanup for '{}'", self.name().bright_white());

        let initial_paths = self.find_paths();
        let mut paths_to_process = Vec::new();
        let mut paths_explicitly_ignored = Vec::new();

        // Filter out any paths that the user has specified to ignore.
        for p in initial_paths {
            let canonical_p = p.canonicalize().unwrap_or_else(|_| p.clone());
            let mut should_ignore = false;
            for ign_str in ignore {
                let ign_path = PathBuf::from(ign_str);
                let canonical_ign_path = ign_path.canonicalize().unwrap_or_else(|_| ign_path.clone());

                if canonical_p == canonical_ign_path || canonical_p.starts_with(&canonical_ign_path) {
                    should_ignore = true;
                    break;
                }
            }

            if should_ignore {
                paths_explicitly_ignored.push(p);
            } else {
                paths_to_process.push(p);
            }
        }

        // Log paths that were explicitly ignored by the user
        for ignored_path in paths_explicitly_ignored {
            ignored_paths_logs.lock().unwrap().push(format!(
                "🚫 Ignored by user: {}",
                ignored_path.display().to_string().bright_cyan()
            ));
        }


        // Retrieve the value of the `TMPDIR` environment variable.
        let tmpdir_path = env::var("TMPDIR").ok().map(PathBuf::from);

        let paths_to_check_for_size: Arc<Mutex<Vec<PathToCheck>>> = Arc::new(Mutex::new(Vec::new()));

        // FIRST PASS: PARALLEL CHECKING AND INITIAL SIZING
        paths_to_process.par_iter().for_each(|path| {
            // Critical skip: Check if the current path is either the global `/private/tmp` or
            // the specific active temporary directory defined by `TMPDIR`.
            if path.as_path() == PathBuf::from("/private/tmp").as_path() || tmpdir_path.as_ref().map_or(false, |tmp| path.as_path() == tmp.as_path()) {
                skipped_during_size_check.lock().unwrap().push(SkippedEntry {
                    path: path.display().to_string(),
                    reason: "Active system temporary directory".to_string(),
                });
                return;
            }

            checking_logs.lock().unwrap().push(format!("🔍 Checking: {}", path.display()));

            if !path.exists() {
                let msg = "Not found".to_string();
                failed_entries.lock().unwrap().push(FailedEntry {
                    path: path.display().to_string(),
                    error: msg.clone(),
                });
                warning_logs.lock().unwrap().push(format!("⚠️ {}: {}", path.display(), msg));
                return;
            }

            match get_size(path, &skipped_during_size_check, &warning_logs) {
                Ok(size_before) => {
                    if size_before == 0 {
                        warning_logs.lock().unwrap().push(format!(
                            "⚪ Skipping empty path: {}",
                            path.display().to_string().bright_magenta()
                        ));
                        return;
                    }
                    paths_to_check_for_size.lock().unwrap().push(PathToCheck {
                        path: path.clone(),
                        initial_size: size_before,
                    });
                }
                Err(e) => {
                    let err_str = e.to_string();
                    let reason = format!("Size check failed: {}", err_str);
                    skipped_during_size_check.lock().unwrap().push(SkippedEntry {
                        path: path.display().to_string(),
                        reason,
                    });
                    warning_logs.lock().unwrap().push(format!(
                        "🚫 Size check failed for {}: {}",
                        path.display().to_string().bright_yellow(),
                        err_str.bright_white()
                    ));
                }
            }
        });

        // SECOND PASS: PARALLEL CLEANING/DRY RUN AND LOGGING COLLECTION
        paths_to_check_for_size.lock().unwrap().par_iter().for_each(|item| {
            let path = &item.path;
            let size_before = item.initial_size;
            let size_fmt_before = format_bytes(size_before);

            if dry_run {
                would_clean_logs.lock().unwrap().push(format!(
                    "🧾 Would clean: {} ({})",
                    path.display().to_string().bright_green(),
                    size_fmt_before.bright_white().bold()
                ));
                successful_entries.lock().unwrap().push(CleanupEntry {
                    path: path.display().to_string(),
                    size: size_fmt_before.clone(),
                });
                total_freed.fetch_add(size_before, Ordering::Relaxed);
            } else {
                cleaning_logs.lock().unwrap().push(format!(
                    "🧹 Cleaning: {} ({})",
                    path.display().to_string().bright_green(),
                    size_fmt_before.bright_white().bold()
                ));

                if let Err(e) = remove_path(path) {
                    let err_str = e.to_string();
                    failed_entries.lock().unwrap().push(FailedEntry {
                        path: path.display().to_string(),
                        error: err_str.clone(),
                    });
                    warning_logs.lock().unwrap().push(format!(
                        "❌ Failed to delete {}: {}",
                        path.display().to_string().bright_yellow(),
                        err_str.bright_white()
                    ));
                }

                match get_size(path, &skipped_during_size_check, &warning_logs) {
                    Ok(size_after) => {
                        let freed = size_before.saturating_sub(size_after);
                        if freed > 0 {
                            let freed_fmt = format_bytes(freed);
                            total_freed.fetch_add(freed, Ordering::Relaxed);
                            successful_entries.lock().unwrap().push(CleanupEntry {
                                path: path.display().to_string(),
                                size: freed_fmt,
                            });
                        } else {
                            warning_logs.lock().unwrap().push(format!(
                                "⚠️ No space freed for path: {}", path.display()
                            ));
                        }
                    }
                    Err(e) => {
                        let reason = format!("Failed to get size after cleanup: {}", e);
                        warning_logs.lock().unwrap().push(format!(
                            "🚫 Failed to get size after cleanup for {}: {}",
                            path.display(),
                            e
                        ));
                        skipped_during_size_check.lock().unwrap().push(SkippedEntry {
                            path: path.display().to_string(),
                            reason,
                        });
                    }
                }
            }
        });

        log_debug!("Completed cleanup for '{}'", self.name().to_string().bright_white());
        Ok(())
    }
}

/// Recursively calculates the total size of a file or directory.
fn get_size(
    path: &PathBuf,
    skipped_paths: &Arc<Mutex<Vec<SkippedEntry>>>,
    warning_logs: &Arc<Mutex<Vec<String>>>,
) -> Result<u64, std::io::Error> {
    if !path.exists() {
        return Ok(0);
    }

    let metadata = fs::symlink_metadata(path)?;

    if metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(metadata.len());
    }

    let mut size = 0;
    for entry in fs::read_dir(path)? {
        match entry {
            Ok(e) => {
                let entry_path = e.path();
                match get_size(&entry_path, skipped_paths, warning_logs) {
                    Ok(s) => size += s,
                    Err(err) => {
                        let reason = format!("Unreadable entry: {}", err);
                        skipped_paths.lock().unwrap().push(SkippedEntry {
                            path: entry_path.display().to_string(),
                            reason,
                        });
                        warning_logs.lock().unwrap().push(format!(
                            "⚠️ Unreadable entry {}: {}",
                            entry_path.display(),
                            err
                        ));
                    }
                }
            },
            Err(err) => {
                let reason = format!("Unreadable directory entry: {}", err);
                skipped_paths.lock().unwrap().push(SkippedEntry {
                    path: path.display().to_string(),
                    reason,
                });
                warning_logs.lock().unwrap().push(format!(
                    "⚠️ Unreadable directory entry {}: {}",
                    path.display(),
                    err
                ));
            }
        }
    }
    Ok(size)
}

/// Formats a given number of bytes into a human-readable string (e.e., "10.5 MB").
fn format_bytes(bytes: u64) -> String {
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
fn is_sip_enabled() -> bool {
    Command::new("csrutil")
        .arg("status")
        .output()
        .ok()
        .map_or(false, |output| {
            String::from_utf8_lossy(&output.stdout).contains("enabled")
        })
}

/// Represents a cleaner for system-wide caches.
pub struct SystemCachesCleaner;

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

/// Represents a cleaner for user-specific caches.
pub struct UserCachesCleaner;

impl Cleaner for UserCachesCleaner {
    fn name(&self) -> &str {
        "User Caches"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        let home = env::var("HOME").unwrap_or_default();
        vec![PathBuf::from(format!("{}/Library/Caches", home))]
    }
}

/// Represents a cleaner for temporary directories.
pub struct TemporaryFilesCleaner;

impl Cleaner for TemporaryFilesCleaner {
    fn name(&self) -> &str {
        "Temporary Files"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        vec![
            PathBuf::from("/private/tmp"),
            PathBuf::from("/private/var/folders"),
            PathBuf::from("/var/folders"), // Symlink to /private/var/folders
        ]
    }
}

/// Represents a cleaner for user-specific logs.
pub struct UserLogsCleaner;

impl Cleaner for UserLogsCleaner {
    fn name(&self) -> &str {
        "User Logs"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        let home = env::var("HOME").unwrap_or_default();
        vec![PathBuf::from(format!("{}/Library/Logs", home))]
    }
}

/// Represents a cleaner for crash reporter logs.
pub struct CrashReporterLogsCleaner;

impl Cleaner for CrashReporterLogsCleaner {
    fn name(&self) -> &str {
        "Crash Reporter Logs"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        let home = env::var("HOME").unwrap_or_default();
        vec![PathBuf::from(format!("{}/Library/Application Support/CrashReporter", home))]
    }
}

/// Represents a cleaner for Trash bins (user's and mounted volumes).
pub struct TrashCleaner;

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

/// The main function for cleaning macOS system junk and temporary files.
/// It orchestrates the cleaning process by iterating through different `Cleaner` implementations.
///
/// # Arguments
/// * `dry_run` - A boolean flag. If `true`, no files will be deleted; only a report
///               of what *would* be deleted is shown.
/// * `ignore` - A `Vec<String>` of substrings. Any path containing one of these substrings
///              will be excluded from the cleanup process.
///
/// # Returns
/// A `Result<(), Box<dyn std::error::Error>>` indicating the overall success or failure
/// of the cleanup function. Individual file deletion failures are logged as warnings.
pub fn clean_my_mac(
    dry_run: bool,
    ignore: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    log_debug!("{}", "🧼 Starting macOS cleanup...".bright_purple());

    let successful_entries = Arc::new(Mutex::new(Vec::<CleanupEntry>::new()));
    let failed_entries = Arc::new(Mutex::new(Vec::<FailedEntry>::new()));
    let skipped_during_size_check = Arc::new(Mutex::new(Vec::<SkippedEntry>::new()));
    let total_freed = Arc::new(AtomicU64::new(0));

    // Global log collectors for stacked output
    let checking_logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let would_clean_logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let cleaning_logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let warning_logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let ignored_paths_logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new())); // New log collector

    // Create instances of our specific cleaner types
    let cleaners: Vec<Box<dyn Cleaner>> = vec![
        Box::new(SystemCachesCleaner),
        Box::new(UserCachesCleaner),
        Box::new(TemporaryFilesCleaner),
        Box::new(UserLogsCleaner),
        Box::new(CrashReporterLogsCleaner),
        Box::new(TrashCleaner),
    ];

    // Iterate through each cleaner and execute its `clean` method
    for cleaner in cleaners {
        cleaner.clean(
            dry_run,
            &ignore,
            Arc::clone(&successful_entries),
            Arc::clone(&failed_entries),
            Arc::clone(&skipped_during_size_check),
            Arc::clone(&total_freed),
            Arc::clone(&checking_logs),
            Arc::clone(&would_clean_logs),
            Arc::clone(&cleaning_logs),
            Arc::clone(&warning_logs),
            Arc::clone(&ignored_paths_logs), // Pass the new log collector
        )?;
    }

    if !checking_logs.lock().unwrap().is_empty() {
        eprintln!("\n");
        for log_line in checking_logs.lock().unwrap().iter() {
            log_info!("{}", log_line);
        }
    }

    if !would_clean_logs.lock().unwrap().is_empty() {
        eprintln!("\n");
        for log_line in would_clean_logs.lock().unwrap().iter() {
            log_info!("{}", log_line);
        }
    }

    if !cleaning_logs.lock().unwrap().is_empty() {
        eprintln!("\n");
        for log_line in cleaning_logs.lock().unwrap().iter() {
            log_info!("{}", log_line);
        }
    }

    if env::var("OSX_SHOW_WARNINGS").is_ok() || is_debug_enabled() {
        if !warning_logs.lock().unwrap().is_empty() {
            eprintln!("\n");
            for log_line in warning_logs.lock().unwrap().iter() {
                log_warn!("{}", log_line);
            }
        }
    }

    // Print collected ignored paths logs
    if !ignored_paths_logs.lock().unwrap().is_empty() {
        eprintln!("\n");
        println!("{}", "🚫 Paths Ignored by User".bold().underline().cyan());
        for log_line in ignored_paths_logs.lock().unwrap().iter() {
            log_info!("{}", log_line);
        }
    }


    let total = total_freed.load(Ordering::Relaxed);
    let total_fmt = format_bytes(total);

    let mut all_success = successful_entries.lock().unwrap().clone();
    all_success.push(CleanupEntry {
        path: "Total".to_string(),
        size: total_fmt.clone(),
    });

    eprintln!("\n");
    let table = Table::new(&all_success)
        .with(Style::modern())
        .to_string();

    println!("{}", "🧾 Cleanup Summary (Successful)".bold().underline().green());
    println!("{}", table);

    let all_failures = failed_entries.lock().unwrap();
    if !all_failures.is_empty() {
        eprintln!("\n");
        let table = Table::new(&*all_failures)
            .with(Style::modern())
            .to_string();

        println!("{}", "⚠️ Cleanup Failures".bold().underline().yellow());
        println!("{}", table);
    }

    if env::var("OSX_SHOW_SKIPPED").is_ok() || is_debug_enabled() {
        let all_skipped = skipped_during_size_check.lock().unwrap();
        if !all_skipped.is_empty() {
            eprintln!("\n");
            let table = Table::new(&*all_skipped)
                .with(Style::modern())
                .to_string();

            println!("{}", "⚪ Skipped Paths (During Size Check)".bold().underline().magenta());
            println!("{}", table);
        }
    }

    eprintln!("\n");
    if dry_run {
        log_info!("🧠 Estimated space to free: {}", total_fmt.bright_green().bold());
    } else {
        log_info!("✔ Total space freed: {}", total_fmt.bright_green().bold());
    }

    if is_sip_enabled() {
        log_warn!(
            "{}",
            "⚠️  System Integrity Protection (SIP) is enabled. Some files may not be removable."
                .bright_yellow()
        );
    }

    log_debug!("✔ Finished clean_my_mac.");
    Ok(())
}