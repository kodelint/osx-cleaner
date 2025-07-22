use crate::utils::filesystem::remove_path;
// Imports the `remove_path` function, which is assumed to handle file/directory deletion.
use rayon::prelude::*;
use crate::{log_debug, log_info, log_warn};
// Imports traits from the `rayon` crate to enable parallel iteration over collections, improving performance for I/O bound tasks.
use std::{
    // Provides access to environment variables, like HOME.
    fs, // Provides file system primitives for interacting with files and directories.
    path::PathBuf, // A mutable, owned path type for building and manipulating file paths.
};
// Imports custom logging macros for different severity levels (debug, info, warn).
use colored::Colorize;
// Imports the `Colorize` trait from the `colored` crate, allowing for colored text output in the terminal.

/// Defines a common interface for any entity (e.g., an application, a CLI tool) that can be uninstalled.
///
/// This trait provides a default `uninstall` method that leverages the `find_related_paths`
/// method to identify and remove all associated files, including standard application paths,
/// macOS Launch Agents/Daemons, and package receipts.
pub trait Uninstaller {
    /// Returns the user-friendly name of the application or tool.
    /// This name is used for logging and for constructing common file paths.
    fn name(&self) -> &str;

    /// Discovers and returns a list of file system paths associated with the specific app or tool.
    /// These paths represent files and directories that belong to the entity and should be removed during uninstallation.
    fn find_related_paths(&self) -> Vec<PathBuf>;

    /// Executes the uninstallation process.
    ///
    /// It first gathers all relevant paths by combining `find_related_paths` with
    /// common macOS-specific locations for launch agents and package receipts.
    ///
    /// The deletion is performed in parallel using Rayon for efficiency.
    ///
    /// # Arguments
    /// * `dry_run` - A boolean flag. If `true`, the uninstaller will only log which files *would* be deleted
    ///               without actually performing any deletions. If `false`, actual deletion occurs.
    ///
    /// # Returns
    /// A `Result` indicating success (`Ok(())`) or failure (`Err(Box<dyn std::error::Error>)`).
    /// Errors are typically related to underlying file system operations.
    fn uninstall(&self, dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
        // Log the initiation of the uninstall process for clarity.
        log_debug!("Starting uninstall for '{}'", self.name().bright_white());

        // TODO: Advanced Improvement: Before proceeding with file deletion,
        // it would be beneficial to check if the target application is currently running.
        // If it is, the uninstaller could:
        // 1. Prompt the user to manually quit the application.
        // 2. Attempt to programmatically quit the application (requires elevated permissions and careful handling).
        // This prevents "Resource busy" errors and ensures a cleaner uninstall.

        // Collect all paths identified by the specific uninstaller implementation.
        let mut paths = self.find_related_paths();
        // Extend the list with paths to launch agents/daemons that might be associated with the app.
        paths.extend(find_launch_agents_for_app(self.name()));
        // Extend the list with paths to package installation receipts.
        paths.extend(find_pkg_receipts(self.name()));

        // Use Rayon's parallel iterator to process each path concurrently.
        // This can significantly speed up the operation, especially for many files.
        paths.par_iter().for_each(|path| {
            // Log the current path being considered for removal.
            log_debug!("Processing path: {}", path.display().to_string().bright_white());

            // Check if the path actually exists on the file system.
            if !path.exists() {
                // If the path does not exist, log a warning and skip to the next path.
                log_warn!("{}: {}", "Not found".bright_yellow(), path.display().to_string().bright_white());
                return; // Skips the rest of the current iteration for this path.
            }

            // Differentiate between a dry run and an actual deletion.
            if dry_run {
                // In dry-run mode, simply log what *would* be deleted without modifying the file system.
                log_info!("{}: {}", "Would delete".bright_green(), path.display().to_string().bright_white());
            } else {
                // In actual uninstall mode, attempt to delete the path.
                log_info!("{}: {}", "Deleting".bright_green(), path.display().to_string().bright_white());
                // Call the `remove_path` utility function.
                if let Err(e) = remove_path(path) {
                    // If deletion fails, log a warning with the specific path and the error message.
                    log_warn!("{} {}: {}", "Failed to delete".bright_yellow(),
                        path.display().to_string().bright_white(), e.to_string().bright_white());
                    // TODO: Advanced Improvement: Instead of just logging, these failed entries could be collected
                    // into a `Arc<Mutex<Vec<FailedEntry>>>` (similar to `cleaner.rs`) and reported in a final summary table
                    // to provide a more comprehensive overview of the uninstall failures to the user.
                }
            }
        });

        // Log the completion of the uninstall process for the specific app.
        log_debug!("Completed uninstall for '{}'", self.name().to_string().bright_white());
        // Return `Ok(())` if the uninstall process completed without critical errors,
        // even if some individual file deletions failed (which are logged as warnings).
        Ok(())
    }
}

