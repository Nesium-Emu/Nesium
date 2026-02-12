//! ROM Browser - Scan, cache, and display NES ROM collections
//!
//! This module provides:
//! - Recursive ROM directory scanning
//! - iNES header parsing for metadata
//! - Logo/icon extraction from CHR ROM data
//! - Persistent caching for fast startup
//! - Thumbnail generation and management

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crate::artwork_scraper::ArtworkDownloader;
use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// ROM entry with metadata and cached thumbnail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomEntry {
    /// Full path to ROM file
    pub path: PathBuf,
    
    /// Display name (from header or filename)
    pub title: String,
    
    /// Mapper number
    pub mapper: u8,
    
    /// PRG ROM size in 16KB units
    pub prg_size: u8,
    
    /// CHR ROM size in 8KB units
    pub chr_size: u8,
    
    /// Logo/icon as base64-encoded RGBA data (64x64)
    #[serde(default)]
    pub logo_base64: String,
    
    /// File size in bytes
    pub file_size: u64,
    
    /// Last modified timestamp
    pub modified: u64,
}

impl RomEntry {
    /// Create from ROM file path with optional online artwork
    pub fn from_file(path: PathBuf, artwork_downloader: Option<&mut ArtworkDownloader>) -> Result<Self, Box<dyn std::error::Error>> {
        let metadata = fs::metadata(&path)?;
        let file_size = metadata.len();
        
        // Skip files that are too large (> 10MB, likely not a valid NES ROM)
        const MAX_ROM_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
        if file_size > MAX_ROM_SIZE {
            return Err(format!("File too large: {} bytes (max {})", file_size, MAX_ROM_SIZE).into());
        }
        
        let modified = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        
        // Read ROM data
        let data = if path.extension().and_then(|s| s.to_str()) == Some("zip") {
            Self::read_from_zip(&path)?
        } else {
            fs::read(&path)?
        };
        
        // Parse iNES header
        if data.len() < 16 || &data[0..4] != b"NES\x1A" {
            return Err("Invalid iNES header".into());
        }
        
        let prg_size = data[4];
        let chr_size = data[5];
        let flags6 = data[6];
        let flags7 = data[7];
        let mapper = (flags7 & 0xF0) | (flags6 >> 4);
        
        // Extract title from filename (better than raw header data)
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();
        
        // Try to download artwork if enabled
        let logo_base64 = if let Some(downloader) = artwork_downloader {
            if let Some(artwork_path) = downloader.download_artwork(&path, &title) {
                log::info!("Using downloaded artwork for: {}", title);
                // Load the cached artwork and convert to base64
                if let Ok(img_data) = fs::read(&artwork_path) {
                    if let Ok(img) = image::load_from_memory(&img_data) {
                        let rgba = img.to_rgba8();
                        let (width, height) = rgba.dimensions();
                        if width == 64 && height == 64 {
                            BASE64.encode(rgba.as_raw())
                        } else {
                            // Resize if needed
                            let resized = img.resize_exact(64, 64, image::imageops::FilterType::Lanczos3);
                            BASE64.encode(resized.to_rgba8().as_raw())
                        }
                    } else {
                        // Fallback to CHR extraction
                        Self::extract_logo(&data, prg_size, chr_size, &title)?
                    }
                } else {
                    // Fallback to CHR extraction
                    Self::extract_logo(&data, prg_size, chr_size, &title)?
                }
            } else {
                // No online artwork, use CHR extraction
                Self::extract_logo(&data, prg_size, chr_size, &title)?
            }
        } else {
            // Online artwork disabled, use CHR extraction
            Self::extract_logo(&data, prg_size, chr_size, &title)?
        };
        
        Ok(Self {
            path,
            title,
            mapper,
            prg_size,
            chr_size,
            logo_base64,
            file_size,
            modified,
        })
    }
    
