//! Main Nesium application with egui integration
//!
//! This module provides the primary UI for Nesium, including:
//! - Menu bar (File, Emulation, Settings, Help)
//! - NES screen rendering with scaling
//! - Status bar with FPS and ROM info
//! - Settings dialogs and input configuration

use super::audio::AudioOutput;
use super::settings::{KeyBindings, Settings, Theme};
use crate::cartridge::Cartridge;
use crate::cpu::Cpu;
use crate::memory::MemoryBus;
use crate::trace::TraceState;
use egui::{Color32, ColorImage, TextureHandle, TextureOptions};
use std::path::PathBuf;
use std::time::Instant;

// NES display dimensions
const NES_WIDTH: usize = 256;
const NES_HEIGHT: usize = 240;

// NTSC timing
const PPU_CYCLES_PER_FRAME: u64 = 89_342;
const CPU_CYCLES_PER_PPU_CYCLE: f64 = 1.0 / 3.0;

// NES color palette
const NES_PALETTE: [[u8; 3]; 64] = [
    [0x75, 0x75, 0x75], [0x27, 0x1B, 0x8F], [0x00, 0x00, 0xAB], [0x47, 0x00, 0x9F],
    [0x8F, 0x00, 0x77], [0xAB, 0x00, 0x13], [0xA7, 0x00, 0x00], [0x7F, 0x0B, 0x00],
    [0x43, 0x2F, 0x00], [0x00, 0x47, 0x00], [0x00, 0x51, 0x00], [0x00, 0x3F, 0x17],
    [0x1B, 0x3F, 0x5F], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],
    [0xBC, 0xBC, 0xBC], [0x00, 0x73, 0xEF], [0x23, 0x3B, 0xEF], [0x83, 0x00, 0xF3],
    [0xBF, 0x00, 0xBF], [0xE7, 0x00, 0x5B], [0xDB, 0x2B, 0x00], [0xCB, 0x4F, 0x0F],
    [0x8B, 0x73, 0x00], [0x00, 0x97, 0x00], [0x00, 0xAB, 0x00], [0x00, 0x93, 0x3B],
    [0x00, 0x83, 0x8B], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],
    [0xFF, 0xFF, 0xFF], [0x3F, 0xBF, 0xFF], [0x5F, 0x97, 0xFF], [0xA7, 0x8B, 0xFD],
    [0xF7, 0x7B, 0xFF], [0xFF, 0x77, 0xB7], [0xFF, 0x77, 0x63], [0xFF, 0x9F, 0x3B],
    [0xF3, 0xBF, 0x3F], [0x83, 0xD3, 0x13], [0x4F, 0xDF, 0x4B], [0x58, 0xF8, 0x98],
    [0x00, 0xEB, 0xDB], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],
    [0xFF, 0xFF, 0xFF], [0xAB, 0xE7, 0xFF], [0xC7, 0xD7, 0xFF], [0xD7, 0xCB, 0xFF],
    [0xFF, 0xC7, 0xFF], [0xFF, 0xC7, 0xDB], [0xFF, 0xBF, 0xB3], [0xFF, 0xDB, 0xAB],
    [0xFF, 0xE7, 0xA3], [0xE3, 0xFF, 0xA3], [0xAB, 0xF3, 0xBF], [0xB3, 0xFF, 0xCF],
    [0x9F, 0xFF, 0xF3], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],
];

/// Emulation state
struct EmulationState {
    cpu: Cpu,
    memory: MemoryBus,
    ppu_cycles_this_frame: u64,
    cpu_cycle_accumulator: f64,
    total_ppu_cycles: u64,
    trace: TraceState,
    rom_path: PathBuf,
    rom_name: String,
    has_battery: bool,
}