/// Discovers and returns a list of `.plist` files that serve as Launch Agents or Launch Daemons
/// and appear to be related to the given application name.
/// These files are used by macOS to automatically launch applications or scripts at boot or login.
///
/// This function performs a case-insensitive substring search on the filename.
fn find_launch_agents_for_app(app_name: &str) -> Vec<PathBuf> {
    let mut plist_paths = Vec::new(); // Initialize an empty vector to store the found .plist paths.

    // Get the current user's home directory to construct user-specific LaunchAgents path.
    let home = std::env::var("HOME").unwrap_or_default();
    let user_launch_agents = format!("{}/Library/LaunchAgents", home);

    // Define the standard directories where macOS stores Launch Agents and Launch Daemons.
    // These are converted to `String` to be owned and avoid lifetime issues in the `for` loop.
    let dirs = vec![
        "/Library/LaunchAgents".to_string(), // System-wide Launch Agents.
        "/Library/LaunchDaemons".to_string(), // System-wide Launch Daemons.
        user_launch_agents, // User-specific Launch Agents.
    ];

    // Iterate through each of the defined directories.
    for dir in dirs {
        // Attempt to read the contents of the directory.
        if let Ok(entries) = fs::read_dir(&dir) {
            // Iterate over each entry (file or subdirectory) in the directory.
            // `flatten()` is used to discard `Err` results from `read_dir` entries.
            for entry in entries.flatten() {
                let path = entry.path(); // Get the full path of the current entry.
                // Extract the filename from the path and convert it to a string if possible.
                if let Some(fname) = path.file_name().and_then(|s| s.to_str()) {
                    // Check if the filename (converted to lowercase) contains the app name (also lowercase)
                    // AND if the filename ends with ".plist".
                    // This is a heuristic: a more precise method would parse the plist content for a BundleIdentifier or Label key.
                    if fname.to_lowercase().contains(&app_name.to_lowercase()) && fname.ends_with(".plist") {
                        plist_paths.push(path); // If both conditions are met, add the path to our list.
                    }
                }
            }
        }
    }

    plist_paths // Return the vector of discovered .plist paths.
}

/// Discovers and returns a list of package installation receipt files (`.pkg` or related)
/// that appear to be associated with the given application name.
/// These receipts track what files were installed by a macOS installer package.
///
/// This function performs a case-insensitive substring search on the filename.
fn find_pkg_receipts(app_name: &str) -> Vec<PathBuf> {
    let mut receipts = Vec::new(); // Initialize an empty vector to store the found receipt paths.
    // Define the standard directories where macOS stores package installation receipts.
    let receipt_dirs = vec!["/var/db/receipts", "/Library/Receipts"];

    // Iterate through each of the defined receipt directories.
    for dir in receipt_dirs {
        // Attempt to read the contents of the directory.
        if let Ok(entries) = fs::read_dir(dir) {
            // Iterate over each entry in the directory.
            for entry in entries.flatten() {
                let path = entry.path(); // Get the full path of the current entry.
                // Extract the filename from the path and convert it to a string.
                if let Some(fname) = path.file_name().and_then(|s| s.to_str()) {
                    // Check if the filename (converted to lowercase) contains the app name (also lowercase).
                    // Receipt names can be varied, so a substring match is a common approach.
                    if fname.to_lowercase().contains(&app_name.to_lowercase()) {
                        receipts.push(path); // If the condition is met, add the path to our list.
                    }
                }
            }
        }
    }

    receipts // Return the vector of discovered receipt paths.
}