    /// Read ROM from ZIP archive (takes first .nes file)
    fn read_from_zip(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        const MAX_UNCOMPRESSED_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
        
        let file = fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        
        // Find first .nes file
        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            if file.name().to_lowercase().ends_with(".nes") {
                // Check uncompressed size
                let size = file.size();
                if size > MAX_UNCOMPRESSED_SIZE {
                    return Err(format!("Uncompressed file too large: {} bytes", size).into());
                }
                
                // Read with size limit
                let mut data = Vec::with_capacity(size.min(MAX_UNCOMPRESSED_SIZE) as usize);
                let bytes_read = file.take(MAX_UNCOMPRESSED_SIZE).read_to_end(&mut data)?;
                
                if bytes_read as u64 >= MAX_UNCOMPRESSED_SIZE {
                    return Err("File exceeds maximum size during extraction".into());
                }
                
                return Ok(data);
            }
        }
        
        Err("No .nes file found in ZIP".into())
    }
    
    /// Extract logo/icon from CHR ROM
    /// 
    /// Extracts the Nintendo logo from CHR ROM at $1FA0-$20FF (standard location)
    /// and generates a 64x64 RGBA thumbnail
    fn extract_logo(data: &[u8], prg_size: u8, chr_size: u8, title: &str) -> Result<String, Box<dyn std::error::Error>> {
        const HEADER_SIZE: usize = 16;
        const PRG_BANK_SIZE: usize = 16384;
        const CHR_BANK_SIZE: usize = 8192;
        
        // If no CHR ROM, generate a placeholder with game initials
        if chr_size == 0 {
            return Ok(Self::generate_placeholder_logo(title));
        }
        
        let prg_rom_size = prg_size as usize * PRG_BANK_SIZE;
        let chr_start = HEADER_SIZE + prg_rom_size;
        let chr_end = chr_start + chr_size as usize * CHR_BANK_SIZE;
        
        if chr_end > data.len() {
            return Ok(Self::generate_placeholder_logo(title));
        }
        
        let chr_data = &data[chr_start..chr_end];
        
        // Try to extract Nintendo logo tiles (common at $1FA0 in CHR ROM)
        let logo_offset = 0x1FA0;
        if logo_offset + 128 <= chr_data.len() {
            return Ok(Self::render_chr_tiles(chr_data, logo_offset));
        }
        
        // Fallback: Use first few pattern tiles
        Ok(Self::render_chr_tiles(chr_data, 0))
    }
    
    /// Analyze CHR data to find the most interesting tiles
    fn find_best_tiles(chr_data: &[u8]) -> Vec<usize> {
        let num_tiles = chr_data.len() / 16;
        let mut tile_scores: Vec<(usize, u32)> = Vec::new();
        
        for tile_idx in 0..num_tiles.min(256) {
            let offset = tile_idx * 16;
            if offset + 16 > chr_data.len() {
                break;
            }
            
            // Score tiles based on complexity and color usage
            let mut score = 0u32;
            let mut unique_pixels = std::collections::HashSet::new();
            
            for py in 0..8 {
                let plane0 = chr_data[offset + py];
                let plane1 = chr_data[offset + py + 8];
                
                for px in 0..8 {
                    let bit = 7 - px;
                    let color_idx = ((plane0 >> bit) & 1) | (((plane1 >> bit) & 1) << 1);
                    unique_pixels.insert(color_idx);
                    
                    // Prefer non-background pixels
                    if color_idx != 0 {
                        score += 3;
                    }
                }
            }
            
            // Bonus for tiles with multiple colors (more interesting)
            score += (unique_pixels.len() as u32) * 10;
            
            // Penalize completely empty tiles
            if unique_pixels.len() <= 1 {
                score = 0;
            }
            
            tile_scores.push((tile_idx, score));
        }
        
        // Sort by score and take top tiles
        tile_scores.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
        tile_scores.iter().take(64).map(|&(idx, _)| idx).collect()
    }
    
    /// Render CHR pattern tiles to RGBA image with improved selection
    fn render_chr_tiles(chr_data: &[u8], _offset: usize) -> String {
        const ICON_SIZE: usize = 64;
        const TILE_SIZE: usize = 8;
        const TILES_X: usize = 8; // 8x8 grid of tiles
        const TILES_Y: usize = 8;
        
        let mut rgba = vec![0u8; ICON_SIZE * ICON_SIZE * 4];
        
        // Enhanced NES palette with better colors
        let palette = [
            [32, 32, 48, 255],      // Dark background
            [100, 180, 255, 255],   // NES blue
            [255, 100, 100, 255],   // NES red  
            [255, 220, 100, 255],   // NES yellow/gold
        ];
        
        // Try to find the best tiles instead of using fixed offset
        let best_tiles = if chr_data.len() >= 1024 {
            Self::find_best_tiles(chr_data)
        } else {
            // Not enough data, use sequential
            (0..64).collect()
        };
        
        // Render 8x8 tile grid using best tiles
        for ty in 0..TILES_Y {
            for tx in 0..TILES_X {
                let grid_index = ty * TILES_X + tx;
                let tile_index = best_tiles.get(grid_index).copied().unwrap_or(0);
                let tile_offset = tile_index * 16;
                
                if tile_offset + 16 > chr_data.len() {
                    continue;
                }
                
                // Decode NES 2bpp tile format
                for py in 0..TILE_SIZE {
                    let plane0 = chr_data[tile_offset + py];
                    let plane1 = chr_data[tile_offset + py + 8];
                    
                    for px in 0..TILE_SIZE {
                        let bit = 7 - px;
                        let color_idx = ((plane0 >> bit) & 1) | (((plane1 >> bit) & 1) << 1);
                        let color = palette[color_idx as usize];
                        
                        let screen_x = tx * TILE_SIZE + px;
                        let screen_y = ty * TILE_SIZE + py;
                        let pixel_idx = (screen_y * ICON_SIZE + screen_x) * 4;
                        
                        rgba[pixel_idx] = color[0];
                        rgba[pixel_idx + 1] = color[1];
                        rgba[pixel_idx + 2] = color[2];
                        rgba[pixel_idx + 3] = color[3];
                    }
                }
            }
        }
        
        // Encode to base64
        BASE64.encode(&rgba)
    }
    
    /// Generate placeholder logo with game initials for ROMs without CHR data
    fn generate_placeholder_logo(title: &str) -> String {
        const SIZE: usize = 64;
        let mut rgba = vec![0u8; SIZE * SIZE * 4];
        
        // Generate color from title hash (each game gets unique color)
        let hash = title.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        let hue = (hash % 360) as f32;
        let (r, g, b) = Self::hsl_to_rgb(hue, 0.6, 0.4);
        let (r_light, g_light, b_light) = Self::hsl_to_rgb(hue, 0.6, 0.6);
        
        // Create gradient background
        for y in 0..SIZE {
            for x in 0..SIZE {
                let i = (y * SIZE + x) * 4;
                
                // Radial gradient from center
                let dx = x as f32 - SIZE as f32 / 2.0;
                let dy = y as f32 - SIZE as f32 / 2.0;
                let dist = (dx * dx + dy * dy).sqrt() / (SIZE as f32 / 2.0);
                let factor = (1.0 - dist).max(0.0);
                
                rgba[i] = ((r_light as f32 * factor + r as f32 * (1.0 - factor)) as u8).max(20);
                rgba[i + 1] = ((g_light as f32 * factor + g as f32 * (1.0 - factor)) as u8).max(20);
                rgba[i + 2] = ((b_light as f32 * factor + b as f32 * (1.0 - factor)) as u8).max(30);
                rgba[i + 3] = 255;
            }
        }
        
        // Extract initials (first letter of first 2-3 words)
        let initials = Self::extract_initials(title);
        
        // Draw initials as simple pixelated text (NES-style)
        Self::draw_text_on_image(&mut rgba, SIZE, &initials, 255, 255, 255);
        
        BASE64.encode(&rgba)
    }
    
    /// Extract initials from game title (e.g. "Super Mario Bros" -> "SMB")
    fn extract_initials(title: &str) -> String {
        let words: Vec<&str> = title
            .split(|c: char| c.is_whitespace() || c == '-')
            .filter(|w| !w.is_empty() && w.len() > 1)
            .filter(|w| !matches!(w.to_lowercase().as_str(), "the" | "a" | "an" | "of"))
            .take(3)
            .collect();
        
        if words.is_empty() {
            // Fallback: take first 2-3 chars
            title.chars().filter(|c| c.is_alphanumeric()).take(3).collect()
        } else {
            words.iter().filter_map(|w| w.chars().next()).collect()
        }
    }
    
    /// Draw text on RGBA image (simple 5x7 bitmap font)
    fn draw_text_on_image(rgba: &mut [u8], size: usize, text: &str, r: u8, g: u8, b: u8) {
        let char_width = 6;
        let char_height = 9;
        let total_width = text.len() * char_width;
        let start_x = (size - total_width) / 2;
        let start_y = (size - char_height) / 2;
        
        for (i, ch) in text.chars().enumerate() {
            Self::draw_char(rgba, size, ch, start_x + i * char_width, start_y, r, g, b);
        }
    }
    
    /// Draw a single character (simple bitmap font)
    fn draw_char(rgba: &mut [u8], size: usize, ch: char, x: usize, y: usize, r: u8, g: u8, b: u8) {
        // Simple 5x7 bitmap font (only uppercase letters for initials)
        let glyph = match ch.to_ascii_uppercase() {
            'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
            'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
            'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
            'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
            'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
            'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
            'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
            'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
            'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
            'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
            'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
            'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
            'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
            'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
            'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
            'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
            'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
            'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
            'S' => [0b01110, 0b10001, 0b10000, 0b01110, 0b00001, 0b10001, 0b01110],
            'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
            'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
            'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
            'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
            'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
            'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
            'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
            '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
            '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
            '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
            '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
            _ => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        };
        
        for (row, &bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if (bits >> (4 - col)) & 1 == 1 {
                    let px = x + col;
                    let py = y + row;
                    if px < size && py < size {
                        let idx = (py * size + px) * 4;
                        rgba[idx] = r;
                        rgba[idx + 1] = g;
                        rgba[idx + 2] = b;
                        rgba[idx + 3] = 255;
                    }
                }
            }
        }
    }
    
    /// Convert HSL to RGB
    fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = l - c / 2.0;
        
        let (r, g, b) = match h as u32 {
            0..=59 => (c, x, 0.0),
            60..=119 => (x, c, 0.0),
            120..=179 => (0.0, c, x),
            180..=239 => (0.0, x, c),
            240..=299 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        
        (
            ((r + m) * 255.0) as u8,
            ((g + m) * 255.0) as u8,
            ((b + m) * 255.0) as u8,
        )
    }
    
    /// Decode base64 logo to RGBA bytes
    pub fn decode_logo(&self) -> Option<Vec<u8>> {
        BASE64.decode(&self.logo_base64).ok()
    }
}