impl EmulationState {
    fn new(cartridge: Cartridge, rom_path: PathBuf) -> Self {
        let rom_name = rom_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();
        let has_battery = cartridge.has_ram;
        
        let mut memory = MemoryBus::new(cartridge);
        let mut cpu = Cpu::new();
        cpu.reset(&mut memory as &mut dyn crate::cpu::CpuBus);
        
        Self {
            cpu,
            memory,
            ppu_cycles_this_frame: 0,
            cpu_cycle_accumulator: 0.0,
            total_ppu_cycles: 0,
            trace: TraceState::new(false),
            rom_path,
            rom_name,
            has_battery,
        }
    }

    fn step_frame(&mut self) {
        self.ppu_cycles_this_frame = 0;

        while self.ppu_cycles_this_frame < PPU_CYCLES_PER_FRAME {
            let nmi_triggered = self.memory.step_ppu();
            
            if nmi_triggered {
                self.cpu.trigger_nmi(&mut self.memory as &mut dyn crate::cpu::CpuBus);
            }
            
            if self.memory.mapper_irq_pending() && (self.cpu.status & crate::cpu::FLAG_I) == 0 {
                self.memory.acknowledge_mapper_irq();
                self.cpu.trigger_irq(&mut self.memory as &mut dyn crate::cpu::CpuBus);
            }

            self.cpu_cycle_accumulator += CPU_CYCLES_PER_PPU_CYCLE;
            
            while self.cpu_cycle_accumulator >= 1.0 {
                if self.trace.enabled {
                    self.trace.ppu_cycle_count = self.total_ppu_cycles;
                }
                let cpu_cycles = self.cpu.step(
                    &mut self.memory as &mut dyn crate::cpu::CpuBus,
                    &mut self.trace,
                );
                self.cpu_cycle_accumulator -= cpu_cycles as f64;
                
                let irq = self.memory.step_apu(cpu_cycles as u64);
                if irq && (self.cpu.status & crate::cpu::FLAG_I) == 0 {
                    self.cpu.trigger_irq(&mut self.memory as &mut dyn crate::cpu::CpuBus);
                }
            }

            self.ppu_cycles_this_frame += 1;
            self.total_ppu_cycles += 1;
        }
    }

    fn get_framebuffer(&self) -> &[u8] {
        &self.memory.ppu.framebuffer
    }

    fn handle_input(&mut self, button: NesButton, pressed: bool) {
        match button {
            NesButton::A => self.memory.input.controller1.a = pressed,
            NesButton::B => self.memory.input.controller1.b = pressed,
            NesButton::Select => self.memory.input.controller1.select = pressed,
            NesButton::Start => self.memory.input.controller1.start = pressed,
            NesButton::Up => self.memory.input.controller1.up = pressed,
            NesButton::Down => self.memory.input.controller1.down = pressed,
            NesButton::Left => self.memory.input.controller1.left = pressed,
            NesButton::Right => self.memory.input.controller1.right = pressed,
        }
    }

    fn get_audio_samples(&mut self) -> Vec<f32> {
        self.memory.apu.take_samples()
    }

    fn adjust_audio_rate(&mut self, queue_size: usize, target_size: usize) {
        self.memory.apu.adjust_sample_rate(queue_size, target_size);
    }

    fn get_sram(&self) -> &[u8] {
        &self.memory.prg_ram
    }

    fn set_sram(&mut self, data: &[u8]) {
        let len = data.len().min(self.memory.prg_ram.len());
        self.memory.prg_ram[..len].copy_from_slice(&data[..len]);
    }

    fn reset(&mut self) {
        self.cpu.reset(&mut self.memory as &mut dyn crate::cpu::CpuBus);
        self.ppu_cycles_this_frame = 0;
        self.cpu_cycle_accumulator = 0.0;
    }
}

/// NES controller buttons
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NesButton {
    A, B, Select, Start, Up, Down, Left, Right,
}

/// UI dialog state
#[derive(Default)]
struct DialogState {
    show_about: bool,
    show_settings: bool,
    show_input_config: bool,
    input_config_binding: Option<NesButton>,
}

