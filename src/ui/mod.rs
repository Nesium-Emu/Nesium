//! Nesium UI - Modern egui-based user interface for the NES emulator
//!
//! This module provides a polished, native-looking desktop UI using egui/eframe.
//! It handles ROM loading, input configuration, settings, and renders the NES
//! framebuffer as an egui texture.

mod app;
mod settings;
mod audio;
mod launcher;

pub use app::NesiumApp;