/// ROM cache for fast loading
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RomCache {
    /// Cached ROM entries by path
    pub entries: HashMap<PathBuf, RomEntry>,
    
    /// Last scan timestamp
    pub last_scan: u64,
}

impl RomCache {
    /// Get cache file path
    pub fn cache_path() -> PathBuf {
        if let Some(cache_dir) = dirs::cache_dir() {
            let nesium_dir = cache_dir.join("nesium");
            fs::create_dir_all(&nesium_dir).ok();
            nesium_dir.join("roms_cache.json")
        } else {
            PathBuf::from("roms_cache.json")
        }
    }
    
    /// Load cache from disk
    pub fn load() -> Self {
        let path = Self::cache_path();
        
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(contents) => {
                    match serde_json::from_str(&contents) {
                        Ok(cache) => {
                            log::info!("Loaded ROM cache from: {}", path.display());
                            return cache;
                        }
                        Err(e) => {
                            log::error!("Failed to parse ROM cache: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to read ROM cache: {}", e);
                }
            }
        }
        
        Self::default()
    }
    
    /// Save cache to disk
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::cache_path();
        let contents = serde_json::to_string_pretty(self)?;
        fs::write(&path, contents)?;
        log::info!("Saved ROM cache to: {} ({} entries)", path.display(), self.entries.len());
        Ok(())
    }
    
    /// Check if entry needs refresh (file modified or missing from cache)
    pub fn needs_refresh(&self, path: &Path) -> bool {
        if let Some(entry) = self.entries.get(path) {
            if let Ok(metadata) = fs::metadata(path) {
                if let Ok(modified) = metadata.modified() {
                    let current_modified = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    return current_modified > entry.modified;
                }
            }
        }
        true
    }
}

