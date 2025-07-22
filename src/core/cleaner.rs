use crate::utils::filesystem::remove_path; // Imports the `remove_path` function, responsible for deleting files/directories.
use crate::{log_debug, log_info, log_warn}; // Imports custom logging macros.
use colored::Colorize; // For colored terminal output.
use glob::glob; // For pattern matching file paths (e.g., /Volumes/*/.Trashes).
use rayon::prelude::*; // For parallel iteration and processing.
use std::{
    env, // For accessing environment variables.
    fs, // For file system operations.
    path::PathBuf, // For path manipulation.
    process::Command, // For executing external commands (e.g., csrutil).
    sync::atomic::{AtomicU64, Ordering}, // For thread-safe accumulation of total freed space.
    sync::{Arc, Mutex}, // For shared, mutable data structures across threads.
};

use tabled::{settings::Style, Table, Tabled};
use crate::logger::is_debug_enabled;
// For formatting output into tables.

/// Represents an entry in the successful cleanup summary table.
#[derive(Tabled, Clone)]
struct CleanupEntry {
    #[tabled(rename = "Path")]
    path: String,
    #[tabled(rename = "Size")]
    size: String,
}

/// Represents an entry for paths that failed to be cleaned.
#[derive(Tabled, Clone)]
struct FailedEntry {
    #[tabled(rename = "Path")]
    path: String,
    #[tabled(rename = "Error")]
    error: String,
}

/// Represents an entry for paths that were skipped during initial processing (e.g., unreadable, active TMPDIR).
#[derive(Tabled, Clone)]
struct SkippedEntry {
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

/// Recursively calculates the total size of a file or directory.
///
/// This function is designed to be called from parallel contexts. It handles files,
/// symbolic links, and directories. For directories, it traverses their contents.
/// It also collects paths that are unreadable due to permission issues or other I/O errors
/// into a shared `skipped_paths` vector.
///
/// # Arguments
/// * `path` - A reference to `PathBuf` representing the file or directory to measure.
/// * `skipped_paths` - An `Arc<Mutex<Vec<SkippedEntry>>>` to safely collect information
///                     about paths that could not be fully read during size calculation.
///
/// # Returns
/// A `Result<u64, std::io::Error>`:
/// * `Ok(size)`: The total size in bytes if successful.
/// * `Err(error)`: An `std::io::Error` if the initial metadata lookup fails (e.g., path doesn't exist or permissions prevent even reading metadata).
fn get_size(path: &PathBuf, skipped_paths: &Arc<Mutex<Vec<SkippedEntry>>>) -> Result<u64, std::io::Error> {
    // If the path does not exist, its size is 0, and no error is returned at this stage.
    if !path.exists() {
        return Ok(0);
    }

    // Get metadata for the path. `symlink_metadata` is used to get information about the
    // symlink itself, rather than the target it points to, if `path` is a symlink.
    let metadata = fs::symlink_metadata(path)?;

    // If the path points to a regular file or a symbolic link, return its logical size directly.
    if metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(metadata.len());
    }

    // If it's a directory, initialize size to 0 and recursively sum the sizes of its contents.
    let mut size = 0;
    // Attempt to read the contents of the directory.
    for entry in fs::read_dir(path)? {
        match entry {
            Ok(e) => {
                let entry_path = e.path(); // Get the full path of the current entry (file/subdir).
                // Recursively call `get_size` for the current entry.
                match get_size(&entry_path, skipped_paths) {
                    Ok(s) => size += s, // If successful, add its size to the total for the parent directory.
                    Err(err) => {
                        // If an error occurs while getting the size of a sub-entry (e.g., permission denied for a file inside the directory),
                        // log a debug warning and add this specific sub-entry to the `skipped_paths` list.
                        let reason = format!("Unreadable entry: {}", err);
                        skipped_paths.lock().unwrap().push(SkippedEntry {
                            path: entry_path.display().to_string(),
                            reason,
                        });
                    }
                }
            },
            Err(err) => {
                // If an error occurs while reading a directory entry *itself* (e.g., a corrupted entry or permissions for the entry),
                // log a debug warning and add the parent directory to the `skipped_paths` list.
                let reason = format!("Unreadable directory entry: {}", err);
                // log_debug!("⚠️ Skipping unreadable directory entry in {} ({})", path.display(), err);
                skipped_paths.lock().unwrap().push(SkippedEntry {
                    path: path.display().to_string(),
                    reason,
                });
            }
        }
    }
    Ok(size) // Return the accumulated size of the directory.
}

