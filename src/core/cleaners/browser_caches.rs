use crate::core::cleaners::Cleaner;
use colored::Colorize;
use std::{
    path::PathBuf,
    env,
};
use glob::glob;
use crate::{log_debug, log_warn};

/// Represents a cleaner for various browser caches.
pub struct BrowserCachesCleaner;

impl BrowserCachesCleaner {
    pub fn new() -> Self {
        BrowserCachesCleaner
    }
}

impl Cleaner for BrowserCachesCleaner {
    fn name(&self) -> &str {
        "Browser Caches"
    }

    fn find_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let home_dir = match env::home_dir() {
            Some(path) => path,
            None => {
                log_warn!("Could not find home directory, skipping browser cache scan.");
                return paths;
            }
        };

        // --- Chrome Caches ---
        // These patterns now target the cache *directories* themselves, not their contents.
        let chrome_cache_patterns = vec![
            format!("{}/Library/Caches/Google/Chrome/Default/Cache", home_dir.display()),
            format!("{}/Library/Application Support/Google/Chrome/Default/Cache", home_dir.display()),
            format!("{}/Library/Application Support/Google/Chrome/Default/Code Cache", home_dir.display()),
            format!("{}/Library/Application Support/Google/Chrome/Default/Service Worker/CacheStorage", home_dir.display()),
            // Generic patterns for other profiles (e.g., Profile 1, Profile 2)
            format!("{}/Library/Caches/Google/Chrome/*/Cache", home_dir.display()),
            format!("{}/Library/Application Support/Google/Chrome/*/Cache", home_dir.display()),
            format!("{}/Library/Application Support/Google/Chrome/*/Code Cache", home_dir.display()),
            format!("{}/Library/Application Support/Google/Chrome/*/Service Worker/CacheStorage", home_dir.display()),
        ];

        for pattern_str in chrome_cache_patterns {
            if let Ok(entries) = glob(&pattern_str) {
                for entry in entries.filter_map(|e| e.ok()) {
                    // Only add if it's a directory (or a symlink to one)
                    if entry.is_dir() {
                        log_debug!("Found Chrome cache directory: {}", entry.display());
                        paths.push(entry);
                    } else {
                        log_debug!("Skipping non-directory Chrome cache entry: {}", entry.display());
                    }
                }
            } else {
                log_warn!("Invalid glob pattern for Chrome: {}", pattern_str);
            }
        }

        // --- Firefox Caches ---
        // Targets the main cache location within Firefox profiles.
        let firefox_cache_pattern = format!("{}/Library/Caches/Firefox/Profiles/*/cache2", home_dir.display()); // 'cache2' is the directory holding 'entries'
        if let Ok(entries) = glob(&firefox_cache_pattern) {
            for entry in entries.filter_map(|e| e.ok()) {
                if entry.is_dir() {
                    log_debug!("Found Firefox cache directory: {}", entry.display());
                    paths.push(entry);
                } else {
                    log_debug!("Skipping non-directory Firefox cache entry: {}", entry.display());
                }
            }
        } else {
            log_warn!("Invalid glob pattern for Firefox: {}", firefox_cache_pattern);
        }

        // --- Brave Caches (similar to Chrome as it's Chromium-based) ---
        let brave_cache_patterns = vec![
            format!("{}/Library/Caches/BraveSoftware/Brave-Browser/Default/Cache", home_dir.display()),
            format!("{}/Library/Application Support/BraveSoftware/Brave-Browser/Default/Cache", home_dir.display()),
            format!("{}/Library/Application Support/BraveSoftware/Brave-Browser/Default/Code Cache", home_dir.display()),
            // Generic patterns for other profiles
            format!("{}/Library/Caches/BraveSoftware/Brave-Browser/*/Cache", home_dir.display()),
            format!("{}/Library/Application Support/BraveSoftware/Brave-Browser/*/Cache", home_dir.display()),
            format!("{}/Library/Application Support/BraveSoftware/Brave-Browser/*/Code Cache", home_dir.display()),
        ];

        for pattern_str in brave_cache_patterns {
            if let Ok(entries) = glob(&pattern_str) {
                for entry in entries.filter_map(|e| e.ok()) {
                    if entry.is_dir() {
                        log_debug!("Found Brave cache directory: {}", entry.display());
                        paths.push(entry);
                    } else {
                        log_debug!("Skipping non-directory Brave cache entry: {}", entry.display());
                    }
                }
            } else {
                log_warn!("Invalid glob pattern for Brave: {}", pattern_str);
            }
        }

        paths
    }
}