/// ROM scanner for discovering and cataloging ROMs
pub struct RomScanner {
    cache: RomCache,
    artwork_downloader: Option<ArtworkDownloader>,
}

impl RomScanner {
    /// Create new scanner
    pub fn new() -> Self {
        Self {
            cache: RomCache::load(),
            artwork_downloader: None,
        }
    }
    
    /// Create new scanner with artwork downloading enabled
    pub fn with_artwork(config: &Config) -> Self {
        let artwork_downloader = if config.artwork.enable_online {
            log::info!("Online artwork downloading enabled");
            let artwork_type = match config.artwork.preferred_type.as_str() {
                "box" => crate::artwork_scraper::ArtworkType::BoxArt,
                "screenshot" => crate::artwork_scraper::ArtworkType::Screenshot,
                "title" => crate::artwork_scraper::ArtworkType::TitleScreen,
                _ => crate::artwork_scraper::ArtworkType::CartridgeLabel,
            };
            Some(ArtworkDownloader::new(artwork_type, true))
        } else {
            None
        };
        
        Self {
            cache: RomCache::load(),
            artwork_downloader,
        }
    }
    
    /// Scan directories for ROMs
    pub fn scan_directories(&mut self, dirs: &[PathBuf]) -> Vec<RomEntry> {
        let mut entries = Vec::new();
        let mut scanned = 0;
        let mut cached = 0;
        let mut failed = 0;
        
        for dir in dirs {
            if !dir.exists() {
                log::warn!("ROM directory does not exist: {}", dir.display());
                continue;
            }
            
            log::info!("Scanning ROM directory: {}", dir.display());
            
            for entry in WalkDir::new(dir)
                .follow_links(true)
                .max_depth(10) // Limit recursion depth to avoid pathological cases
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                
                // Check for .nes or .zip files
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if ext_str == "nes" || ext_str == "zip" {
                        scanned += 1;
                        
                        // Log progress every 10 files
                        if scanned % 10 == 0 {
                            log::info!("Scanning progress: {} files processed...", scanned);
                        }
                        
                        // Check cache first
                        if !self.cache.needs_refresh(path) {
                            if let Some(cached_entry) = self.cache.entries.get(path) {
                                entries.push(cached_entry.clone());
                                cached += 1;
                                continue;
                            }
                        }
                        
                        // Parse ROM with detailed logging
                        log::debug!("Parsing ROM: {}", path.display());
                        match RomEntry::from_file(path.to_path_buf(), self.artwork_downloader.as_mut()) {
                            Ok(rom_entry) => {
                                log::debug!("Successfully scanned ROM: {}", rom_entry.title);
                                self.cache.entries.insert(path.to_path_buf(), rom_entry.clone());
                                entries.push(rom_entry);
                            }
                            Err(e) => {
                                log::warn!("Failed to parse ROM {}: {}", path.display(), e);
                                failed += 1;
                                // Continue to next file, don't let one bad ROM stop the scan
                            }
                        }
                    }
                }
            }
        }
        
        // Update scan timestamp
        self.cache.last_scan = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Save cache
        if let Err(e) = self.cache.save() {
            log::error!("Failed to save ROM cache: {}", e);
        }
        
        log::info!(
            "ROM scan complete: {} total ({} from cache, {} newly scanned, {} failed)",
            entries.len(),
            cached,
            scanned - cached - failed,
            failed
        );
        
        entries
    }
    
    /// Get cached entries without rescanning
    pub fn get_cached_entries(&self) -> Vec<RomEntry> {
        self.cache.entries.values().cloned().collect()
    }
}

