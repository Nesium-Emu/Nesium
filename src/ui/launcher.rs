//! ROM Launcher UI - Beautiful grid/list view of ROM collection
//!
//! Provides a modern, RetroArch-style launcher with:
//! - Grid/list view toggle
//! - Search and filtering
//! - Cartridge-style icons
//! - Recent and favorites
//! - One-click ROM loading

use crate::config::Config;
use crate::rom_browser::{RomEntry, RomScanner};
use egui::{Color32, ColorImage, RichText, TextureHandle, TextureOptions, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::thread;

/// Launcher UI state
pub struct LauncherUi {
    /// ROM entries
    roms: Vec<RomEntry>,
    
    /// Filtered/sorted ROM entries (indices into roms)
    filtered_roms: Vec<usize>,
    
    /// Search query
    search_query: String,
    
    /// Currently selected ROM index (in filtered_roms)
    selected_index: Option<usize>,
    
    /// Texture cache for ROM logos
    logo_textures: HashMap<PathBuf, TextureHandle>,
    
    /// Scan in progress
    scan_in_progress: bool,
    
    /// Scan result receiver
    scan_receiver: Option<Receiver<Vec<RomEntry>>>,
    
    /// Show settings dialog
    show_settings: bool,
    
    /// Show add directory dialog
    show_add_dir: bool,
    
    /// Scroll position
    scroll_offset: f32,
}

impl LauncherUi {
    /// Create new launcher UI
    pub fn new() -> Self {
        Self {
            roms: Vec::new(),
            filtered_roms: Vec::new(),
            search_query: String::new(),
            selected_index: None,
            logo_textures: HashMap::new(),
            scan_in_progress: false,
            scan_receiver: None,
            show_settings: false,
            show_add_dir: false,
            scroll_offset: 0.0,
        }
    }
    
    /// Start scanning ROM directories in background
    pub fn start_scan(&mut self, config: &Config) {
        if self.scan_in_progress {
            return;
        }
        
        let dirs = config.rom_dirs.clone();
        
        if dirs.is_empty() {
            log::warn!("No ROM directories configured");
            return;
        }
        
        let (tx, rx) = channel();
        self.scan_receiver = Some(rx);
        self.scan_in_progress = true;
        
        // Clone config for thread
        let config_clone = config.clone();
        
        thread::spawn(move || {
            // Catch panics to prevent hanging the UI
            let result = std::panic::catch_unwind(|| {
                let mut scanner = RomScanner::with_artwork(&config_clone);
                scanner.scan_directories(&dirs)
            });
            
            match result {
                Ok(roms) => {
                    let _ = tx.send(roms);
                }
                Err(e) => {
                    log::error!("ROM scan thread panicked: {:?}", e);
                    // Send empty list to unblock UI
                    let _ = tx.send(Vec::new());
                }
            }
        });
        
        log::info!("Started ROM scan in background");
    }
    
    /// Check for scan completion
    fn check_scan_completion(&mut self) {
        if let Some(receiver) = &self.scan_receiver {
            if let Ok(roms) = receiver.try_recv() {
                log::info!("ROM scan completed: {} ROMs found", roms.len());
                self.roms = roms;
                self.update_filtered_roms();
                self.scan_in_progress = false;
                self.scan_receiver = None;
                
                // Clear texture cache (ROM list changed)
                self.logo_textures.clear();
            }
        }
    }
    
    /// Update filtered ROM list based on search query and sort mode
    fn update_filtered_roms(&mut self) {
        let query = self.search_query.to_lowercase();
        
        self.filtered_roms = self
            .roms
            .iter()
            .enumerate()
            .filter(|(_, rom)| {
                if query.is_empty() {
                    true
                } else {
                    rom.title.to_lowercase().contains(&query)
                }
            })
            .map(|(i, _)| i)
            .collect();
        
        // Sort by title (case-insensitive)
        self.filtered_roms.sort_by(|&a, &b| {
            self.roms[a]
                .title
                .to_lowercase()
                .cmp(&self.roms[b].title.to_lowercase())
        });
    }
    
    /// Get or create logo texture
    fn get_logo_texture(
        &mut self,
        rom_path: &PathBuf,
        logo_base64: &str,
        ctx: &egui::Context,
    ) -> Option<TextureHandle> {
        // Check cache first
        if let Some(texture) = self.logo_textures.get(rom_path) {
            return Some(texture.clone());
        }
        
        // Decode logo
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        if let Ok(rgba) = BASE64.decode(logo_base64) {
            if rgba.len() == 64 * 64 * 4 {
                let color_image = ColorImage::from_rgba_unmultiplied([64, 64], &rgba);
                let texture = ctx.load_texture(
                    format!("rom_logo_{}", rom_path.display()),
                    color_image,
                    TextureOptions::LINEAR,
                );
                self.logo_textures.insert(rom_path.clone(), texture.clone());
                return Some(texture);
            }
        }
        
        None
    }
    
    /// Show launcher UI
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
    ) -> Option<PathBuf> {
        let mut rom_to_load = None;
        
        // Check scan completion
        self.check_scan_completion();
        
        // Top panel - Search and controls
        egui::TopBottomPanel::top("launcher_top").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("🎮 ROM Browser").size(20.0));
                
                ui.add_space(16.0);
                
                // Search box
                ui.label("Search:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .desired_width(300.0)
                        .hint_text("Type to search..."),
                );
                
                if response.changed() {
                    self.update_filtered_roms();
                }
                
                ui.add_space(8.0);
                
                // Rescan button
                if ui
                    .add_enabled(!self.scan_in_progress, egui::Button::new("🔄 Rescan"))
                    .on_hover_text("Rescan ROM directories")
                    .clicked()
                {
                    self.start_scan(config);
                }
                
                // Settings button
                if ui.button("⚙ Settings").clicked() {
                    self.show_settings = true;
                }
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{} ROMs", self.filtered_roms.len()));
                    
                    if self.scan_in_progress {
                        ui.spinner();
                        ui.label("Scanning...");
                    }
                });
            });
            ui.add_space(8.0);
        });
        
        // Central panel - ROM grid
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.roms.is_empty() && !self.scan_in_progress {
                // Empty state
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("No ROMs Found");
                    ui.add_space(16.0);
                    ui.label("Add a ROM directory to get started");
                    ui.add_space(16.0);
                    if ui.button("📁 Add ROM Directory").clicked() {
                        self.show_add_dir = true;
                    }
                });
            } else {
                // ROM grid
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.show_rom_grid(ui, ctx, &mut rom_to_load, config);
                    });
            }
        });
        
        // Settings dialog
        if self.show_settings {
            self.show_settings_dialog(ctx, config);
        }
        
        // Add directory dialog
        if self.show_add_dir {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                config.add_rom_dir(dir);
                if let Err(e) = config.save() {
                    log::error!("Failed to save config: {}", e);
                }
                self.start_scan(config);
            }
            self.show_add_dir = false;
        }
        
        rom_to_load
    }
    
    /// Show ROM grid view
    fn show_rom_grid(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        rom_to_load: &mut Option<PathBuf>,
        config: &Config,
    ) {
        let available_width = ui.available_width();
        let card_width = 180.0;
        let card_height = 200.0;
        let spacing = 12.0;
        
        // Calculate columns
        let columns = ((available_width + spacing) / (card_width + spacing)).floor().max(1.0) as usize;
        
        ui.add_space(8.0);
        
        // Draw grid
        let mut current_col = 0;
        let filtered_roms = self.filtered_roms.clone(); // Clone to avoid borrow issues
        
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(spacing, spacing);
            
            for &rom_idx in &filtered_roms {
                // ROM card
                let card_response = self.draw_rom_card(ui, ctx, rom_idx, card_width, card_height, config);
                
                if card_response.clicked() {
                    *rom_to_load = Some(self.roms[rom_idx].path.clone());
                }
                
                current_col += 1;
                if current_col >= columns {
                    current_col = 0;
                    ui.end_row();
                }
            }
        });
    }
    
    /// Draw a ROM card
    fn draw_rom_card(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        rom_idx: usize,
        width: f32,
        height: f32,
        config: &Config,
    ) -> egui::Response {
        // Extract data we need to avoid borrow issues
        let rom_path = self.roms[rom_idx].path.clone();
        let rom_title = self.roms[rom_idx].title.clone();
        let rom_mapper = self.roms[rom_idx].mapper;
        let rom_logo = self.roms[rom_idx].logo_base64.clone();
        let is_favorite = config.is_favorite(&rom_path);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(width, height),
            egui::Sense::click(),
        );
        
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);
            
            // Card background with hover effect
            let bg_color = if response.hovered() {
                Color32::from_rgb(60, 65, 80)
            } else {
                Color32::from_rgb(40, 45, 60)
            };
            
            // Draw rounded rectangle background
            ui.painter().rect(
                rect,
                4.0,
                bg_color,
                egui::Stroke::new(2.0, visuals.bg_stroke.color),
                egui::StrokeKind::Outside,
            );
            
            // Logo area
            let logo_rect = egui::Rect::from_min_size(
                rect.min + Vec2::new(8.0, 8.0),
                Vec2::new(width - 16.0, 120.0),
            );
            
            // Draw logo
            if let Some(texture) = self.get_logo_texture(&rom_path, &rom_logo, ctx) {
                let logo_size = 100.0;
                let logo_offset = (width - 16.0 - logo_size) / 2.0;
                let logo_display_rect = egui::Rect::from_min_size(
                    logo_rect.min + Vec2::new(logo_offset, 10.0),
                    Vec2::new(logo_size, logo_size),
                );
                ui.painter().image(
                    texture.id(),
                    logo_display_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                // Placeholder
                ui.painter().rect_filled(
                    logo_rect.shrink(8.0),
                    2.0,
                    Color32::from_rgb(30, 30, 40),
                );
            }
            
            // Title area
            let title_rect = egui::Rect::from_min_size(
                rect.min + Vec2::new(8.0, 136.0),
                Vec2::new(width - 16.0, 48.0),
            );
            
            // Title text (manually positioned)
            let title_text = if rom_title.len() > 24 {
                format!("{}...", &rom_title[..21])
            } else {
                rom_title.clone()
            };
            
            let title_pos = title_rect.center_top() + Vec2::new(0.0, 8.0);
            ui.painter().text(
                title_pos,
                egui::Align2::CENTER_TOP,
                title_text,
                egui::FontId::proportional(13.0),
                Color32::WHITE,
            );
            
            // Mapper info
            let mapper_pos = title_pos + Vec2::new(0.0, 20.0);
            ui.painter().text(
                mapper_pos,
                egui::Align2::CENTER_TOP,
                format!("Mapper {}", rom_mapper),
                egui::FontId::proportional(10.0),
                Color32::from_gray(180),
            );
            
            // Favorite star
            if is_favorite {
                let star_pos = rect.max - Vec2::new(24.0, height - 8.0);
                ui.painter().text(
                    star_pos,
                    egui::Align2::CENTER_CENTER,
                    "⭐",
                    egui::FontId::proportional(16.0),
                    Color32::from_rgb(255, 220, 100),
                );
            }
        }
        
        response
    }
    
    /// Show settings dialog
    fn show_settings_dialog(&mut self, ctx: &egui::Context, config: &mut Config) {
        egui::Window::new("⚙ ROM Browser Settings")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("ROM Directories");
                ui.add_space(8.0);
                
                // List ROM directories
                let mut to_remove = None;
                for (i, dir) in config.rom_dirs.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("📁 {}", dir.display()));
                        if ui.button("❌").clicked() {
                            to_remove = Some(i);
                        }
                    });
                }
                
                if let Some(i) = to_remove {
                    let dir = config.rom_dirs.remove(i);
                    log::info!("Removed ROM directory: {}", dir.display());
                    if let Err(e) = config.save() {
                        log::error!("Failed to save config: {}", e);
                    }
                }
                
                ui.add_space(8.0);
                
                if ui.button("➕ Add Directory").clicked() {
                    self.show_add_dir = true;
                }
                
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                
                // Close button
                if ui.button("Close").clicked() {
                    self.show_settings = false;
                }
            });
    }
    
    /// Load cached ROMs on startup (without rescan)
    pub fn load_cached(&mut self) {
        let scanner = RomScanner::new();
        self.roms = scanner.get_cached_entries();
        self.update_filtered_roms();
        log::info!("Loaded {} ROMs from cache", self.roms.len());
    }
}