/// Represents a standard macOS Graphical User Interface (GUI) application.
/// This struct implements the `Uninstaller` trait to define how a typical `.app` bundle
/// and its associated files should be uninstalled.
pub struct MacApp {
    name: String, // Stores the exact name of the macOS application (e.g., "Google Chrome", "Safari").
}

impl MacApp {
    /// Creates a new `MacApp` uninstaller instance.
    /// # Arguments
    /// * `name` - The name of the application.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(), // Convert the string slice to an owned String.
        }
    }
}

// Implement the `Uninstaller` trait for `MacApp`.
impl Uninstaller for MacApp {
    /// Returns the name of this `MacApp` instance.
    fn name(&self) -> &str {
        &self.name // Return a reference to the stored application name.
    }

    /// Discovers common file system paths related to a macOS GUI application.
    /// This includes the main application bundle, various support files, preferences, caches, and logs.
    fn find_related_paths(&self) -> Vec<PathBuf> {
        let home = std::env::var("HOME").unwrap_or_default(); // Get the user's home directory.

        let mut paths = vec![
            // 1. Main Application Bundle: The primary location of the `.app` file.
            PathBuf::from(format!("/Applications/{}.app", self.name)),

            // 2. Application Support Data: Configuration files, user data, etc.
            //    Can be system-wide (`/Library/`) or user-specific (`~/Library/`).
            PathBuf::from(format!("/Library/Application Support/{}", self.name)),
            PathBuf::from(format!("{}/Library/Application Support/{}", home, self.name)),

            // 3. Preferences (Property List files - .plist): Store application settings.
            //    Often follow a reverse-domain name convention (e.g., com.apple.Safari.plist).
            //    Using the app name as a heuristic; a more robust solution uses CFBundleIdentifier.
            PathBuf::from(format!("/Library/Preferences/com.{}.plist", self.name)), // System-wide preferences.
            PathBuf::from(format!("{}/Library/Preferences/com.{}.plist", home, self.name)), // User-specific preferences.

            // 4. Caches: Temporary files for faster performance.
            PathBuf::from(format!("{}/Library/Caches/{}", home, self.name)), // User-specific caches.

            // 5. Logs: Application log files.
            PathBuf::from(format!("{}/Library/Logs/{}", home, self.name)), // User-specific logs.

            // 6. Containers (for sandboxed applications): Applications run in isolated environments.
            //    These paths are based on the application's Bundle Identifier (e.g., `com.yourcompany.yourapp`).
            //    The current implementation uses a wildcard heuristic based on the app name.
            //    TODO: Implement parsing of the app's `Info.plist` to obtain the `CFBundleIdentifier`
            //    for precise container path construction (e.g., `format!("{}/Library/Containers/{}", home, bundle_id)`).
            PathBuf::from(format!("{}/Library/Containers/{}.*", home, self.name)), // Heuristic: Matches containers starting with app name.

            // 7. Group Containers (for apps sharing data): Used by multiple apps from the same developer.
            //    Also based on a group identifier. Heuristic used here.
            //    TODO: Similar to individual containers, more precise identification requires group IDs.
            PathBuf::from(format!("{}/Library/Group Containers/*{}.*", home, self.name)), // Heuristic: Matches group containers containing app name.

            // 8. Crash Reporter Logs: Plist files generated when an application crashes.
            PathBuf::from(format!("{}/Library/Application Support/CrashReporter/{}_*.plist", home, self.name)),

            // 9. Various Plug-Ins, Extensions, and Resources:
            //    These paths cover various types of extensions that apps might install.
            //    Both system-wide and user-specific locations are considered.
            PathBuf::from(format!("/Library/Input Methods/{}", self.name)),
            PathBuf::from(format!("/Library/Screen Savers/{}", self.name)),
            PathBuf::from(format!("/Library/Widgets/{}", self.name)),
            PathBuf::from(format!("/Library/QuickLook/{}", self.name)),
            PathBuf::from(format!("/Library/Internet Plug-Ins/{}", self.name)),
            PathBuf::from(format!("/Library/Fonts/{}", self.name)), // Some apps might install custom fonts.

            PathBuf::from(format!("{}/Library/Input Methods/{}", home, self.name)),
            PathBuf::from(format!("{}/Library/Screen Savers/{}", home, self.name)),
            PathBuf::from(format!("{}/Library/Widgets/{}", home, self.name)),
            PathBuf::from(format!("{}/Library/QuickLook/{}", home, self.name)),
            PathBuf::from(format!("{}/Library/Internet Plug-Ins/{}", home, self.name)),
            PathBuf::from(format!("{}/Library/Fonts/{}", home, self.name)),
        ];

        // Filter out any paths that might have resulted in empty strings (e.g., if `home` was empty
        // and some formatting created an empty path component).
        paths.retain(|p| !p.as_os_str().is_empty());

        paths // Return the comprehensive list of paths.
    }
}