/// Main application state
pub struct NesiumApp {
    settings: Settings,
    emulation: Option<EmulationState>,
    texture: Option<TextureHandle>,
    audio: Option<AudioOutput>,
    dialogs: DialogState,
    
    // Performance tracking
    frame_count: u64,
    fps: f32,
    fps_counter: u32,
    last_fps_time: Instant,
    speed_percent: f32,
    
    // Frame timing for 60fps throttling (NES-master style)
    last_emulation_frame_time: Instant,
    
    // Emulation control
    paused: bool,
    fast_forward: bool,
    fast_forward_speed: f32,
    frame_advance_requested: bool,
    
    // Pending ROM to load (for drag-and-drop)
    pending_rom: Option<PathBuf>,
    
    // Input state tracking
    pressed_keys: std::collections::HashSet<egui::Key>,
    
    // Theme applied flag
    theme_applied: bool,
}

impl Default for NesiumApp {
    fn default() -> Self {
        Self::new()
    }
}

impl NesiumApp {
    pub fn new() -> Self {
        Self::with_rom(None)
    }

    pub fn with_rom(rom_path: Option<PathBuf>) -> Self {
        let settings = Settings::load();
        let audio = AudioOutput::new();
        
        if let Some(ref audio) = audio {
            audio.set_volume(settings.audio.volume);
            audio.set_muted(settings.audio.muted);
            log::info!("Audio initialized with volume: {}", settings.audio.volume);
        } else {
            log::warn!("Failed to initialize audio output");
        }
        
        Self {
            settings,
            emulation: None,
            texture: None,
            audio,
            dialogs: DialogState::default(),
            frame_count: 0,
            fps: 60.0,
            fps_counter: 0,
            last_fps_time: Instant::now(),
            speed_percent: 100.0,
            last_emulation_frame_time: Instant::now(),
            paused: false,
            fast_forward: false,
            fast_forward_speed: 1.0,
            frame_advance_requested: false,
            pending_rom: rom_path,
            pressed_keys: std::collections::HashSet::new(),
            theme_applied: false,
        }
    }

    /// Load a ROM from a file path
    fn load_rom(&mut self, path: PathBuf) {
        // Save current game if needed
        self.save_sram();
        
        match Cartridge::load(path.to_str().unwrap_or("")) {
            Ok(cartridge) => {
                log::info!("Loaded ROM: {}", path.display());
                log::info!("Mapper: {}", cartridge.mapper_id);
                log::info!("PRG ROM: {} KB", cartridge.prg_rom.len() / 1024);
                log::info!("CHR ROM: {} KB", cartridge.chr_rom.len() / 1024);
                
                let mut emulation = EmulationState::new(cartridge, path.clone());
                
                // Load save file if present
                self.load_sram_for(&mut emulation);
                
                self.emulation = Some(emulation);
                self.settings.add_recent_rom(path);
                self.paused = false;
                self.frame_count = 0;
            }
            Err(e) => {
                log::error!("Failed to load ROM: {}", e);
            }
        }
    }

    fn save_sram(&self) {
        if let Some(ref emu) = self.emulation {
            if emu.has_battery {
                let sram = emu.get_sram();
                if !sram.iter().all(|&b| b == 0) {
                    let save_path = emu.rom_path.with_extension("sav");
                    if let Err(e) = std::fs::write(&save_path, sram) {
                        log::error!("Failed to save SRAM: {}", e);
                    } else {
                        log::info!("Saved game to: {}", save_path.display());
                    }
                }
            }
        }
    }

    fn load_sram_for(&self, emulation: &mut EmulationState) {
        if emulation.has_battery {
            let save_path = emulation.rom_path.with_extension("sav");
            if save_path.exists() {
                if let Ok(data) = std::fs::read(&save_path) {
                    emulation.set_sram(&data);
                    log::info!("Loaded save file: {}", save_path.display());
                }
            }
        }
    }

    fn open_rom_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("NES ROM", &["nes"])
            .add_filter("All files", &["*"]);
        
