//! Configuration management for Nesium
//!
//! Handles loading and saving of user configuration including ROM directories,
//! recent ROMs, favorites, and UI preferences.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// ROM directories to scan
    #[serde(default)]
    pub rom_dirs: Vec<PathBuf>,

    /// Recently played ROMs (paths)
    #[serde(default)]
    pub recent_roms: Vec<PathBuf>,

    /// Favorite ROMs (paths)
    #[serde(default)]
    pub favorites: HashSet<PathBuf>,

    /// UI preferences
    #[serde(default)]
    pub ui: UiConfig,

    /// Artwork preferences
    #[serde(default)]
    pub artwork: ArtworkConfig,

    /// Whether to show the launcher on startup
    #[serde(default = "default_show_launcher")]
    pub show_launcher_on_startup: bool,
}

fn default_show_launcher() -> bool {
    true
}

/// UI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// View mode: "grid" or "list"
    #[serde(default = "default_view_mode")]
    pub view_mode: String,

    /// Grid columns (auto = 0)
    #[serde(default = "default_grid_columns")]
    pub grid_columns: usize,

    /// Sort mode: "name", "recent", "favorite"
    #[serde(default = "default_sort_mode")]
    pub sort_mode: String,

    /// Dark theme
    #[serde(default = "default_dark_theme")]
    pub dark_theme: bool,
}

/// Artwork configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtworkConfig {
    /// Enable online artwork downloading
    #[serde(default)]
    pub enable_online: bool,

    /// Preferred artwork type: "box", "cartridge", "screenshot", "title"
    #[serde(default = "default_artwork_type")]
    pub preferred_type: String,

    /// Auto-download artwork during scan
    #[serde(default)]
    pub auto_download: bool,
}

fn default_view_mode() -> String {
    "grid".to_string()
}

fn default_grid_columns() -> usize {
    0 // Auto
}

fn default_sort_mode() -> String {
    "name".to_string()
}

fn default_dark_theme() -> bool {
    true
}

fn default_artwork_type() -> String {
    "cartridge".to_string()
}

impl Default for ArtworkConfig {
    fn default() -> Self {
        Self {
            enable_online: false, // Opt-in for privacy
            preferred_type: default_artwork_type(),
            auto_download: false,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            view_mode: default_view_mode(),
            grid_columns: default_grid_columns(),
            sort_mode: default_sort_mode(),
            dark_theme: default_dark_theme(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rom_dirs: Vec::new(),
            recent_roms: Vec::new(),
            favorites: HashSet::new(),
            ui: UiConfig::default(),
            artwork: ArtworkConfig::default(),
            show_launcher_on_startup: true,
        }
    }
}

impl Config {
    /// Get the configuration file path
    pub fn config_path() -> PathBuf {
        if let Some(config_dir) = dirs::config_dir() {
            let nesium_dir = config_dir.join("nesium");
            fs::create_dir_all(&nesium_dir).ok();
            nesium_dir.join("config.toml")
        } else {
            PathBuf::from("config.toml")
        }
    }

    /// Load configuration from file
    pub fn load() -> Self {
        let path = Self::config_path();

        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(contents) => match toml::from_str(&contents) {
                    Ok(config) => {
                        log::info!("Loaded configuration from: {}", path.display());
                        return config;
                    }
                    Err(e) => {
                        log::error!("Failed to parse config file: {}", e);
                    }
                },
                Err(e) => {
                    log::error!("Failed to read config file: {}", e);
                }
            }
        }

        log::info!("Using default configuration");
        Self::default()
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::config_path();
        let contents = toml::to_string_pretty(self)?;
        fs::write(&path, contents)?;
        log::info!("Saved configuration to: {}", path.display());
        Ok(())
    }

    /// Add a ROM directory
    pub fn add_rom_dir(&mut self, dir: PathBuf) {
        if !self.rom_dirs.contains(&dir) {
            self.rom_dirs.push(dir);
        }
    }

    /// Remove a ROM directory
    pub fn remove_rom_dir(&mut self, dir: &Path) {
        self.rom_dirs.retain(|d| d != dir);
    }

    /// Add to recent ROMs (maintains max 20)
    pub fn add_recent(&mut self, rom_path: PathBuf) {
        // Remove if already exists
        self.recent_roms.retain(|p| p != &rom_path);

        // Add to front
        self.recent_roms.insert(0, rom_path);

        // Keep only the most recent 20
        self.recent_roms.truncate(20);
    }

    /// Toggle favorite status
    pub fn toggle_favorite(&mut self, rom_path: PathBuf) -> bool {
        if self.favorites.contains(&rom_path) {
            self.favorites.remove(&rom_path);
            false
        } else {
            self.favorites.insert(rom_path);
            true
        }
    }

    /// Check if a ROM is favorited
    pub fn is_favorite(&self, rom_path: &Path) -> bool {
        self.favorites.contains(rom_path)
    }
}