/// Formats a given number of bytes into a human-readable string (e.g., "10.5 MB").
/// It uses KB, MB, and GB units for better readability.
///
/// # Arguments
/// * `bytes` - The number of bytes (u64) to format.
///
/// # Returns
/// A `String` representing the formatted size.
fn format_bytes(bytes: u64) -> String {
    // Define constants for byte conversions.
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    // Use a match expression to determine the appropriate unit and format the size.
    match bytes {
        b if b >= GB => format!("{:.2} GB", b as f64 / GB as f64), // If bytes are 1GB or more.
        b if b >= MB => format!("{:.2} MB", b as f64 / MB as f64), // If bytes are 1MB or more.
        b if b >= KB => format!("{:.2} KB", b as f64 / KB as f64), // If bytes are 1KB or more.
        b => format!("{} bytes", b), // For less than 1KB, display in bytes.
    }
}

/// Checks if System Integrity Protection (SIP) is enabled on macOS.
/// SIP is a security feature that can prevent the modification or deletion of certain
/// system files or directories, even with root privileges. This check provides
/// a warning to the user if SIP might hinder the cleanup process.
///
/// # Returns
/// `true` if SIP is enabled, `false` otherwise (e.g., if the `csrutil` command fails or SIP is disabled).
fn is_sip_enabled() -> bool {
    // Execute the `csrutil status` command to query SIP status.
    Command::new("csrutil")
        .arg("status")
        .output() // Capture the stdout and stderr.
        .ok() // Convert `Result` to `Option`, discarding any execution errors.
        .map_or(false, |output| {
            // If command output is available, convert stdout bytes to a lossy string
            // and check if it contains the substring "enabled".
            String::from_utf8_lossy(&output.stdout).contains("enabled")
        })
}

