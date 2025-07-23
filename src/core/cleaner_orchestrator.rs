use std::{
    env,
    sync::atomic::AtomicU64,
    sync::{Arc, Mutex},
};
use std::sync::atomic::Ordering;
use colored::Colorize;
use tabled::{settings::Style, Table};
use crate::logger::is_debug_enabled;
use crate::{log_debug, log_info, log_warn};
use rayon::prelude::*;

// Import the Cleaner trait and all specific cleaner implementations
use super::cleaners::{
    Cleaner,
    SystemCachesCleaner,
    UserCachesCleaner,
    TemporaryFilesCleaner,
    UserLogsCleaner,
    CrashReporterLogsCleaner,
    TrashCleaner,
    LargeFilesCleaner,
    CleanupEntry,
    BrowserCachesCleaner,
    FailedEntry,
    SkippedEntry,
    PathToCheck,
    format_bytes,
    is_sip_enabled,
};

// Helper function to print a category of info-level logs
fn print_info_logs_category(logs_arc: &Arc<Mutex<Vec<String>>>) {
    let logs = logs_arc.lock().unwrap();
    if !logs.is_empty() {
        eprintln!("\n");
        for log_line in logs.iter() {
            log_info!("\t{}", log_line);
        }
    }
}

/// The main function for cleaning macOS system junk and temporary files.
/// It orchestrates the cleaning process by iterating through different `Cleaner` implementations.
///
/// # Arguments
/// * `dry_run` - A boolean flag. If `true`, no files will be deleted; only a report
///               of what *would* be deleted is shown.
/// * `ignore` - A `Vec<String>` of substrings. Any path containing...
pub fn clean_my_mac(dry_run: bool, ignore: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    log_debug!("Starting clean_my_mac (dry_run: {})", dry_run);

    // Initialize cleaners that are always part of the 'junk' cleaning
    let standard_cleaners: Vec<Box<dyn Cleaner>> = vec![
        Box::new(SystemCachesCleaner::new()),
        Box::new(UserCachesCleaner::new()),
        Box::new(TemporaryFilesCleaner::new()),
        Box::new(UserLogsCleaner::new()),
        Box::new(CrashReporterLogsCleaner::new()),
        Box::new(TrashCleaner::new()),
        Box::new(BrowserCachesCleaner::new()),
    ];

    // Shared accumulators for logs and results across all parallel cleaners
    let all_successful_entries: Arc<Mutex<Vec<CleanupEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let all_failed_entries: Arc<Mutex<Vec<FailedEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let all_skipped_during_size_check: Arc<Mutex<Vec<SkippedEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let total_freed_space = Arc::new(AtomicU64::new(0));

    // Shared accumulators for categorized logs
    let checking_logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let cleaning_logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let would_clean_logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // This now needs to be an Arc<Mutex> because it's mutated in a parallel closure.
    let all_paths_to_clean_after_check: Arc<Mutex<Vec<PathToCheck>>> = Arc::new(Mutex::new(Vec::new()));

    // New accumulator specifically for large files when in dry run mode
    let large_files_to_display_in_dry_run: Arc<Mutex<Vec<PathToCheck>>> = Arc::new(Mutex::new(Vec::new()));


    // --- Phase 1: Collect all paths to check and log "Checking" ---
    // Process standard cleaners
    standard_cleaners.par_iter().for_each(|cleaner| {
        // Pass the ignore list to the cleaner's clean method
        match cleaner.clean(&checking_logs, &all_skipped_during_size_check, &ignore) {
            Ok(paths) => {
                all_paths_to_clean_after_check.lock().unwrap().extend(paths);
            }
            Err(e) => {
                log_warn!("❌ Cleaner '{}' failed to identify paths: {}", cleaner.name(), e);
                all_failed_entries.lock().unwrap().push(FailedEntry {
                    path: format!("Cleaner: {}", cleaner.name()),
                    error: format!("Failed to run: {}", e),
                });
            }
        }
    });

    // Special handling for LargeFilesCleaner based on dry_run mode
    let large_files_cleaner_instance = LargeFilesCleaner::new();
    match large_files_cleaner_instance.clean(&checking_logs, &all_skipped_during_size_check, &ignore) {
        Ok(paths_found_by_large_cleaner) => {
            if dry_run {
                // If dry run, these large files go to a separate display table
                large_files_to_display_in_dry_run.lock().unwrap().extend(paths_found_by_large_cleaner);
            } else {
                // If not dry run, these large files go to the main list for actual cleaning
                all_paths_to_clean_after_check.lock().unwrap().extend(paths_found_by_large_cleaner);
            }
        }
        Err(e) => {
            log_warn!("❌ Large Files Cleaner failed to identify paths: {}", e);
            all_failed_entries.lock().unwrap().push(FailedEntry {
                path: "Large Files Cleaner".to_string(),
                error: format!("Failed to run: {}", e),
            });
        }
    }


    eprintln!("\n{}", "🔍 Verifying Paths...".bold().underline().cyan());
    print_info_logs_category(&checking_logs);

    // Phase 2: Perform (or simulate) Cleaning and Log "Cleaning" or "Would clean"
    eprintln!("\n{}", "🚚🧹 Performing Cleanup...".bold().underline().green());

    // Acquire lock once to get a reference to the inner Vec, then use par_iter on that reference
    let paths_to_process = all_paths_to_clean_after_check.lock().unwrap();
    paths_to_process.par_iter().for_each(|p| {
        let path_display = p.path.display().to_string();
        let size_fmt_before = p.formatted_size.clone();

        match crate::utils::filesystem::remove_path(&p.path, dry_run) {
            Ok(_) => {
                if dry_run {
                    would_clean_logs.lock().unwrap().push(format!(
                        "🧹🪣 Would clean: {} ({})",
                        path_display.bright_green(),
                        size_fmt_before.bright_white().bold()
                    ));
                    // MODIFIED: Increment total_freed_space even in dry_run mode
                    total_freed_space.fetch_add(p.initial_size, Ordering::SeqCst);
                } else {
                    cleaning_logs.lock().unwrap().push(format!(
                        "🧹🪣 After Cleaning: {} ({})",
                        path_display.bright_green(),
                        size_fmt_before.bright_white().bold()
                    ));
                    all_successful_entries.lock().unwrap().push(CleanupEntry {
                        path: path_display,
                        size: size_fmt_before,
                        cleaner_name: p.cleaner_name.clone(),
                    });
                    total_freed_space.fetch_add(p.initial_size, Ordering::SeqCst);
                }
            }
            Err(e) => {
                log_warn!("❌ Failed to clean {}: {}", path_display, e);
                all_failed_entries.lock().unwrap().push(FailedEntry {
                    path: path_display,
                    error: e.to_string(),
                });
            }
        }
    });

    print_info_logs_category(&cleaning_logs);
    print_info_logs_category(&would_clean_logs);


    // --- Summary Section ---
    let total_fmt = format_bytes(total_freed_space.load(Ordering::SeqCst));

    let mut all_success = all_successful_entries.lock().unwrap();
    all_success.push(CleanupEntry {
        path: "Total".to_string(),
        size: total_fmt.clone(),
        cleaner_name: "".to_string(),
    });

    let table = Table::new(&*all_success)
        .with(Style::modern())
        .to_string();

    if dry_run {
        println!("\n{}", "📊🧠 Estimated Cleanup Summary (Dry Run)".bold().underline().purple());
    } else {
        println!("\n{}", "📥📄🗑️ Cleanup Summary (Successful)".bold().underline().green());
    }
    println!("{}", table);

    let all_failures = all_failed_entries.lock().unwrap();
    if !all_failures.is_empty() {
        eprintln!("\n");
        let table = Table::new(&*all_failures)
            .with(Style::modern())
            .to_string();

        println!("{}", "⚠️ Cleanup Failures".bold().underline().yellow());
        println!("{}", table);
    }

    // Display Large Files table if in dry run mode and files were found
    if dry_run {
        let large_files_for_display = large_files_to_display_in_dry_run.lock().unwrap();
        if !large_files_for_display.is_empty() {
            eprintln!("\n");
            // Convert PathToCheck to CleanupEntry for display purposes
            let display_entries: Vec<CleanupEntry> = large_files_for_display.iter().map(|p| CleanupEntry {
                path: p.path.display().to_string(),
                size: p.formatted_size.clone(),
                cleaner_name: p.cleaner_name.clone(),
            }).collect();
            let table = Table::new(&display_entries)
                .with(Style::modern())
                .to_string();

            println!("{}", "📦 Large Files Found (Dry Run)".bold().underline().blue());
            println!("{}", table);
        }
    }


    if env::var("OSX_SHOW_SKIPPED").is_ok() || is_debug_enabled() {
        let all_skipped = all_skipped_during_size_check.lock().unwrap();
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
        log_info!(
            "{}",
            "⚠️  System Integrity Protection (SIP) is enabled. Some files may not be removable."
                .bright_yellow()
        );
    }

    log_debug!("✅ Finished clean_my_mac.");
    Ok(())
}