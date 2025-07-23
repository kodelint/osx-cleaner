use std::env;
use std::path::PathBuf;
use super::Cleaner; // Correctly import the Cleaner trait from the parent module

/// Represents a cleaner for crash reporter logs.
pub struct CrashReporterLogsCleaner;

impl CrashReporterLogsCleaner {
    pub fn new() -> Self {
        CrashReporterLogsCleaner // This simply returns an instance of the struct
    }
}

impl Cleaner for CrashReporterLogsCleaner { // FIXED: Removed the full path here
    fn name(&self) -> &str {
        "Crash Reporter Logs"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        let home = env::var("HOME").unwrap_or_default();
        vec![PathBuf::from(format!("{}/Library/Application Support/CrashReporter", home))]
    }
}