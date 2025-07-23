// This file implements the application's logging system.
// It provides macros for different log levels (INFO, WARN, ERROR, DEBUG)
// and handles conditional output, especially for debug messages, with colored terminal output.

use colored::*; // Used for adding color to log messages.
use std::sync::atomic::{AtomicBool, Ordering}; // For thread-safe, atomic control of the debug flag.
use std::sync::OnceLock; // Ensures the DEBUG_ENABLED flag is initialized exactly once.

/// Provides convenient logging macros.
/// `#[macro_export]` makes these macros globally available within the crate.

// `log_info!` for general application progress and informational messages.
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => (eprintln!("{} {}", "[INFO]".bright_green(), format!($($arg)*)));
}

// // `log_warn!` for non-critical issues or noteworthy conditions.
// #[macro_export]
// macro_rules! log_warn {
//     ($($arg:tt)*) => (eprintln!("{} {}", "[WARN]".bright_yellow(), format!($($arg)*)));
// }

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => ({
        // Only print if OSX_SHOW_WARNINGS environment variable is set
        if std::env::var("OSX_SHOW_WARNINGS").is_ok() {
            eprintln!("{} {}", "[WARN]".bright_yellow(), format!($($arg)*));
        }
    });
}

// `log_error!` for critical errors requiring immediate attention.
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => (eprintln!("{} {}", "[ERROR]".bright_red(), format!($($arg)*)));
}

// `log_debug!` for detailed internal application tracing.
// Messages are only printed if debug mode is enabled via `is_debug_enabled()`.
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        if $crate::logger::is_debug_enabled() {
           eprintln!("{} {}", "[DEBUG]".dimmed(), format!($($arg)*));
        }
    };
}

// Global flag to control debug logging, ensured to be initialized once.
static DEBUG_ENABLED: OnceLock<AtomicBool> = OnceLock::new();

/// Initializes the logger, setting the global debug mode.
/// This function should be called once at application startup.
///
/// # Arguments
/// * `debug`: If `true`, enables debug logging; otherwise, only info, warn, and error messages are printed.
pub fn init(debug: bool) {
    DEBUG_ENABLED
        .get_or_init(|| AtomicBool::new(debug)) // Initialize if not already set.
        .store(debug, Ordering::Relaxed); // Update the flag with the provided debug value.

    if debug {
        log_debug!("Logger initialized in DEBUG mode");
    }
}

/// Checks if debug logging is currently enabled.
/// Used primarily by the `log_debug!` macro.
///
/// # Returns
/// * `true` if debug logging is enabled, `false` otherwise.
pub fn is_debug_enabled() -> bool {
    DEBUG_ENABLED
        .get() // Attempt to retrieve the AtomicBool.
        .map(|f| f.load(Ordering::Relaxed)) // Load its value if present.
        .unwrap_or(false) // Default to false if `init` was never called.
}