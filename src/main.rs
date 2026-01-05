//! Nesium - A high-accuracy NES emulator with a modern UI
//!
//! This is the main entry point for the Nesium emulator.
//! It supports both GUI mode (default) and CLI mode for testing.

mod apu;
mod cartridge;
mod cpu;
mod input;
mod memory;
mod ppu;
mod trace;
mod ui;

use clap::Parser;
use std::path::PathBuf;

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

/// Load the application icon
fn load_icon() -> egui::IconData {
    // Simple 32x32 NES controller-inspired icon
    let size = 32;
    let mut rgba = vec![0u8; size * size * 4];
    
    // Draw a simple pixel art controller icon
    for y in 0..size {
        for x in 0..size {
            let i = (y * size + x) * 4;
            
            // Background - dark
            let (r, g, b, a) = if x >= 4 && x < 28 && y >= 8 && y < 24 {
                // Controller body
                if y >= 10 && y < 22 && x >= 6 && x < 26 {
                    // Inner body - lighter
                    (60, 60, 70, 255)
                } else {
                    // Border
                    (40, 40, 50, 255)
                }
            } else if x >= 8 && x < 12 && y >= 12 && y < 16 {
                // D-pad up
                (100, 180, 255, 255)
            } else if x >= 8 && x < 12 && y >= 18 && y < 22 {
                // D-pad down  
                (100, 180, 255, 255)
            } else if x >= 5 && x < 9 && y >= 15 && y < 19 {
                // D-pad left
                (100, 180, 255, 255)
            } else if x >= 11 && x < 15 && y >= 15 && y < 19 {
                // D-pad right
                (100, 180, 255, 255)
            } else if x >= 20 && x < 24 && y >= 13 && y < 17 {
                // A button
                (255, 100, 100, 255)
            } else if x >= 23 && x < 27 && y >= 16 && y < 20 {
                // B button
                (255, 200, 100, 255)
            } else {
                // Transparent
                (0, 0, 0, 0)
            };
            
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = a;
        }
    }
    
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
