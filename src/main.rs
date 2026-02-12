//! Nesium - A high-accuracy NES emulator with a modern UI
//!
//! This is the main entry point for the Nesium emulator.
//! It supports both GUI mode (default) and CLI mode for testing.

// Core emulation modules from the library crate
// Re-exported so `crate::cpu`, `crate::ppu`, etc. still work in local modules
pub use nesium::apu;
pub use nesium::cartridge;
pub use nesium::cpu;
pub use nesium::input;
pub use nesium::memory;
pub use nesium::ppu;
pub use nesium::trace;

// Desktop-only modules
mod artwork_scraper;
mod config;
mod rom_browser;
mod ui;

use clap::Parser;
use std::path::PathBuf;
use std::fs;

#[derive(Parser)]
#[command(name = "nesium")]
#[command(about = "A high-accuracy NES emulator with a modern UI")]
struct Args {
    /// Path to a NES ROM file to load immediately (optional)
    #[arg(value_name = "ROM", required = false)]
    rom_path: Option<String>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

fn main() -> eframe::Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    env_logger::Builder::from_default_env()
        .filter_level(log_level)
        .format_timestamp_millis()
        .init();

    log::info!("Starting Nesium v{}", env!("CARGO_PKG_VERSION"));

    // Configure native options
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Nesium - NES Emulator")
            .with_inner_size([900.0, 700.0])
            .with_min_inner_size([640.0, 480.0])
            .with_icon(load_icon()),
        vsync: true,
        ..Default::default()
    };

    // Create the app with optional ROM path - convert to absolute path
    let rom_to_load = args.rom_path.and_then(|p| {
        let path = PathBuf::from(&p);
        // Try to canonicalize (make absolute), fallback to original if it fails
        match path.canonicalize() {
            Ok(abs_path) => {
                log::info!("ROM path resolved to: {}", abs_path.display());
                Some(abs_path)
            }
            Err(e) => {
                log::error!("Failed to resolve ROM path '{}': {}", p, e);
                None
            }
        }
    });

    eframe::run_native(
        "Nesium",
        native_options,
        Box::new(move |cc| {
            // Set up custom fonts
            setup_fonts(&cc.egui_ctx);
            
            // Create the app with optional ROM to load
            let app = ui::NesiumApp::with_rom(rom_to_load);
            
            Ok(Box::new(app))
        }),
    )
}

/// Load the application icon from ICO/PNG file or generate fallback
fn load_icon() -> egui::IconData {
    // Try to load from ICO or PNG file first (preferred)
    if let Ok(icon) = load_icon_from_file() {
        return icon;
    }
    
    // Fallback to programmatically generated icon
    generate_icon_fallback()
}

/// Try to load icon from ICO or PNG file in resources directory
fn load_icon_from_file() -> Result<egui::IconData, Box<dyn std::error::Error>> {
    // Try multiple possible paths, prioritizing the official ICO file
    let paths = [
        "resources/NESIUM.ico",
        "resources/nesium-icon.png",
        "resources/nesium-logo-simple.png",
        "../resources/NESIUM.ico",
        "../resources/nesium-icon.png",
        "./resources/NESIUM.ico",
        "./resources/nesium-icon.png",
    ];
    
    for path in &paths {
        if let Ok(data) = fs::read(path) {
            if let Ok(img) = image::load_from_memory(&data) {
                let rgba = img.to_rgba8();
                let (width, height) = rgba.dimensions();
                let pixels = rgba.into_raw();
                
                log::info!("Loaded icon from: {}", path);
                return Ok(egui::IconData {
                    rgba: pixels,
                    width,
                    height,
                });
            }
        }
    }
    
    Err("Icon file not found".into())
}

/// Generate fallback icon programmatically (based on logo design)
fn generate_icon_fallback() -> egui::IconData {
    let size = 64; // Higher resolution for better quality
    let mut rgba = vec![0u8; size * size * 4];
    
    // Draw icon based on logo design - NES controller with NESIUM styling
    for y in 0..size {
        for x in 0..size {
            let i = (y * size + x) * 4;
            
            // Background gradient (dark blue-gray)
            let bg_factor = (x as f32 / size as f32) * 0.3 + 0.7;
            let (r, g, b, a) = if x >= 8 && x < size - 8 && y >= 16 && y < size - 16 {
                // Controller body area
                if x >= 12 && x < size - 12 && y >= 20 && y < size - 20 {
                    // Inner body
                    (42, 42, 52, 255)
                } else {
                    // Border (NES blue)
                    (26, 26, 46, 255)
                }
            } else if x >= 20 && x < 36 && y >= 24 && y < 40 {
                // D-pad area
                let dpad_x = x - 20;
                let dpad_y = y - 24;
                if (dpad_x >= 6 && dpad_x < 10 && dpad_y < 16) || // Up
                   (dpad_x >= 6 && dpad_x < 10 && dpad_y >= 12 && dpad_y < 16) || // Down
                   (dpad_x < 4 && dpad_y >= 6 && dpad_y < 10) || // Left
                   (dpad_x >= 12 && dpad_x < 16 && dpad_y >= 6 && dpad_y < 10) { // Right
                    (100, 180, 255, 255) // NES blue
                } else if dpad_x >= 4 && dpad_x < 12 && dpad_y >= 4 && dpad_y < 12 {
                    (100, 180, 255, 200) // Center (lighter)
                } else {
                    (0, 0, 0, 0) // Transparent
                }
            } else if (x - 48).pow(2) + (y - 28).pow(2) <= 64 {
                // A button (red circle)
                let dist = ((x - 48) as f32).powi(2) + ((y - 28) as f32).powi(2);
                if dist <= 36.0 {
                    (255, 100, 100, 255) // Red
                } else if dist <= 64.0 {
                    (255, 120, 120, 200) // Lighter red border
                } else {
                    (0, 0, 0, 0)
                }
            } else if (x - 56).pow(2) + (y - 36).pow(2) <= 64 {
                // B button (orange circle)
                let dist = ((x - 56) as f32).powi(2) + ((y - 36) as f32).powi(2);
                if dist <= 36.0 {
                    (255, 200, 100, 255) // Orange
                } else if dist <= 64.0 {
                    (255, 220, 120, 200) // Lighter orange border
                } else {
                    (0, 0, 0, 0)
                }
            } else {
                // Background gradient
                let r = (26.0 * bg_factor) as u8;
                let g = (30.0 * bg_factor) as u8;
                let b = (46.0 * bg_factor) as u8;
                (r, g, b, 255)
            };
            
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = a;
        }
    }
    
    log::debug!("Generated fallback icon programmatically");
    egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}

/// Set up custom fonts for better typography
fn setup_fonts(ctx: &egui::Context) {
    let fonts = egui::FontDefinitions::default();
    
    // Configure font sizes for a more modern look
    let mut style = (*ctx.style()).clone();
    
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::new(24.0, egui::FontFamily::Proportional)),
        (egui::TextStyle::Body, egui::FontId::new(14.0, egui::FontFamily::Proportional)),
        (egui::TextStyle::Monospace, egui::FontId::new(14.0, egui::FontFamily::Monospace)),
        (egui::TextStyle::Button, egui::FontId::new(14.0, egui::FontFamily::Proportional)),
        (egui::TextStyle::Small, egui::FontId::new(12.0, egui::FontFamily::Proportional)),
    ].into();
    
    // Spacing tweaks
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    
    ctx.set_style(style);
    ctx.set_fonts(fonts);
}
