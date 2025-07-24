use crate::logger::is_debug_enabled;
use crate::utils::filesystem::split_filenames;
use crate::{log_debug, log_info, log_warn};
use colored::Colorize;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::{
    collections::HashMap,
    env,
    sync::atomic::AtomicU64,
    sync::{Arc, Mutex},
};
use tabled::{Table, settings::Style};
// Import the Cleaner trait and all specific cleaner implementations
use super::cleaners::{
    BrowserCachesCleaner, Cleaner, CleanupEntry, CrashReporterLogsCleaner, FailedEntry,
    LargeFilesCleaner, PathToCheck, SkippedEntry, SystemCachesCleaner, TemporaryFilesCleaner,
    TrashCleaner, UserCachesCleaner, UserLogsCleaner, format_bytes, is_sip_enabled,
};

// Helper function to update the aggregated log maps
// This function takes a mutable reference to an Arc<Mutex<HashMap<String, u64>>>
// This allows it to update shared, thread-safe hash maps from multiple threads.
fn update_aggregated_log_map(
    log_map: &Arc<Mutex<HashMap<String, u64>>>,
    cleaner_name: &str,
    path: &PathBuf,
    size: u64,
) {
    // Determine the common display path for aggregation.
    // If it's a file within a directory, the parent directory is used for aggregation.
    // Otherwise, the path itself is used. This helps group related files under a common entry.
    let common_display_path = if path.components().count() > 1 && path.file_name().is_some() {
        // For files within a directory, log the parent directory for aggregation
        path.parent().unwrap_or(path).display().to_string()
    } else {
        // For top-level directories (which cleaners often return), log the path itself
        path.display().to_string()
    };

    // Create a unique key for the HashMap entry, combining cleaner name and the common path.
    let entry_key = format!("{}: {}", cleaner_name, common_display_path);
    // Acquire a lock on the log map, then update the size for the corresponding entry.
    // If the entry doesn't exist, it's inserted with the current size; otherwise, the size is added.
    *log_map.lock().unwrap().entry(entry_key).or_insert(0) += size;
}

