# 🧼 OSX Cleaner (CLI Edition)

![Rust Logo](https://img.shields.io/badge/Rust-red?style=for-the-badge&logo=rust)
![Platform](https://img.shields.io/badge/Platform-macOS-blue?style=for-the-badge&logo=apple)


[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![CI](https://github.com/kodelint/osx-cleaner/actions/workflows/release.yml/badge.svg)](https://github.com/kodelint/osx-cleaner/actions/workflows/release.yml)
[![GitHub release](https://img.shields.io/github/release/kodelint/osx-cleaner.svg)](https://github.com/kodelint/osx-cleaner/releases)
[![GitHub stars](https://img.shields.io/github/stars/kodelint/osx-cleaner.svg)](https://github.com/kodelint/osx-cleaner/stargazers)
[![Last commit](https://img.shields.io/github/last-commit/kodelint/osx-cleaner.svg)](https://github.com/kodelint/osx-cleaner/commits/main)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/kodelint/osx-cleaner/pulls)

<p align="center">
  <img src="https://raw.githubusercontent.com/kodelint/blog-images/main/common/01-osx-cleaner.png" alt="osx-cleaner" width="500"/>
</p>

A blazing-fast, terminal-based macOS cleanup and uninstaller utility written in Rust.
`osx` CLI is designed for developers and power users who want complete control over system hygiene 
without the bloat of GUI tools. Whether you're cleaning caches to free up space or performing a 
surgical uninstallation of applications, this tool gives you precision, performance, and full visibility into what’s happening under the hood.
Built natively for macOS and optimized with parallelism and tabled summaries, it helps you:

- Clear system and app junk (logs, caches, temp files, trash)
- Perform dry-runs to preview what will be cleaned or removed
- Fully uninstall GUI or CLI apps by deleting related config, cache, and support files
- Skip SIP-protected or system-critical paths safely

Unlike traditional tools that hide actions behind buttons, **this CLI shows you everything** every path, every byte, every skipped file, so you stay in control.

---

## ✨ Features

- ✅ Clean common macOS junk (logs, caches, trash)
- ✅ Fully uninstall apps and CLIs with all related files
- ✅ Supports dry-run mode for safe inspection
- ✅ Skips SIP-protected and system-critical paths
- ✅ Parallel file size computation using `rayon`
- ✅ Summarized tabular reports for both success and errors
- ✅ Fast, safe, and built natively for macOS

---

## 🚀 Installation

```sh
cargo install --path .
```
Or clone and build

```bash
git clone https://github.com/kodelint/osx-cleaner.git
cd osx
cargo build --release
```

## 🔧 Usage
```bash
>> osx -h
🚀 macOS application and system cleaner

Usage: osx [OPTIONS] [DEBUG] <COMMAND>

Commands:
  uninstall     Uninstall a macOS app or CLI tool
  clean-my-mac  Clean junk files from system locations

Arguments:
  [DEBUG]  # 

Options:
      --dry-run  Show what would be deleted without deleting
  -h, --help     Print help
  -V, --version  Print version
```
### Available Commands
| Command        | Description                        |
|----------------|------------------------------------|
| `clean-my-mac` | Clean junk files from macOS system |
| `uninstall`    | Uninstall a macOS app or CLI tool  |

### Global Options
| Flag            | Description                                 |
|-----------------|---------------------------------------------|
| `--dry-run`     | Show what would be deleted without deleting |
| `-h, --help`    | Show help and usage                         |
| `-V, --version` | Print version info                          |



## 🧹 clean-my-mac – System Junk Cleaner
You’ll see summary tables like this:
```bash
>> osx --dry-run clean-my-mac

[INFO] Logger initialized in INFO mode
[INFO] 🧹 Cleaning up your Mac...
[INFO] 🔍 Checking: /var/folders
[INFO] 🔍 Checking: /System/Library/Caches
[INFO] 🔍 Checking: /private/var/folders
[INFO] 🔍 Checking: /Library/Caches
[INFO] 🔍 Checking: /Users/kodelint/Library/Caches
[INFO] 🔍 Checking: /Users/kodelint/Library/Logs
[INFO] 🔍 Checking: /Users/kodelint/Library/Application Support/CrashReporter
[INFO] 🔍 Checking: /Users/kodelint/.Trash
[INFO] 🧾 Would clean: /System/Library/Caches (497.34 MB)
[INFO] 🧾 Would clean: /Library/Caches (448.65 MB)
[INFO] 🧾 Would clean: /Users/kodelint/Library/Logs (18.98 MB)
[INFO] 🧾 Would clean: /var/folders (916.33 MB)
[INFO] 🧾 Would clean: /Users/kodelint/Library/Caches (590.71 MB)
[INFO] 🧾 Would clean: /private/var/folders (916.33 MB)

🧾 Cleanup Summary (Successful)
┌────────────────────────────────┬───────────┐
│ Path                           │ Size      │
├────────────────────────────────┼───────────┤
│ /System/Library/Caches         │ 497.34 MB │
├────────────────────────────────┼───────────┤
│ /Library/Caches                │ 448.65 MB │
├────────────────────────────────┼───────────┤
│ /Users/kodelint/Library/Logs   │ 18.98 MB  │
├────────────────────────────────┼───────────┤
│ /var/folders                   │ 916.33 MB │
├────────────────────────────────┼───────────┤
│ /Users/kodelint/Library/Caches │ 590.71 MB │
├────────────────────────────────┼───────────┤
│ /private/var/folders           │ 916.33 MB │
├────────────────────────────────┼───────────┤
│ Total                          │ 3.31 GB   │
└────────────────────────────────┴───────────┘

[INFO] 🧠 Estimated space to free: 3.31 GB
[WARN] ⚠️  System Integrity Protection (SIP) is enabled. Some files may not be removable.
[INFO] Clean-up completed successfully.
```

## 📂 Cleanup Targets
The tool automatically finds and cleans the following:

#### System & user cache folders

* Log folders
* User Trash
* `/var/folders`, `/private/var/folders`
* `/Volumes/*/.Trashes`

#### It never deletes:

* `/private/tmp`
* `$TMPDIR`
* Any SIP-protected location (if SIP is on)

## 🧽 uninstall – Full App Uninstaller
```bash
osx uninstall Docker
```
This command locates and deletes all known traces of a macOS app, including:

* Application bundle (`/Applications/*.app`)
* User configuration (`~/Library/Preferences`)
* Caches and logs
* LaunchAgents, LoginItems, CLI symlinks

```bash
# Dry run only
osx --dry-run uninstall zoom.us
# Full uninstall
osx uninstall slack
```
Supports both GUI apps (.app) and CLI tools installed via Homebrew or symlinked to `/usr/local/bin`, `/opt/homebrew/bin`, etc.

## 🛡️ System Integrity Protection (SIP)
If SIP is enabled, certain system paths like `/System/Library/Caches` cannot be modified. The tool detects and 
gracefully skips these locations, logging warnings as needed.

## 👨‍💻 Developer Notes
* Uses rayon for parallel processing
* Uses tabled for pretty table formatting
* Uses glob to match mounted drive trashes
* Respects macOS environment variables (e.g., `$HOME`, `$TMPDIR`)