/// Represents a command-line interface (CLI) tool installed on the system.
/// This struct implements the `Uninstaller` trait to define how a CLI tool
/// and its associated files should be uninstalled.
pub struct CliTool {
    name: String, // Stores the name of the command-line tool (e.g., "brew", "git").
}

impl CliTool {
    /// Creates a new `CliTool` uninstaller instance.
    /// # Arguments
    /// * `name` - The name of the CLI tool.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(), // Convert the string slice to an owned String.
        }
    }
}

// Implement the `Uninstaller` trait for `CliTool`.
impl Uninstaller for CliTool {
    /// Returns the name of this `CliTool` instance.
    fn name(&self) -> &str {
        &self.name // Return a reference to the stored tool name.
    }

    /// Discovers common file system paths where command-line tools and their related files might be found.
    /// This includes common binary locations, libraries, documentation, and configuration files.
    fn find_related_paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![
            // 1. Common Binary Locations: Where executables are typically installed.
            PathBuf::from(format!("/usr/local/bin/{}", self.name)), // Common for user-installed binaries.
            PathBuf::from(format!("/usr/bin/{}", self.name)), // System binaries (less common for uninstallation).
            PathBuf::from(format!("/opt/homebrew/bin/{}", self.name)), // Homebrew's default binary symlink path.

            // 2. Libraries and Frameworks: Shared components used by the tool.
            PathBuf::from(format!("/usr/local/lib/{}", self.name)), // Libraries specific to the tool.
            PathBuf::from(format!("/Library/Frameworks/{}.framework", self.name)), // System-wide frameworks.
            PathBuf::from(format!("/usr/local/Frameworks/{}.framework", self.name)), // Local frameworks.

            // 3. Documentation and Man Pages: Help files for the tool.
            PathBuf::from(format!("/usr/local/share/man/man1/{}.1", self.name)), // Manual pages.
            PathBuf::from(format!("/usr/local/share/doc/{}", self.name)), // General documentation.

            // 4. Configuration Files: Settings and configuration for the tool.
            PathBuf::from(format!("/etc/{}", self.name)), // System-wide configuration.
            PathBuf::from(format!("/etc/paths.d/{}", self.name)), // Files that add directories to the system's PATH.
        ];

        // 5. Homebrew Specific Paths: If Homebrew is installed, check its cellar for the tool's actual installation directory.
        //    Homebrew installs tools into a "Cellar" and then symlinks them to `/opt/homebrew/bin` or `/usr/local/bin`.
        //    Checking the Cellar ensures the original installation directory is targeted for removal.
        if PathBuf::from("/opt/homebrew/Cellar").exists() {
            paths.push(PathBuf::from(format!("/opt/homebrew/Cellar/{}", self.name)));
        }
        if PathBuf::from("/usr/local/Cellar").exists() { // For older Homebrew installations or specific setups.
            paths.push(PathBuf::from(format!("/usr/local/Cellar/{}", self.name)));
        }

        // Filter out any paths that might have resulted in empty strings.
        paths.retain(|p| !p.as_os_str().is_empty());

        paths // Return the comprehensive list of paths.
    }
}