        if let Some(ref dir) = self.settings.last_rom_directory {
            dialog = dialog.set_directory(dir);
        }
        
        if let Some(path) = dialog.pick_file() {
            self.pending_rom = Some(path);
        }
    }

    fn update_emulation(&mut self, ctx: &egui::Context) {
        // Handle pending ROM load
        if let Some(path) = self.pending_rom.take() {
            log::info!("Loading pending ROM: {}", path.display());
            self.load_rom(path);
        }

        // Handle input from keyboard
        self.handle_input(ctx);

        // Run emulation
        let should_run = self.emulation.is_some() && (!self.paused || self.frame_advance_requested);
        
        if should_run {
            // Target frame time: 60fps = ~16.67ms per frame (NES-master approach)
            let target_frame_time = std::time::Duration::from_secs_f64(1.0 / 60.0);
            
            // Calculate speed multiplier
            let speed_multiplier = if self.fast_forward {
                self.fast_forward_speed as f64
            } else {
                1.0
            };
            
            // Calculate how many frames we should run
            let frames_to_run = if self.frame_advance_requested {
                // Frame advance: run exactly 1 frame
                1
            } else {
                // Check if enough time has passed for at least one frame
                let elapsed = self.last_emulation_frame_time.elapsed();
                let target_time_secs = target_frame_time.as_secs_f64() / speed_multiplier;
                let target_time = std::time::Duration::from_secs_f64(target_time_secs);
                
                if elapsed >= target_time {
                    // Calculate how many frames we can run
                    let frames = (elapsed.as_secs_f64() / target_frame_time.as_secs_f64()) * speed_multiplier;
                    let frames_int = frames.floor() as usize;
                    
                    // Update timing: subtract the time used for these frames
                    if frames_int > 0 {
                        let time_used_secs = target_frame_time.as_secs_f64() * (frames_int as f64 / speed_multiplier);
                        let time_used = std::time::Duration::from_secs_f64(time_used_secs);
                        self.last_emulation_frame_time += time_used;
                    }
                    
                    frames_int.min(10) // Cap at 10 frames per update
                } else {
                    // Not enough time has passed, skip this update
                    0
                }
            };

            for _ in 0..frames_to_run {
                // Step emulation
                if let Some(ref mut emu) = self.emulation {
                    emu.step_frame();
                }
                
                // Update texture from framebuffer (copy to avoid borrow issues)
                let framebuffer: Vec<u8> = self.emulation.as_ref()
                    .map(|emu| emu.get_framebuffer().to_vec())
                    .unwrap_or_default();
                if !framebuffer.is_empty() {
                    self.update_texture(ctx, &framebuffer);
                }
                
                // Handle audio
                if let Some(ref mut emu) = self.emulation {
                    let samples = emu.get_audio_samples();
                    if let Some(ref audio) = self.audio {
                        audio.queue_samples(&samples);
                        emu.adjust_audio_rate(audio.queued_samples(), audio.target_queue_size());
                    }
                }

                self.frame_count += 1;
                self.fps_counter += 1;
            }

            self.frame_advance_requested = false;
        } else {
            // Reset frame timing when paused
            self.last_emulation_frame_time = Instant::now();
        }

        // Update FPS
        let elapsed = self.last_fps_time.elapsed();
        if elapsed.as_secs_f32() >= 1.0 {
            self.fps = self.fps_counter as f32 / elapsed.as_secs_f32();
            self.speed_percent = (self.fps / 60.0988) * 100.0;
            self.fps_counter = 0;
            self.last_fps_time = Instant::now();
        }
    }

    fn update_texture(&mut self, ctx: &egui::Context, framebuffer: &[u8]) {
        // Convert palette indices to RGBA
        let mut pixels = Vec::with_capacity(NES_WIDTH * NES_HEIGHT);
        for &palette_idx in framebuffer.iter().take(NES_WIDTH * NES_HEIGHT) {
            let palette_idx = (palette_idx & 0x3F) as usize;
            let rgb = NES_PALETTE[palette_idx];
            pixels.push(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        }

        let image = ColorImage {
            size: [NES_WIDTH, NES_HEIGHT],
            pixels,
        };

        match &mut self.texture {
            Some(texture) => {
                texture.set(image, TextureOptions::NEAREST);
            }
            None => {
                self.texture = Some(ctx.load_texture("nes_screen", image, TextureOptions::NEAREST));
            }
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let bindings = &self.settings.key_bindings;
        
        ctx.input(|i| {
            // Check each bound key
            let buttons = [
                (bindings.a, NesButton::A),
                (bindings.b, NesButton::B),
                (bindings.select, NesButton::Select),
                (bindings.start, NesButton::Start),
                (bindings.up, NesButton::Up),
                (bindings.down, NesButton::Down),
                (bindings.left, NesButton::Left),
                (bindings.right, NesButton::Right),
            ];

            for (key, button) in buttons {
                let currently_pressed = i.key_down(key);
                let was_pressed = self.pressed_keys.contains(&key);

                if currently_pressed && !was_pressed {
                    self.pressed_keys.insert(key);
                    if let Some(ref mut emu) = self.emulation {
                        emu.handle_input(button, true);
                    }
                } else if !currently_pressed && was_pressed {
                    self.pressed_keys.remove(&key);
                    if let Some(ref mut emu) = self.emulation {
                        emu.handle_input(button, false);
                    }
                }
            }

            // Handle keyboard shortcuts
            if i.key_pressed(egui::Key::Escape) {
                self.paused = !self.paused;
            }
            if i.key_pressed(egui::Key::R) && i.modifiers.ctrl {
                if let Some(ref mut emu) = self.emulation {
                    emu.reset();
                }
            }
            if i.key_pressed(egui::Key::Space) && i.modifiers.shift {
                self.frame_advance_requested = true;
            }

            // Fast forward toggle
            self.fast_forward = i.key_down(egui::Key::F);
        });
    }

    fn render_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                // File menu
                ui.menu_button("File", |ui| {
                    if ui.button("📂 Open ROM...").clicked() {
                        self.open_rom_dialog();
                        ui.close_menu();
                    }
                    
                    ui.menu_button("Recent ROMs", |ui| {
                        if self.settings.recent_roms.is_empty() {
                            ui.label("No recent ROMs");
                        } else {
                            for rom in self.settings.recent_roms.clone() {
                                let name = rom.file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Unknown");
                                if ui.button(name).clicked() {
                                    self.pending_rom = Some(rom);
                                    ui.close_menu();
                                }
                            }
                        }
                    });
                    
                    ui.separator();
                    
                    if ui.button("🚪 Exit").clicked() {
                        self.save_sram();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                // Emulation menu
                ui.menu_button("Emulation", |ui| {
                    let pause_text = if self.paused { "▶ Resume" } else { "⏸ Pause" };
                    if ui.button(pause_text).clicked() {
                        self.paused = !self.paused;
                        ui.close_menu();
                    }
                    
                    if ui.button("🔄 Reset").clicked() {
                        if let Some(ref mut emu) = self.emulation {
                            emu.reset();
                        }
                        ui.close_menu();
                    }
                    
                    if ui.button("⏭ Frame Advance").clicked() {
                        self.frame_advance_requested = true;
                        ui.close_menu();
                    }
                    
                    ui.separator();
                    
                    ui.menu_button("Speed", |ui| {
                        if ui.radio(self.fast_forward_speed == 1.0, "1x (100%)").clicked() {
                            self.fast_forward_speed = 1.0;
                        }
                        if ui.radio(self.fast_forward_speed == 1.5, "1.5x").clicked() {
                            self.fast_forward_speed = 1.5;
                        }
                        if ui.radio(self.fast_forward_speed == 2.0, "2x").clicked() {
                            self.fast_forward_speed = 2.0;
                        }
                        if ui.radio(self.fast_forward_speed == 3.0, "3x").clicked() {
                            self.fast_forward_speed = 3.0;
                        }
                        if ui.radio(self.fast_forward_speed == 4.0, "4x").clicked() {
                            self.fast_forward_speed = 4.0;
                        }
                    });
                });

                // Settings menu
                ui.menu_button("Settings", |ui| {
                    if ui.button("🎮 Input Configuration...").clicked() {
                        self.dialogs.show_input_config = true;
                        ui.close_menu();
                    }
                    
                    ui.separator();
                    
                    ui.menu_button("🎨 Theme", |ui| {
                        if ui.radio(self.settings.theme == Theme::Dark, "Dark").clicked() {
                            self.settings.theme = Theme::Dark;
                            self.settings.apply_theme(ctx);
                            self.settings.save();
                        }
                        if ui.radio(self.settings.theme == Theme::Light, "Light").clicked() {
                            self.settings.theme = Theme::Light;
                            self.settings.apply_theme(ctx);
                            self.settings.save();
                        }
                        if ui.radio(self.settings.theme == Theme::Catppuccin, "Catppuccin").clicked() {
                            self.settings.theme = Theme::Catppuccin;
                            self.settings.apply_theme(ctx);
                            self.settings.save();
                        }
                        if ui.radio(self.settings.theme == Theme::Nord, "Nord").clicked() {
                            self.settings.theme = Theme::Nord;
                            self.settings.apply_theme(ctx);
                            self.settings.save();
                        }
                    });
                    
                    ui.menu_button("📺 Video", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Scale:");
                            if ui.add(egui::Slider::new(&mut self.settings.video.scale, 1..=6)).changed() {
                                self.settings.save();
                            }
                        });
                        
                        if ui.checkbox(&mut self.settings.video.integer_scaling, "Integer Scaling").changed() {
                            self.settings.save();
                        }
                        if ui.checkbox(&mut self.settings.video.show_fps, "Show FPS").changed() {
                            self.settings.save();
                        }
                    });
                    
                    ui.menu_button("🔊 Audio", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Volume:");
                            if ui.add(egui::Slider::new(&mut self.settings.audio.volume, 0.0..=1.0)).changed() {
                                if let Some(ref audio) = self.audio {
                                    audio.set_volume(self.settings.audio.volume);
                                }
                                self.settings.save();
                            }
                        });
                        
                        if ui.checkbox(&mut self.settings.audio.muted, "Mute").changed() {
                            if let Some(ref audio) = self.audio {
                                audio.set_muted(self.settings.audio.muted);
                            }
                            self.settings.save();
                        }
                    });
                });

                // Help menu
                ui.menu_button("Help", |ui| {
                    if ui.button("ℹ About Nesium").clicked() {
                        self.dialogs.show_about = true;
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn render_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(24.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // ROM name
                    if let Some(ref emu) = self.emulation {
                        ui.label(&emu.rom_name);
                    } else {
                        ui.label("No ROM loaded");
                    }
                    
                    ui.separator();
                    
                    // Pause indicator
                    if self.paused {
                        ui.colored_label(Color32::from_rgb(255, 180, 0), "⏸ PAUSED");
                    } else if self.fast_forward {
                        ui.colored_label(Color32::from_rgb(0, 200, 100), "⏩ FAST");
                    } else {
                        ui.colored_label(Color32::from_rgb(100, 200, 100), "▶ RUNNING");
                    }
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // FPS and speed
                        if self.settings.video.show_fps {
                            ui.label(format!("{:.0}%", self.speed_percent));
                            ui.separator();
                            ui.label(format!("{:.1} FPS", self.fps));
                        }
                    });
                });
            });
    }

    fn render_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(8, 8, 12)))
            .show(ctx, |ui| {
                if let Some(ref texture) = self.texture {
                    // Calculate scaled size maintaining aspect ratio
                    let available = ui.available_size();
                    let scale = self.settings.video.scale as f32;
                    
                    let nes_aspect = NES_WIDTH as f32 / NES_HEIGHT as f32;
                    let mut width = NES_WIDTH as f32 * scale;
                    let mut height = NES_HEIGHT as f32 * scale;
                    
                    // Fit to available space
                    if width > available.x {
                        width = available.x;
                        height = width / nes_aspect;
                    }
                    if height > available.y {
                        height = available.y;
                        width = height * nes_aspect;
                    }
                    
                    // Integer scaling if enabled
                    if self.settings.video.integer_scaling {
                        let int_scale = (width / NES_WIDTH as f32).floor().max(1.0);
                        width = NES_WIDTH as f32 * int_scale;
                        height = NES_HEIGHT as f32 * int_scale;
                    }
                    
                    // Center the image
                    let offset_x = (available.x - width) / 2.0;
                    let offset_y = (available.y - height) / 2.0;
                    
                    let screen_rect = egui::Rect::from_min_size(
                        ui.min_rect().min + egui::vec2(offset_x, offset_y),
                        egui::vec2(width, height),
                    );
                    
                    // Draw CRT-style frame/border
                    let border_width = 4.0;
                    
                    // Outer glow
                    ui.painter().rect_filled(
                        screen_rect.expand(border_width + 2.0),
                        egui::CornerRadius::same(8),
                        Color32::from_rgb(20, 20, 30),
                    );
                    
                    // Border
                    ui.painter().rect_stroke(
                        screen_rect.expand(border_width),
                        egui::CornerRadius::same(6),
                        egui::Stroke::new(2.0, Color32::from_rgb(50, 50, 60)),
                        egui::StrokeKind::Middle,
                    );
                    
                    // Inner shadow
                    ui.painter().rect_stroke(
                        screen_rect,
                        egui::CornerRadius::same(2),
                        egui::Stroke::new(1.0, Color32::from_rgb(30, 30, 35)),
                        egui::StrokeKind::Inside,
                    );
                    
                    // The NES screen
                    ui.put(screen_rect, egui::Image::new(egui::load::SizedTexture::new(
                        texture.id(),
                        egui::vec2(width, height),
                    )));
                } else {
                    // No ROM loaded - show welcome screen
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(ui.available_height() / 3.0);
                            
                            ui.heading(egui::RichText::new("🎮 NESIUM")
                                .size(48.0)
                                .color(Color32::from_rgb(100, 180, 255)));
                            
                            ui.add_space(16.0);
                            
                            ui.label(egui::RichText::new("High-Accuracy NES Emulator")
                                .size(18.0)
                                .color(Color32::from_rgb(150, 150, 160)));
                            
                            ui.add_space(40.0);
                            
                            if ui.button(egui::RichText::new("📂 Open ROM")
                                .size(16.0))
                                .clicked() {
                                self.open_rom_dialog();
                            }
                            
                            ui.add_space(20.0);
                            
                            ui.label(egui::RichText::new("or drag and drop a .nes file")
                                .size(14.0)
                                .color(Color32::from_rgb(100, 100, 110)));
                        });
                    });
                }
            });
    }

    fn render_dialogs(&mut self, ctx: &egui::Context) {
        // About dialog
        if self.dialogs.show_about {
            egui::Window::new("About Nesium")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading(egui::RichText::new("🎮 Nesium")
                            .size(32.0)
                            .color(Color32::from_rgb(100, 180, 255)));
                        
                        ui.add_space(8.0);
                        ui.label("Version 0.1.0");
                        ui.add_space(16.0);
                        
                        ui.label("A high-accuracy NES emulator");
                        ui.label("written in Rust");
                        
                        ui.add_space(16.0);
                        
                        ui.label(egui::RichText::new("Features:")
                            .strong());
                        ui.label("• Cycle-accurate CPU, PPU, APU");
                        ui.label("• NROM, MMC1, UxROM, MMC3 mappers");
                        ui.label("• Battery-backed SRAM saves");
                        
                        ui.add_space(16.0);
                        
                        if ui.button("Close").clicked() {
                            self.dialogs.show_about = false;
                        }
                    });
                });
        }

        // Input configuration dialog
        if self.dialogs.show_input_config {
            self.render_input_config_dialog(ctx);
        }
    }

    fn render_input_config_dialog(&mut self, ctx: &egui::Context) {
        let mut open = true;
        
        egui::Window::new("Input Configuration")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.heading("Controller 1");
                ui.add_space(8.0);
                
                egui::Grid::new("input_grid")
                    .num_columns(2)
                    .spacing([20.0, 8.0])
                    .show(ui, |ui| {
                        let bindings = [
                            ("A Button", NesButton::A, self.settings.key_bindings.a),
                            ("B Button", NesButton::B, self.settings.key_bindings.b),
                            ("Select", NesButton::Select, self.settings.key_bindings.select),
                            ("Start", NesButton::Start, self.settings.key_bindings.start),
                            ("Up", NesButton::Up, self.settings.key_bindings.up),
                            ("Down", NesButton::Down, self.settings.key_bindings.down),
                            ("Left", NesButton::Left, self.settings.key_bindings.left),
                            ("Right", NesButton::Right, self.settings.key_bindings.right),
                        ];
                        
                        for (name, button, key) in bindings {
                            ui.label(name);
                            
                            let is_binding = self.dialogs.input_config_binding == Some(button);
                            
                            let button_text = if is_binding {
                                "Press any key...".to_string()
                            } else {
                                format!("{:?}", key)
                            };
                            
                            if ui.button(&button_text).clicked() {
                                self.dialogs.input_config_binding = Some(button);
                            }
                            
                            ui.end_row();
                        }
                    });
                
                // Handle key binding
                if let Some(binding_button) = self.dialogs.input_config_binding {
                    ctx.input(|i| {
                        for key in egui::Key::ALL {
                            if i.key_pressed(*key) {
                                match binding_button {
                                    NesButton::A => self.settings.key_bindings.a = *key,
                                    NesButton::B => self.settings.key_bindings.b = *key,
                                    NesButton::Select => self.settings.key_bindings.select = *key,
                                    NesButton::Start => self.settings.key_bindings.start = *key,
                                    NesButton::Up => self.settings.key_bindings.up = *key,
                                    NesButton::Down => self.settings.key_bindings.down = *key,
                                    NesButton::Left => self.settings.key_bindings.left = *key,
                                    NesButton::Right => self.settings.key_bindings.right = *key,
                                }
                                self.dialogs.input_config_binding = None;
                                self.settings.save();
                                break;
                            }
                        }
                    });
                }
                
                ui.add_space(16.0);
                
                ui.horizontal(|ui| {
                    if ui.button("Reset to Defaults").clicked() {
                        self.settings.key_bindings = KeyBindings::default();
                        self.settings.save();
                    }
                    
                    if ui.button("Close").clicked() {
                        self.dialogs.show_input_config = false;
                    }
                });
            });
        
        if !open {
            self.dialogs.show_input_config = false;
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(ref path) = file.path {
                    if path.extension().map(|e| e == "nes").unwrap_or(false) {
                        self.pending_rom = Some(path.clone());
                    }
                }
            }
        });
    }
}

impl eframe::App for NesiumApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme on first run
        if !self.theme_applied {
            self.settings.apply_theme(ctx);
            self.theme_applied = true;
        }

        // Handle drag and drop
        self.handle_dropped_files(ctx);

        // Run emulation
        self.update_emulation(ctx);

        // Render UI
        self.render_menu_bar(ctx);
        self.render_status_bar(ctx);
        self.render_central_panel(ctx);
        self.render_dialogs(ctx);

        // Request repaint for continuous emulation
        if self.emulation.is_some() && !self.paused {
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Save game before exiting
        self.save_sram();
        self.settings.save();
    }
}
