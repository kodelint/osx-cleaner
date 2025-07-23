use crate::{log_debug, log_info};
// Imports the `log_debug` macro for logging debug-level messages.
use colored::Colorize;
// Imports the `Colorize` trait from the `colored` crate, used for adding color to terminal output strings.
use std::fs;
// Imports the standard library's file system module for operations like deleting files/directories, reading metadata, etc.
use std::io;
// Imports the standard library's I/O module, primarily for `io::Result` and `io::Error`.
use std::path::Path;
// Imports `Path` from the standard library, a universal type for file system paths.

/// Recursively deletes a file or directory at the given path.
///
/// This function attempts to remove the specified file system entry.
/// It distinguishes between files/symlinks and directories to use the appropriate deletion method.
///
/// If the path does not exist, this function returns `Ok(())` immediately, as there's nothing to delete.
///
/// # Arguments
/// * `path` - A reference to a `Path` indicating the file or directory to be removed.
/// * `dry_run` - A boolean flag. If `true`, the function will simulate the removal
///               without actually deleting files.
///
/// # Errors
///
/// Returns an `io::Error` if the removal of the file or directory fails for any reason
/// (e.g., permission denied, path is locked, disk error) during an actual run (`dry_run` is `false`).
///
/// # Behavior
///
/// - If `dry_run` is true, it simply logs a message indicating that the path *would* be removed.
/// - If the `path` does not exist, it returns `Ok(())` as the desired state is met.
/// - (Commented out in provided code, but typically) If the `path` points to a regular file or a symbolic link,
///   it would use `fs::remove_file()`.
/// - (Commented out in provided code, but typically) If the `path` points to a directory, it would recursively delete
///   the directory and all its contents using `fs::remove_dir_all()`.
/// - (Commented out in provided code, but typically) For other filesystem object types, it would attempt to delete
///   as a file using `fs::remove_file()`.
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use osx::utils::filesystem::remove_path;
/// // Attempt to remove a file named "somefile" in the /tmp directory (not a dry run).
/// remove_path(Path::new("/tmp/somefile"), false).expect("Failed to remove path");
/// // Simulate removing a directory named "temp_dir" in /var (dry run).
/// remove_path(Path::new("/var/temp_dir"), true).expect("Failed to simulate removal");
/// ```
pub fn remove_path(path: &Path, dry_run: bool) -> io::Result<()> { // Added dry_run parameter
    // Log the attempt to remove the path at debug level.
    log_debug!("Attempting to remove path: {}", path.display());

    // If `dry_run` is true, simulate the removal.
    if dry_run {
        // log_info!("🧠 Pretending to remove: {}", path.display().to_string().yellow());
        return Ok(()); // In dry run, we simulate and return success without actual deletion.
    }

    // Check if the path exists. If it doesn't, there's nothing to do, so return Ok immediately.
    if !path.exists() {
        log_debug!("Path does not exist: {}", path.display()); // Log that the path was not found.
        return Ok(()); // Return success as the desired state (path removed) is already met.
    }

    // The following block is commented out in the provided code, but it represents the
    // typical logic for actual file/directory removal based on path type.
    if path.is_file() || path.is_symlink() {
        // If it's a file or a symbolic link, use `fs::remove_file`.
        log_debug!("Path is a file or symlink. Removing: {}", path.display());
        fs::remove_file(path)?; // The `?` operator propagates any `io::Error` that occurs.
    } else if path.is_dir() {
        // If it's a directory, use `fs::remove_dir_all` for recursive deletion.
        log_debug!("Path is a directory. Recursively removing: {}", path.display());
        fs::remove_dir_all(path)?; // Propagates any `io::Error`.
    } else {
        // For other unusual filesystem objects (e.g., block device, char device, fifo, socket),
        // attempt to remove them as if they were files.
        log_debug!("Path is an unusual filesystem object. Removing as file: {}", path.display());
        fs::remove_file(path)?; // Propagates any `io::Error`.
    }
    // Log successful removal for actual deletion, conditioned by `OSX_SHOW_DETAILS` environment variable.
    if std::env::var("OSX_SHOW_DETAILS").is_ok() {
        log_info!("Successfully removed: {}", path.display());
    }
    Ok(()) // Return Ok to indicate that the actual operation completed without an error.
}

