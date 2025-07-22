use crate::log_debug; // Imports the `log_debug` macro for logging debug-level messages.
use colored::Colorize; // Imports the `Colorize` trait from the `colored` crate, used for adding color to terminal output strings.
use std::fs; // Imports the standard library's file system module for operations like deleting files/directories, reading metadata, etc.
use std::io; // Imports the standard library's I/O module, primarily for `io::Result` and `io::Error`.
use std::path::Path; // Imports `Path` from the standard library, a universal type for file system paths.

/// Recursively deletes a file or directory at the given path.
///
/// This function attempts to remove the specified file system entry.
/// It distinguishes between files/symlinks and directories to use the appropriate deletion method.
///
/// If the path does not exist, this function returns `Ok(())` immediately, as there's nothing to delete.
///
/// # Arguments
/// * `path` - A reference to a `Path` indicating the file or directory to be removed.
///
/// # Errors
///
/// Returns an `io::Error` if the removal of the file or directory fails for any reason
/// (e.g., permission denied, path is locked, disk error).
///
/// # Behavior
///
/// - If the `path` points to a regular file or a symbolic link, it uses `fs::remove_file()`.
/// - If the `path` points to a directory, it recursively deletes the directory and all its contents
///   using `fs::remove_dir_all()`.
/// - If the `path` points to some other filesystem object type (like a socket or a device file),
///   it attempts to delete it as a file using `fs::remove_file()`.
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use osx::utils::filesystem::remove_path;
/// // Attempt to remove a file named "somefile" in the /tmp directory.
/// remove_path(Path::new("/tmp/somefile")).expect("Failed to remove path");
/// ```
pub fn remove_path(path: &Path) -> io::Result<()> {
    // Log the attempt to remove the path at debug level, with the path colored blue.
    log_debug!("Attempting to remove path: {}", path.display().to_string().blue());

    // Check if the path exists. If it doesn't, there's nothing to do, so return Ok immediately.
    if !path.exists() {
        log_debug!("Path does not exist: {}", path.display()); // Log that the path was not found.
        return Ok(()); // Return success as the desired state (path removed) is already met.
    }

    // Determine the type of file system object and call the appropriate removal function.
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

    // Log successful removal.
    log_debug!("Successfully removed: {}", path.display());
    Ok(()) // Return Ok to indicate that the operation completed without an error.
}

/// Recursively calculates the total size of a directory or the size of a file.
///
/// This function walks through the directory tree (if `path` is a directory)
/// and sums up the sizes of all files encountered.
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
        return Ok(fs::metadata(path)?.len()); // `len()` returns size in bytes.
    }

    let mut size = 0; // Initialize total size.
    // If the path is a directory, iterate through its contents.
    if path.is_dir() {
        for entry in fs::read_dir(path)? { // Read directory entries, propagating errors with `?`.
            let entry = entry?; // Get the `DirEntry` (again, `?` for errors).
            let entry_path = entry.path(); // Get the `PathBuf` for the current entry.
            let metadata = fs::symlink_metadata(&entry_path)?; // Get metadata, handling symlinks directly.

            // Recursively call `calculate_dir_size` if the entry is a directory, or add file size directly.
            if metadata.is_dir() {
                size += calculate_dir_size(&entry_path)?; // Recurse for subdirectories.
            } else if metadata.is_file() {
                size += metadata.len(); // Add file size.
            }
            // Other types (symlinks to non-files/dirs, etc.) are currently ignored for size accumulation.
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
    // Define constants for conversion factors as floating-point numbers for division.
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64; // Cast the input bytes to f64 for floating-point arithmetic.

    // Use an if-else if ladder to determine the appropriate unit and format the string.
    if b >= GB {
        format!("{:.2} GB", b / GB) // Format to two decimal places for Gigabytes.
    } else if b >= MB {
        format!("{:.2} MB", b / MB) // Format to two decimal places for Megabytes.
    } else if b >= KB {
        format!("{:.2} KB", b / KB) // Format to two decimal places for Kilobytes.
    } else {
        format!("{} bytes", bytes) // For less than a kilobyte, display in raw bytes.
    }
}