/// The main function for cleaning macOS system junk and temporary files.
/// It identifies common directories for caches, logs, temporary files, and trash,
/// calculates their sizes, and then optionally deletes them.
///
/// This version performs a two-pass approach to separate "Checking" logs from "Cleaning" logs.
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
    log_debug!("{}", "🧼 Starting macOS cleanup...".bright_purple()); // Log the initiation of the cleanup process.

    // Define initial common system-wide cleanup paths.
    let mut paths: Vec<String> = vec![
        "/Library/Caches",           // System-wide application caches.
        "/System/Library/Caches",    // Core system caches (often protected by SIP).
        "/private/tmp",              // Global temporary directory, usually cleared on reboot.
        "/private/var/folders",      // Contains user-specific temporary and cache files, and app sandboxes.
        "/var/folders",              // A symlink that points to `/private/var/folders`.
    ]
        .into_iter() // Convert the `&str` slice to an iterator.
        .map(String::from) // Convert each string literal (`&str`) to an owned `String`.
        .collect(); // Collect the results into a `Vec<String>`.

    // Dynamically add user-specific cleanup paths by retrieving the current user's home directory.
    if let Ok(home) = env::var("HOME") {
        paths.extend(vec![
            format!("{}/Library/Caches", home),                      // User's personal application caches.
            format!("{}/Library/Logs", home),                        // User's application log files.
            format!("{}/Library/Application Support/CrashReporter", home), // User's crash reports.
            format!("{}/.Trash", home),                              // User's personal Trash bin.
        ]);
    }

    // Discover and add trash directories located on mounted volumes (e.g., external hard drives, USBs).
    // `glob` is used for pattern matching.
    if let Ok(volumes) = glob("/Volumes/*/.Trashes") {
        for entry in volumes.flatten() { // Iterate over successful path matches (flatten discards errors).
            paths.push(entry.display().to_string()); // Add the resolved path to our list.
        }
    }

    // Filter out any paths that the user has specified to ignore.
    // `retain` keeps elements for which the closure returns `true`.
    paths.retain(|p| !ignore.iter().any(|ign| p.contains(ign)));

    // Retrieve the value of the `TMPDIR` environment variable. This variable points to
    // the temporary directory currently in use by the running process/user session.
    // We want to avoid deleting this specific active directory to prevent system instability.
    let tmpdir_path = env::var("TMPDIR").ok().map(PathBuf::from);

    // `Arc<Mutex<Vec<PathToCheck>>>` to store paths and their initial sizes after the first "checking" pass.
    // This allows collecting data from parallel threads into a single, shared, and mutable vector.
    let paths_to_clean: Arc<Mutex<Vec<PathToCheck>>> = Arc::new(Mutex::new(Vec::new()));
    // `AtomicU64` for `total_freed` to safely sum up bytes freed across multiple threads.
    let total_freed = Arc::new(AtomicU64::new(0));

    // Shared lists for recording operation outcomes, protected by `Arc<Mutex>`.
    let successful_entries = Arc::new(Mutex::new(Vec::<CleanupEntry>::new()));
    let failed_entries = Arc::new(Mutex::new(Vec::<FailedEntry>::new()));
    let skipped_during_size_check = Arc::new(Mutex::new(Vec::<SkippedEntry>::new()));


    // FIRST PASS: PARALLEL CHECKING AND INITIAL SIZING
    // In this phase, we only check paths, calculate their sizes, and log "Checking".
    // Actual deletion or "Would Clean" logging happens in the second pass.
    paths.par_iter().for_each(|path_str| {
        let path = PathBuf::from(path_str); // Convert the string slice to a `PathBuf` for path operations.

        // Critical skip: Check if the current path is either the global `/private/tmp` or
        // the specific active temporary directory defined by `TMPDIR`.
        // We skip these to prevent deleting files currently in use by the OS or running applications.
        if path.as_path() == PathBuf::from("/private/tmp").as_path() || tmpdir_path.as_ref().map_or(false, |tmp| path.as_path() == tmp.as_path()) {
            skipped_during_size_check.lock().unwrap().push(SkippedEntry {
                path: path.display().to_string(),
                reason: "Active system temporary directory".to_string(),
            });
            return; // Exit this parallel task for the current path.
        }

        log_info!("🔍 Checking: {}", path.display()); // Log that the tool is currently checking this path.

        // Verify if the path actually exists on the file system.
        if !path.exists() {
            let msg = "Not found".to_string();
            // If the path doesn't exist, record it as a failed entry and log a warning.
            failed_entries.lock().unwrap().push(FailedEntry {
                path: path.display().to_string(),
                error: msg.clone(),
            });
            log_warn!("⚠️ {}: {}", path.display(), msg);
            return; // Exit this parallel task for the current path.
        }

        // Attempt to get the size of the current path before any cleaning.
        match get_size(&path, &skipped_during_size_check) {
            Ok(size_before) => {
                // If the directory or file is empty, there's nothing to clean, so skip it.
                if size_before == 0 {
                    log_debug!(
                        "⚪ Skipping empty path: {}",
                        path.display().to_string().bright_magenta()
                    );
                    return; // Exit this parallel task for the current path.
                }

                // If the path is valid and has content, add it to the list of paths to process in the next phase.
                paths_to_clean.lock().unwrap().push(PathToCheck {
                    path,
                    initial_size: size_before,
                });
            }
            Err(e) => {
                // If `get_size` fails (e.g., initial permissions issue preventing even size calculation),
                // record it as a skipped path and log a warning.
                let err_str = e.to_string();
                let reason = format!("Size check failed: {}", err_str);
                skipped_during_size_check.lock().unwrap().push(SkippedEntry {
                    path: path.display().to_string(),
                    reason,
                });
                log_warn!(
                    "🚫 Size check failed for {}: {}",
                    path.display().to_string().bright_yellow(),
                    err_str.bright_white()
                );
            }
        }
    });

    //  SECOND PASS: PARALLEL CLEANING/DRY RUN AND LOGGING COLLECTION
    // After all initial checks are done, process the collected `paths_to_clean`.
    // This part runs in parallel, but the *final printing* will be sequential.
    paths_to_clean.lock().unwrap().par_iter().for_each(|item| {
        let path = &item.path;
        let size_before = item.initial_size;
        let size_fmt_before = format_bytes(size_before);

        if dry_run {
            // Dry run mode: Only report what *would* be cleaned.
            log_info!(
                "🧾 Would clean: {} ({})",
                path.display().to_string().bright_green(),
                size_fmt_before.bright_white().bold()
            );

            // Add to successful entries for the summary table.
            successful_entries.lock().unwrap().push(CleanupEntry {
                path: path.display().to_string(),
                size: size_fmt_before.clone(), // Use the initial size as the "would free" size.
            });

            // Add the initial size to the total freed, even in dry run, to show potential savings.
            total_freed.fetch_add(size_before, Ordering::Relaxed);
        } else {
            // Actual cleanup mode: Perform deletion.
            log_info!(
                "🧹 Cleaning: {} ({})",
                path.display().to_string().bright_green(),
                size_fmt_before.bright_white().bold()
            );

            // Attempt to remove the path using the utility function.
            if let Err(e) = remove_path(path) {
                let err_str = e.to_string();
                // Record the failure and log a warning.
                failed_entries.lock().unwrap().push(FailedEntry {
                    path: path.display().to_string(),
                    error: err_str.clone(),
                });
                log_warn!(
                    "❌ Failed to delete {}: {}",
                    path.display().to_string().bright_yellow(),
                    err_str.bright_white()
                );
            }

            // After attempted deletion, compute the size of the path again.
            // This helps in determining how much space was actually freed.
            match get_size(path, &skipped_during_size_check) {
                Ok(size_after) => {
                    let freed = size_before.saturating_sub(size_after); // Calculate freed space, saturating to 0 if size_after is unexpectedly larger.
                    if freed > 0 {
                        let freed_fmt = format_bytes(freed); // Format the actual freed space.
                        total_freed.fetch_add(freed, Ordering::Relaxed); // Add to the atomic total freed space.

                        // Record the successful cleanup with the actual freed size.
                        successful_entries.lock().unwrap().push(CleanupEntry {
                            path: path.display().to_string(),
                            size: freed_fmt,
                        });
                    } else {
                        // If no space was freed (e.g., deletion failed silently, or path was recreated), warn the user.
                        log_warn!("⚠️ No space freed for path: {}", path.display());
                    }
                }
                Err(e) => {
                    // If getting the size *after* cleanup fails, log and record it as a skipped path.
                    let reason = format!("Failed to get size after cleanup: {}", e);
                    log_warn!(
                        "🚫 Failed to get size after cleanup for {}: {}",
                        path.display(),
                        e
                    );
                    skipped_during_size_check.lock().unwrap().push(SkippedEntry {
                        path: path.display().to_string(),
                        reason,
                    });
                }
            }
        }
    });

    // FINAL SUMMARY GENERATION AND SEQUENTIAL PRINTING
    let total = total_freed.load(Ordering::Relaxed); // Load the final accumulated total freed space.
    let total_fmt = format_bytes(total); // Format the total into a human-readable string.

    // Retrieve and clone successful entries.
    let mut all_success = successful_entries.lock().unwrap().clone();
    // Add a "Total" row to the successful cleanup summary table for a comprehensive overview.
    all_success.push(CleanupEntry {
        path: "Total".to_string(),
        size: total_fmt.clone(),
    });

    // Create and print the table for successful cleanups.
    let table = Table::new(&all_success)
        .with(Style::modern()) // Apply a modern style to the table.
        .to_string(); // Convert the table to a string for printing.

    println!("\n{}", "🧾 Cleanup Summary (Successful)".bold().underline().green()); // Print header.
    println!("{}", table); // Print the successful cleanup table.

    let all_failures = failed_entries.lock().unwrap(); // Retrieve all failed entries.
    // If there were any failures, create and print the failures table.
    if !all_failures.is_empty() {
        let table = Table::new(&*all_failures) // Dereference the MutexGuard to get a reference to the vector.
            .with(Style::modern())
            .to_string();

        println!("\n{}", "⚠️ Cleanup Failures".bold().underline().yellow()); // Print header.
        println!("{}", table); // Print the failures table.
    }

    // Check if the `OSX_SHOW_SKIPPED` environment variable is set.
    // This allows the user to optionally view the table of skipped paths.
    // `env::var("OSX_SHOW_SKIPPED").is_ok()` returns true if the variable exists and is a valid string.
    if env::var("OSX_SHOW_SKIPPED").is_ok() {
        log_debug!("DEBUG: OSX_SHOW_SKIPPED is set inside the tool.");
    } else {
        log_debug!("DEBUG: OSX_SHOW_SKIPPED is NOT set inside the tool.");
    }
    if env::var("OSX_SHOW_SKIPPED").is_ok() || is_debug_enabled() {
        let all_skipped = skipped_during_size_check.lock().unwrap(); // Retrieve all skipped entries.
        // If there were any skipped paths, create and print the skipped paths table.
        if !all_skipped.is_empty() {
            let table = Table::new(&*all_skipped)
                .with(Style::modern())
                .to_string();

            println!("\n{}", "⚪ Skipped Paths (During Size Check)".bold().underline().magenta()); // Print header.
            println!("{}", table); // Print the skipped paths table.
        }
    }

    // Print the final message indicating the total space freed or estimated.
    if dry_run {
        log_info!("🧠 Estimated space to free: {}", total_fmt.bright_green().bold());
    } else {
        log_info!("✅ Total space freed: {}", total_fmt.bright_green().bold());
    }

    // Provide a warning if System Integrity Protection (SIP) is enabled,
    // as it can limit the effectiveness of the cleaner on system-protected files.
    if is_sip_enabled() {
        log_warn!(
            "{}",
            "⚠️  System Integrity Protection (SIP) is enabled. Some files may not be removable."
                .bright_yellow()
        );
    }

    log_debug!("✅ Finished clean_my_mac."); // Log the completion of the entire `clean_my_mac` function.
    Ok(()) // Return `Ok(())` to indicate successful execution of the function.
}