/// Recursively calculates the total size of a directory or the size of a file.
///
/// This function walks through the directory tree (if `path` is a directory)
/// and sums up the sizes of all files encountered. It handles symbolic links
/// by getting their metadata directly rather than following them.
///
/// # Arguments
/// * `path` - A reference to a `Path` representing the file or directory whose size is to be calculated.
///
/// # Returns
/// A `io::Result<u64>`:
/// * `Ok(size)`: The total size in bytes if successful.
/// * `Err(error)`: An `std::io::Error` if any I/O operation (like reading metadata or directory entries) fails.
pub fn calculate_dir_size(path: &Path) -> io::Result<u64> {
    // If the path is a file, return its size directly from metadata.
    if path.is_file() {
        return Ok(fs::metadata(path)?.len()); // `len()` returns size in bytes. The `?` propagates `io::Error`.
    }

    let mut size = 0; // Initialize total size accumulator.
    // If the path is a directory, iterate through its contents.
    if path.is_dir() {
        for entry in fs::read_dir(path)? { // Read directory entries, propagating errors with `?`.
            let entry = entry?; // Get the `DirEntry` from `io::Result` (again, `?` for errors).
            let entry_path = entry.path(); // Get the `PathBuf` for the current entry.
            // Get metadata of the symbolic link itself, not the target, to avoid infinite loops with symlinks.
            let metadata = fs::symlink_metadata(&entry_path)?;

            // Recursively call `calculate_dir_size` if the entry is a directory, or add file size directly.
            if metadata.is_dir() {
                size += calculate_dir_size(&entry_path)?; // Recurse for subdirectories.
            } else if metadata.is_file() {
                size += metadata.len(); // Add file size.
            }
            // Other types (symlinks to non-files/dirs, block devices, char devices, fifos, sockets)
            // are currently ignored for size accumulation in this specific logic.
        }
    }
    Ok(size) // Return the total accumulated size.
}

/// Converts a given number of bytes into a human-readable string representation.
///
/// The output is formatted with appropriate units (Bytes, KB, MB, GB) and
/// two decimal places for KB, MB, and GB.
///
/// # Arguments
/// * `bytes` - The number of bytes (u64) to format.
///
/// # Returns
/// A `String` containing the human-readable size (e.g., "10.5 MB", "512 bytes").
pub fn bytes_to_human(bytes: u64) -> String {
    // Define constants for conversion factors as floating-point numbers for accurate division.
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64; // Cast the input `u64` bytes to `f64` for floating-point arithmetic.

    // Use an if-else if ladder to determine the appropriate unit and format the string.
    if b >= GB {
        format!("{:.2} GB", b / GB) // Format to two decimal places for Gigabytes.
    } else if b >= MB {
        format!("{:.2} MB", b / MB) // Format to two decimal places for Megabytes.
    } else if b >= KB {
        format!("{:.2} KB", b / KB) // Format to two decimal places for Kilobytes.
    } else {
        format!("{} bytes", bytes) // For sizes less than a kilobyte, display in raw bytes.
    }
}

/// Helper function to split an aggregated key string into a cleaner name and a path string.
/// This is used when a combined string (e.g., "CleanerName: /path/to/file") needs to be
/// separated for display purposes in tables.
///
/// # Arguments
/// * `key` - A string slice representing the aggregated key, expected in "CleanerName: Path" format.
///
/// # Returns
/// A tuple containing two `String`s: `(cleaner_name, path)`.
/// If the format is unexpected, it returns an empty string for the cleaner name and the original key as the path.
pub fn split_filenames(key: &str) -> (String, String) {
    // Attempt to split the string at the first occurrence of ": ".
    if let Some((name, path_str)) = key.split_once(": ") {
        (name.to_string(), path_str.to_string()) // If successful, return the two parts as Strings.
    } else {
        // Fallback if the expected format is not found.
        // Treat the whole key as the path and an empty string as the cleaner name.
        (String::new(), key.to_string())
    }
}