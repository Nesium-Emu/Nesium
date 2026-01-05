//! Settings persistence and configuration for Nesium

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;

/// Maximum number of recent ROMs to remember
const MAX_RECENT_ROMS: usize = 10;

/// Key bindings for NES controller
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyBindings {
    pub a: egui::Key,
    pub b: egui::Key,
    pub select: egui::Key,
    pub start: egui::Key,
    pub up: egui::Key,
    pub down: egui::Key,
    pub left: egui::Key,
    pub right: egui::Key,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            a: egui::Key::A,
            b: egui::Key::S,
            select: egui::Key::Tab,
            start: egui::Key::Enter,
            up: egui::Key::ArrowUp,
            down: egui::Key::ArrowDown,
            left: egui::Key::ArrowLeft,
            right: egui::Key::ArrowRight,
        }
    }
}

/// Video settings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VideoSettings {
    pub scale: u32,
    pub fullscreen: bool,
    pub integer_scaling: bool,
    pub show_fps: bool,
    pub vsync: bool,
    pub crt_effect: bool,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            scale: 3,
            fullscreen: false,
            integer_scaling: true,
            show_fps: true,
            vsync: true,
            crt_effect: false,
        }
    }
}

/// Audio settings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioSettings {
    pub volume: f32,
    pub muted: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            volume: 0.7,
            muted: false,
        }
    }
}

/// Emulation settings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmulationSettings {
    pub speed_multiplier: f32,
    pub rewind_enabled: bool,
}

impl Default for EmulationSettings {
    fn default() -> Self {
        Self {
            speed_multiplier: 1.0,
            rewind_enabled: false,
        }
    }
}

/// UI Theme
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    Catppuccin,
    Nord,
}

/// Application settings with persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: Theme,
    pub video: VideoSettings,
    pub audio: AudioSettings,
    pub emulation: EmulationSettings,
    pub key_bindings: KeyBindings,
    pub recent_roms: VecDeque<PathBuf>,
    pub last_rom_directory: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            video: VideoSettings::default(),
            audio: AudioSettings::default(),
            emulation: EmulationSettings::default(),
            key_bindings: KeyBindings::default(),
            recent_roms: VecDeque::new(),
            last_rom_directory: None,
        }
    }
}

impl Settings {
    /// Get the settings file path
    pub fn settings_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("nesium").join("settings.json"))
    }

    /// Load settings from disk, or return defaults
    pub fn load() -> Self {
        Self::settings_path()
            .and_then(|path| std::fs::read_to_string(&path).ok())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Save settings to disk
    pub fn save(&self) {
        if let Some(path) = Self::settings_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, json);
            }
        }
    }

    /// Add a ROM to the recent list
    pub fn add_recent_rom(&mut self, path: PathBuf) {
        // Remove if already present
        self.recent_roms.retain(|p| p != &path);
        // Add to front
        self.recent_roms.push_front(path.clone());
        // Trim to max size
        while self.recent_roms.len() > MAX_RECENT_ROMS {
            self.recent_roms.pop_back();
        }
        // Update last directory
        if let Some(parent) = path.parent() {
            self.last_rom_directory = Some(parent.to_path_buf());
        }
        self.save();
    }

    /// Apply the current theme to egui
    pub fn apply_theme(&self, ctx: &egui::Context) {
        match self.theme {
            Theme::Dark => {
                ctx.set_visuals(Self::dark_theme());
            }
            Theme::Light => {
                ctx.set_visuals(Self::light_theme());
            }
            Theme::Catppuccin => {
                ctx.set_visuals(Self::catppuccin_theme());
            }
            Theme::Nord => {
                ctx.set_visuals(Self::nord_theme());
            }
        }
    }

    fn dark_theme() -> egui::Visuals {
        let mut visuals = egui::Visuals::dark();
        
        // Deep charcoal background
        visuals.panel_fill = egui::Color32::from_rgb(18, 18, 24);
        visuals.window_fill = egui::Color32::from_rgb(24, 24, 32);
        visuals.extreme_bg_color = egui::Color32::from_rgb(12, 12, 16);
        
        // Accent colors - electric blue
        visuals.selection.bg_fill = egui::Color32::from_rgb(66, 135, 245);
        visuals.hyperlink_color = egui::Color32::from_rgb(100, 180, 255);
        
        // Widget colors
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(35, 35, 45);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 50, 65);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(66, 135, 245);
        
        // Rounded corners via widget rounding
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
        
        visuals
    }

    fn light_theme() -> egui::Visuals {
        let mut visuals = egui::Visuals::light();
        
        // Warm white background
        visuals.panel_fill = egui::Color32::from_rgb(252, 250, 248);
        visuals.window_fill = egui::Color32::from_rgb(255, 255, 255);
        visuals.extreme_bg_color = egui::Color32::from_rgb(245, 243, 240);
        
        // Accent - warm coral
        visuals.selection.bg_fill = egui::Color32::from_rgb(230, 100, 90);
        visuals.hyperlink_color = egui::Color32::from_rgb(200, 80, 70);
        
        // Rounded corners
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
        
        visuals
    }

    fn catppuccin_theme() -> egui::Visuals {
        let mut visuals = egui::Visuals::dark();
        
        // Catppuccin Mocha colors
        let base = egui::Color32::from_rgb(30, 30, 46);
        let mantle = egui::Color32::from_rgb(24, 24, 37);
        let crust = egui::Color32::from_rgb(17, 17, 27);
        let surface0 = egui::Color32::from_rgb(49, 50, 68);
        let surface1 = egui::Color32::from_rgb(69, 71, 90);
        let mauve = egui::Color32::from_rgb(203, 166, 247);
        let pink = egui::Color32::from_rgb(245, 194, 231);
        
        visuals.panel_fill = base;
        visuals.window_fill = mantle;
        visuals.extreme_bg_color = crust;
        
        visuals.selection.bg_fill = mauve;
        visuals.hyperlink_color = pink;
        
        visuals.widgets.inactive.bg_fill = surface0;
        visuals.widgets.hovered.bg_fill = surface1;
        visuals.widgets.active.bg_fill = mauve;
        
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(10);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
        
        visuals
    }

    fn nord_theme() -> egui::Visuals {
        let mut visuals = egui::Visuals::dark();
        
        // Nord colors - polar night
        let nord0 = egui::Color32::from_rgb(46, 52, 64);
        let nord1 = egui::Color32::from_rgb(59, 66, 82);
        let nord2 = egui::Color32::from_rgb(67, 76, 94);
        let nord3 = egui::Color32::from_rgb(76, 86, 106);
        // Frost
        let nord8 = egui::Color32::from_rgb(136, 192, 208);
        let nord9 = egui::Color32::from_rgb(129, 161, 193);
        
        visuals.panel_fill = nord0;
        visuals.window_fill = nord1;
        visuals.extreme_bg_color = egui::Color32::from_rgb(36, 40, 50);
        
        visuals.selection.bg_fill = nord8;
        visuals.hyperlink_color = nord9;
        
        visuals.widgets.inactive.bg_fill = nord2;
        visuals.widgets.hovered.bg_fill = nord3;
        visuals.widgets.active.bg_fill = nord8;
        
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
        
        visuals
    }
}
