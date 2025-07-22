use clap::Parser; // Import the `Parser` trait from the `clap` crate, used for parsing command-line arguments.
use colored::Colorize; // Import the `Colorize` trait, which allows adding ANSI color codes to strings for terminal output.
use osx::cli::commands::{Cli, Commands}; // Import the `Cli` struct and `Commands` enum from the `commands` module, which define the CLI structure.
use osx::core::cleaner::clean_my_mac; // Import the `clean_my_mac` function from the `cleaner` module, responsible for system cleanup.
use osx::core::uninstaller::{CliTool, MacApp, Uninstaller}; // Import `CliTool`, `MacApp` structs, and the `Uninstaller` trait from the `uninstaller` module.
use osx::{log_debug, log_error, log_info, log_warn, logger}; // Import custom logging macros and the `logger` initialization function.

/// The main entry point of the `osx` application.
///
/// This function is responsible for:
/// 1. Parsing command-line arguments.
/// 2. Initializing the logger based on the debug flag.
/// 3. Determining which subcommand was invoked (`Uninstall` or `CleanMyMac`).
/// 4. Executing the appropriate logic based on the subcommand.
fn main() {
    let cli = Cli::parse(); // Parse the command-line arguments into the `Cli` struct.
    // `Cli::parse()` uses the `#[derive(Parser)]` macro to automatically
    // interpret the arguments provided by the user.

    // Initialize the logger based on the `debug` flag from the parsed CLI arguments.
    // If `cli.debug` is true, the logger will show debug-level messages; otherwise, it will show info/warn/error.
    logger::init(cli.debug);

    let dry_run = cli.dry_run; // Extract the `dry_run` flag from the parsed CLI arguments.
    // This flag determines if operations should only be simulated or actually performed.

    // Log the initial state of the `dry_run` flag at debug level.
    log_debug!("Starting with dry_run = {}", dry_run.to_string().bright_blue());

    // Use a `match` expression to handle the different subcommands defined in the `Commands` enum.
    match &cli.command { // `&cli.command` takes a reference to the `command` field of the `Cli` struct.
        Commands::Uninstall { name } => { // If the `uninstall` subcommand was invoked, bind its `name` argument.
            log_info!("🔧 Attempting to uninstall '{}'", name.bright_green()); // Inform the user about the uninstall attempt.

            let app = MacApp::new(name); // Create a `MacApp` uninstaller instance for the given name.
            let cli_tool = CliTool::new(name); // Create a `CliTool` uninstaller instance for the given name.

            // Attempt to uninstall the application (GUI app paths).
            // The `Uninstaller` trait's `uninstall` method is called.
            if let Err(e) = app.uninstall(dry_run) {
                // If uninstallation of the Mac app fails, log a warning with the error.
                log_warn!("Failed to uninstall app '{}': {}", name.bright_yellow(), e.to_string().bright_white());
            } else {
                // If uninstallation of the Mac app succeeds, log a success message.
                log_info!("Successfully uninstalled app '{}'", name.bright_green());
            }

            // Attempt to uninstall the command-line tool (CLI tool paths).
            // This is done separately as a name might correspond to both an app and a CLI tool.
            if let Err(e) = cli_tool.uninstall(dry_run) {
                // If uninstallation of the CLI tool fails, log a warning with the error.
                log_warn!("Failed to uninstall CLI tool '{}': {}", name.bright_yellow(), e.to_string().bright_white());
            } else {
                // If uninstallation of the CLI tool succeeds, log a success message.
                log_info!("Successfully uninstalled CLI tool '{}'", name.bright_green());
            }
        }

        Commands::CleanMyMac { ignore } => { // If the `clean-my-mac` subcommand was invoked, bind its `ignore` argument.
            log_info!("🧹 Cleaning up your Mac..."); // Inform the user about the cleanup process initiation.

            // Call the `clean_my_mac` function from the `cleaner` module.
            // It takes the `dry_run` flag and a clone of the `ignore` vector.
            if let Err(e) = clean_my_mac(dry_run, ignore.clone()) {
                // If the cleanup process fails, log an error message with the details.
                log_error!("{}: {}", "Clean-up failed".bright_yellow(), e.to_string().bright_red());
            } else {
                // If the cleanup process completes successfully, log a success message.
                log_info!("{}", "Clean-up completed successfully.".bright_white());
            }
        }
    }

    log_debug!("Finished execution."); // Log that the program has finished its execution, regardless of subcommand success.
}