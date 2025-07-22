use clap::{Parser, Subcommand}; // Import `Parser` and `Subcommand` traits/macros from the `clap` crate.
// `clap` is a popular Rust library for parsing command-line arguments.

/// Command-line interface for the `osx` utility.
///
/// This struct defines the top-level command-line arguments and subcommands
/// for the `osx` application. It uses `clap`'s derive macros for automatic
/// parsing of arguments based on struct fields.
#[derive(Parser)] // Derive the `Parser` trait, which generates the code to parse command-line arguments.
#[command(
    name = "osx", // Sets the name of the executable, which appears in help messages (e.g., `osx --help`).
    about = "🚀 macOS application and system cleaner", // Provides a short description of the application.
    version, // Automatically generates the version string from the Cargo.toml file.
    author = "Your Name <your@email.com>", // Specifies the author information.
    disable_help_subcommand = true // Disables the default `help` subcommand, as `clap` provides `--help` automatically.
)]
pub struct Cli {
    /// Show what would be deleted without deleting
    ///
    /// This field defines a global command-line argument `--dry-run`.
    /// `#[arg(long = "dry-run", global = true)]` configures how this argument is parsed:
    /// - `long = "dry-run"`: Specifies the long form of the argument (e.g., `--dry-run`).
    /// - `global = true`: Makes this argument available for all subcommands (e.g., `osx --dry-run clean-my-mac`).
    #[arg(long = "dry-run", global = true)]
    pub dry_run: bool, // A boolean flag; if present, `dry_run` will be `true`.

    /// Defines the subcommands available for the `osx` utility.
    ///
    /// `#[command(subcommand)]` indicates that this field will hold one of the defined subcommands.
    #[command(subcommand)]
    pub command: Commands, // The `Commands` enum (defined below) will determine which subcommand was invoked.

    /// This field defines a global command-line argument `--debug`.
    #[arg(long, global = true)]
    pub debug: bool, // A boolean flag; if present, `debug` will be `true`.
}

/// Subcommands for the `osx` tool.
///
/// This enum defines the distinct actions that the `osx` utility can perform.
/// Each variant represents a subcommand (e.g., `osx uninstall`, `osx clean-my-mac`).
#[derive(Subcommand)] // Derive the `Subcommand` trait, enabling automatic subcommand parsing.
pub enum Commands {
    /// Uninstall a macOS app or CLI tool
    ///
    /// This variant corresponds to the `uninstall` subcommand.
    Uninstall {
        /// Name of the app or binary to uninstall
        ///
        /// This field captures the positional argument `name` for the `uninstall` subcommand.
        /// When a user types `osx uninstall MyCoolApp`, "MyCoolApp" will be stored in `name`.
        name: String, // The name of the application or tool to be uninstalled.
    },

    /// Clean junk files from system locations
    ///
    /// This variant corresponds to the `clean-my-mac` subcommand.
    CleanMyMac {
        /// List of files/directories to ignore
        ///
        /// This field defines an argument for the `clean-my-mac` subcommand.
        /// `#[arg(long, short, value_delimiter = ',')]` configures it:
        /// - `long = "ignore"`: Specifies the long form (e.g., `--ignore path1`).
        /// - `short = 'i'`: Specifies the short form (e.g., `-i path1`).
        /// - `value_delimiter = ','`: Allows multiple values to be provided separated by commas
        ///                            (e.g., `--ignore /path/to/ignore1,/path/to/ignore2`).
        ignore: Vec<String>, // A vector of strings, where each string is a path to be ignored.
    },
}