/// The main function for cleaning macOS system junk and temporary files.
/// It orchestrates the cleaning process by iterating through different `Cleaner` implementations.
///
/// # Arguments
/// * `dry_run` - A boolean flag. If `true`, no files will be deleted; only a report
///               of what *would* be deleted is shown.
/// * `ignore` - A `Vec<String>` of substrings. Any path containing these substrings
///              will be ignored during the cleaning process.
///
/// # Returns
/// A `Result` indicating success or failure. On success, it returns `Ok(())`.
/// On failure, it returns `Err` with a `Box<dyn std::error::Error>` detailing the error.
pub fn clean_my_mac(dry_run: bool, ignore: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    log_debug!("Starting clean_my_mac (dry_run: {})", dry_run);

    // Initialize a vector of `Cleaner` trait objects. These are the standard cleaners
    // that target common junk files like caches, temporary files, logs, and trash.
    let standard_cleaners: Vec<Box<dyn Cleaner>> = vec![
        Box::new(SystemCachesCleaner::new()),
        Box::new(UserCachesCleaner::new()),
        Box::new(TemporaryFilesCleaner::new()),
        Box::new(UserLogsCleaner::new()),
        Box::new(CrashReporterLogsCleaner::new()),
        Box::new(TrashCleaner::new()),
        Box::new(BrowserCachesCleaner::new()),
    ];

    // Shared accumulators for logs and results across all parallel cleaners.
    // `Arc<Mutex<T>>` is used to allow safe shared access and mutation from multiple threads.
    let all_successful_entries_map: Arc<Mutex<HashMap<String, u64>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let all_failed_entries: Arc<Mutex<Vec<FailedEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let all_skipped_during_size_check: Arc<Mutex<Vec<SkippedEntry>>> =
        Arc::new(Mutex::new(Vec::new()));
    let total_freed_space = Arc::new(AtomicU64::new(0)); // Atomic for thread-safe sum of bytes.

    // Shared accumulators for categorized logs. These HashMaps will store aggregated
    // information (e.g., total size for a given cleaner and path category).
    let checking_logs_map: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));
    let cleaning_logs_map: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));

    // This vector will store all paths identified for potential cleaning after their size has been checked.
    // It's wrapped in `Arc<Mutex>` because it will be populated by parallel threads.
    let all_paths_to_clean_after_check: Arc<Mutex<Vec<PathToCheck>>> =
        Arc::new(Mutex::new(Vec::new()));

    // New accumulator specifically for large files when in dry run mode.
    // This HashMap will store aggregated information about large files to be displayed.
    let large_files_to_display_in_dry_run_map: Arc<Mutex<HashMap<String, u64>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Phase 1: Collect all paths to check and log "Checking"
    // This phase identifies files and directories that are candidates for cleaning.
    eprintln!("\n{}", "🔍 Verifying Paths...".bold().underline().cyan());
    eprint!("\n"); // Added for consistent spacing

    // Process standard cleaners in parallel using Rayon's `par_iter()`.
    // Each cleaner identifies paths it can clean.
    standard_cleaners.par_iter().for_each(|cleaner| {
        // `cleaner.clean` is called to get a list of paths the cleaner has found.
        // The `Arc<Mutex<Vec<>>` arguments are currently dummy; aggregation happens later.
        match cleaner.clean(
            &Arc::new(Mutex::new(Vec::new())),
            &all_skipped_during_size_check,
            &ignore,
        ) {
            Ok(paths_found_by_cleaner) => {
                // Acquire a lock on `all_paths_to_clean_after_check` to add new paths safely.
                let mut paths_to_process_lock = all_paths_to_clean_after_check.lock().unwrap();

                for p in paths_found_by_cleaner {
                    // Calculate the size of each found path.
                    match crate::utils::filesystem::calculate_dir_size(&p.path) {
                        Ok(size) => {
                            let formatted_size = crate::utils::filesystem::bytes_to_human(size);
                            // Use the helper function to update the aggregated "checking" logs.
                            update_aggregated_log_map(
                                &checking_logs_map,
                                cleaner.name(),
                                &p.path,
                                size,
                            );

                            // Add the path along with its size and cleaner name to the list for processing.
                            paths_to_process_lock.push(PathToCheck {
                                path: p.path,
                                initial_size: size,
                                formatted_size,
                                cleaner_name: cleaner.name().to_string(),
                            });
                        }
                        Err(e) => {
                            // Log a warning if the size cannot be determined for a path.
                            log_warn!(
                                "⚠️ Could not determine size for path {}: {}",
                                p.path.display(),
                                e
                            );
                            // Record the skipped entry.
                            all_skipped_during_size_check
                                .lock()
                                .unwrap()
                                .push(SkippedEntry {
                                    path: p.path.display().to_string(),
                                    reason: format!(
                                        "Could not determine size or access path: {}",
                                        e
                                    ),
                                });
                        }
                    }
                }
            }
            Err(e) => {
                // Log a warning if a cleaner fails to identify any paths.
                log_warn!(
                    "❌ Cleaner '{}' failed to identify paths: {}",
                    cleaner.name(),
                    e
                );
                // Record the failed cleaner.
                all_failed_entries.lock().unwrap().push(FailedEntry {
                    path: format!("Cleaner: {}", cleaner.name()),
                    error: format!("Failed to run: {}", e),
                });
            }
        }
    });

    // Special handling for `LargeFilesCleaner` based on `dry_run` mode.
    // Large files are typically not removed by default unless explicitly configured.
    let large_files_cleaner_instance = LargeFilesCleaner::new();
    match large_files_cleaner_instance.clean(
        &Arc::new(Mutex::new(Vec::new())),
        &all_skipped_during_size_check,
        &ignore,
    ) {
        Ok(paths_found_by_large_cleaner) => {
            if dry_run {
                // If in dry run, aggregate these large files into a separate map for display only.
                for p in paths_found_by_large_cleaner {
                    match crate::utils::filesystem::calculate_dir_size(&p.path) {
                        Ok(size) => {
                            // Update the map specifically for large files in dry run.
                            update_aggregated_log_map(
                                &large_files_to_display_in_dry_run_map,
                                &p.cleaner_name,
                                &p.path,
                                size,
                            );
                        }
                        Err(e) => {
                            // Log warning if size of a large file cannot be determined.
                            log_warn!(
                                "⚠️ Could not determine size for large file path {}: {}",
                                p.path.display(),
                                e
                            );
                            all_skipped_during_size_check
                                .lock()
                                .unwrap()
                                .push(SkippedEntry {
                                    path: p.path.display().to_string(),
                                    reason: format!(
                                        "Could not determine size or access for large file: {}",
                                        e
                                    ),
                                });
                        }
                    }
                }
            } else {
                // If not dry run, these large files are added to the main list for actual cleaning.
                all_paths_to_clean_after_check
                    .lock()
                    .unwrap()
                    .extend(paths_found_by_large_cleaner);
            }
        }
        Err(e) => {
            // Log warning if the Large Files Cleaner fails.
            log_warn!("❌ Large Files Cleaner failed to identify paths: {}", e);
            all_failed_entries.lock().unwrap().push(FailedEntry {
                path: "Large Files Cleaner".to_string(),
                error: format!("Failed to run: {}", e),
            });
        }
    }

    // Print aggregated checking logs.
    let checking_logs_map_locked = checking_logs_map.lock().unwrap();
    if !checking_logs_map_locked.is_empty() {
        let mut sorted_logs: Vec<_> = checking_logs_map_locked.iter().collect();
        // Sort logs by key (cleaner name/path) for consistent output.
        sorted_logs.sort_by_key(|&(k, _)| k);

        for (key, &total_size) in sorted_logs {
            let (cleaner_name_only, path_only) = split_filenames(key);
            log_info!(
                "🔍 Checking: '{}' {} ({})",
                cleaner_name_only.white(),
                path_only.white().dimmed(),
                format_bytes(total_size).bright_white().bold()
            );
        }
    }

    // Phase 2: Perform (or simulate) Cleaning and Log "Cleaning" or "Would clean"
    // This phase either deletes the identified files or reports what would be deleted.
    if !dry_run && std::env::var("OSX_SHOW_DETAILS").is_ok() {
        eprintln!(
            "\n{}",
            "🚚🧹 Performing Cleanup...".bold().underline().green()
        );
        eprint!("\n"); // Added for consistent spacing
    }

    // Acquire lock once to get a reference to the inner Vec, then use `par_iter` on that reference.
    // This avoids repeatedly locking and unlocking the mutex for each item.
    let paths_to_process = all_paths_to_clean_after_check.lock().unwrap();
    paths_to_process.par_iter().for_each(|p| {
        let path_display = p.path.display().to_string();
        // Attempt to remove the path. `dry_run` controls actual deletion.
        match crate::utils::filesystem::remove_path(&p.path, dry_run) {
            Ok(_) => {
                // Use the helper function to update the aggregated "cleaning" logs.
                update_aggregated_log_map(
                    &cleaning_logs_map,
                    &p.cleaner_name,
                    &p.path,
                    p.initial_size,
                );

                // Update the `all_successful_entries_map` for the final summary table.
                update_aggregated_log_map(
                    &all_successful_entries_map,
                    &p.cleaner_name,
                    &p.path,
                    p.initial_size,
                );
                // Atomically add the cleaned size to the total freed space.
                total_freed_space.fetch_add(p.initial_size, Ordering::SeqCst);
            }
            Err(e) => {
                // Log a warning if cleaning fails for a specific path.
                log_warn!("❌ Failed to clean {}: {}", path_display, e);
                // Record the failed entry.
                all_failed_entries.lock().unwrap().push(FailedEntry {
                    path: path_display,
                    error: e.to_string(),
                });
            }
        }
    });

    // Print aggregated cleaning logs.
    let cleaning_logs_map_locked = cleaning_logs_map.lock().unwrap();
    if !cleaning_logs_map_locked.is_empty() {
        let mut sorted_logs: Vec<_> = cleaning_logs_map_locked.iter().collect();
        // Sort logs for consistent output.
        sorted_logs.sort_by_key(|&(k, _)| k);
        if dry_run {
            eprintln!(
                "\n{}",
                "☑️  Will reclaimed Space...\n".bold().underline().cyan()
            );
        } else {
            eprintln!("\n{}", "☑️  Reclaimed Space...\n".bold().underline().cyan());
        }
        for (key, &total_size) in sorted_logs {
            let (cleaner_name_only, path_only) = split_filenames(key);
            if dry_run {
                log_info!(
                    "🧹🪣 Would Clean: '{}' {} ({})",
                    cleaner_name_only.bright_white(),
                    path_only.white().dimmed(),
                    format_bytes(total_size).bright_white().bold()
                );
            } else {
                log_info!(
                    "🧹🪣 After Clean: '{}' {} ({})",
                    cleaner_name_only.bright_white(),
                    path_only.white().dimmed(),
                    format_bytes(total_size).bright_white().bold()
                );
            }
        }
    }

    // Summary Section
    // Format the total freed space for display.
    let total_fmt = format_bytes(total_freed_space.load(Ordering::SeqCst));

    let mut final_successful_entries: Vec<CleanupEntry> = Vec::new();
    let aggregated_success_map = all_successful_entries_map.lock().unwrap();

    // Convert the aggregated success map into a vector of `CleanupEntry` for the summary table.
    let mut sorted_aggregated_success: Vec<_> = aggregated_success_map.iter().collect();
    sorted_aggregated_success.sort_by_key(|&(k, _)| k); // Sort for consistent order.

    for (key, &total_size) in sorted_aggregated_success {
        // Split the aggregated key back into cleaner name and path for table display.
        let (cleaner_name_only, path_only) = split_filenames(key);

        final_successful_entries.push(CleanupEntry {
            path: path_only,
            size: format_bytes(total_size),
            cleaner_name: cleaner_name_only,
        });
    }

    // Add a total row to the successful cleanup entries.
    final_successful_entries.push(CleanupEntry {
        path: "Total".to_string(),
        size: total_fmt.clone(),
        cleaner_name: "".to_string(),
    });

    // Create and print the table for successful cleanups.
    let table = Table::new(&final_successful_entries) // Use the new aggregated vector
        .with(Style::modern())
        .to_string();

    if dry_run {
        println!(
            "\n{}\n",
            "📥📄🗑️  Estimated Cleanup Summary (Dry Run)"
                .bold()
                .underline()
                .purple()
        );
    } else {
        println!(
            "\n{}\n",
            "📥📄🗑️  Cleanup Summary (Successful)"
                .bold()
                .underline()
                .green()
        );
    }
    println!("{}", table);

    // Display cleanup failures if any occurred.
    let all_failures = all_failed_entries.lock().unwrap();
    if !all_failures.is_empty() {
        eprintln!("\n");
        let table = Table::new(&*all_failures).with(Style::modern()).to_string();

        println!("{}", "⚠️ Cleanup Failures".bold().underline().yellow());
        println!("{}", table);
    }

    // Display Large Files table if in dry run mode and files were found.
    if dry_run {
        let large_files_for_display_map_locked =
            large_files_to_display_in_dry_run_map.lock().unwrap();
        if !large_files_for_display_map_locked.is_empty() {
            eprintln!("\n");
            let mut display_entries: Vec<CleanupEntry> = Vec::new();
            let mut total_large_file_size = 0;

            let mut sorted_large_files: Vec<_> =
                large_files_for_display_map_locked.iter().collect();
            sorted_large_files.sort_by_key(|&(k, _)| k);

            for (key, &total_size) in sorted_large_files {
                let (cleaner_name_only, path_only) = split_filenames(key);

                display_entries.push(CleanupEntry {
                    path: path_only,
                    size: format_bytes(total_size),
                    cleaner_name: cleaner_name_only,
                });
                total_large_file_size += total_size;
            }

            display_entries.push(CleanupEntry {
                path: "Total Large Files".to_string(),
                size: format_bytes(total_large_file_size),
                cleaner_name: "".to_string(),
            });

            let table = Table::new(&display_entries)
                .with(Style::modern())
                .to_string();

            println!(
                "{}",
                "📦 Large Files Found (Dry Run)".bold().underline().blue()
            );
            println!("{}", table);
        }
    }

    // Display skipped paths during size check if the debug environment variable is set or debug logging is enabled.
    if env::var("OSX_SHOW_SKIPPED").is_ok() || is_debug_enabled() {
        let all_skipped = all_skipped_during_size_check.lock().unwrap();
        if !all_skipped.is_empty() {
            eprintln!("\n");
            let table = Table::new(&*all_skipped).with(Style::modern()).to_string();

            println!(
                "{}",
                "⚪ Skipped Paths (During Size Check)"
                    .bold()
                    .underline()
                    .magenta()
            );
            println!("{}", table);
        }
    }

    eprintln!("\n");
    if dry_run {
        log_info!(
            "🧠 Estimated space to free: {}",
            total_fmt.bright_green().bold()
        );
    } else {
        log_info!("✔ Total space freed: {}", total_fmt.bright_green().bold());
    }

    // Warn the user if System Integrity Protection (SIP) is enabled, as it might limit cleaning.
    if is_sip_enabled() {
        log_info!(
            "{}",
            "⚠️  System Integrity Protection (SIP) is enabled. Some files may not be removable."
                .bright_yellow()
        );
    }
    log_debug!("✅ Finished clean_my_mac.");
    Ok(())
}
