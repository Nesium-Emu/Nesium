//! Artwork scraper for ROM thumbnails
//!
//! Downloads game artwork from online sources (box art, cartridge photos, screenshots)
//! Falls back to CHR ROM extraction if online sources unavailable

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Artwork source
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArtworkSource {
    /// NESFiles.com - cartridge and box photos
    NESFiles,
    /// TheGamesDB.net - general game database
    TheGamesDB,
    /// ScreenScraper.fr - comprehensive ROM database
    ScreenScraper,
    /// Local CHR ROM extraction (fallback)
    CHRExtraction,
}

/// Artwork type preference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtworkType {
    /// Game box/cover art
    BoxArt,
    /// Cartridge label photo
    CartridgeLabel,
    /// In-game screenshot
    Screenshot,
    /// Title screen
    TitleScreen,
}

/// Artwork metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtworkMetadata {
    /// Source URL
    pub url: String,

    /// Source site
    pub source: String,

    /// Artwork type
    pub artwork_type: ArtworkType,

    /// Download timestamp
    pub downloaded: u64,

    /// Local cache path
    pub cache_path: PathBuf,
}

/// Artwork cache
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ArtworkCache {
    /// Cached artwork by ROM path
    pub entries: HashMap<PathBuf, ArtworkMetadata>,
}

impl ArtworkCache {
    /// Get cache file path
    pub fn cache_path() -> PathBuf {
        if let Some(cache_dir) = dirs::cache_dir() {
            let nesium_dir = cache_dir.join("nesium");
            fs::create_dir_all(&nesium_dir).ok();
            nesium_dir.join("artwork_cache.json")
        } else {
            PathBuf::from("artwork_cache.json")
        }
    }

    /// Load cache from disk
    pub fn load() -> Self {
        let path = Self::cache_path();

        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(contents) => match serde_json::from_str(&contents) {
                    Ok(cache) => {
                        log::info!("Loaded artwork cache from: {}", path.display());
                        return cache;
                    }
                    Err(e) => {
                        log::error!("Failed to parse artwork cache: {}", e);
                    }
                },
                Err(e) => {
                    log::error!("Failed to read artwork cache: {}", e);
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
        log::info!("Saved artwork cache to: {}", path.display());
        Ok(())
    }
}

/// Artwork downloader
pub struct ArtworkDownloader {
    cache: ArtworkCache,
    preferred_type: ArtworkType,
    enable_online: bool,
}

impl ArtworkDownloader {
    /// Create new downloader
    pub fn new(preferred_type: ArtworkType, enable_online: bool) -> Self {
        Self {
            cache: ArtworkCache::load(),
            preferred_type,
            enable_online,
        }
    }

    /// Download artwork for ROM (blocking version for use in sync context)
    /// Returns path to cached image, or None if unavailable
    pub fn download_artwork(&mut self, rom_path: &Path, rom_title: &str) -> Option<PathBuf> {
        // Check cache first
        if let Some(metadata) = self.cache.entries.get(rom_path) {
            if metadata.cache_path.exists() {
                log::debug!("Using cached artwork for: {}", rom_title);
                return Some(metadata.cache_path.clone());
            }
        }

        // If online disabled, return None (caller will use CHR extraction)
        if !self.enable_online {
            return None;
        }

        // Try to download from sources
        log::info!("Downloading artwork for: {}", rom_title);

        // Try ScreenScraper first (most reliable)
        if let Some(image_url) = self.search_screenscraper(rom_title) {
            if let Ok(cache_path) = self.download_and_cache_image(&image_url, rom_path, rom_title) {
                return Some(cache_path);
            }
        }

        // Fallback: Try NESFiles.com
        if let Some(urls) = self.search_nesfiles(rom_title) {
            for url in urls {
                if let Ok(cache_path) = self.download_and_cache_image(&url, rom_path, rom_title) {
                    return Some(cache_path);
                }
            }
        }

        // No online artwork found
        None
    }

    /// Search ScreenScraper for game artwork
    /// Returns image URL if found
    fn search_screenscraper(&self, title: &str) -> Option<String> {
        // ScreenScraper API endpoint (simplified - no auth for basic search)
        // Note: For production use, you should register and use API keys
        let normalized_title = Self::normalize_title(title);

        // Try to fetch from ScreenScraper's public database
        let _search_url = format!(
            "https://www.screenscraper.fr/api2/jeuInfos.php?devid=nesium&softname=nesium&romnom={}",
            urlencoding::encode(&normalized_title)
        );

        log::debug!("Searching ScreenScraper for: {}", normalized_title);

        // For now, return None (requires proper API setup)
        // TODO: Implement with proper API key
        None
    }

    /// Search NESFiles.com for game artwork using predictable URL patterns
    fn search_nesfiles(&self, title: &str) -> Option<Vec<String>> {
        // Quick skip for obvious non-NESFiles games
        if Self::should_skip_online_search(title) {
            log::debug!(
                "Skipping online search for: {} (multicart/pirate/hack)",
                title
            );
            return None;
        }

        let normalized_title = Self::normalize_title(title);

        // Convert to NESFiles URL format (spaces to underscores, proper casing)
        let url_title = normalized_title.replace(' ', "_");

        log::debug!(
            "Searching NESFiles for: {} (URL: {})",
            normalized_title,
            url_title
        );

        let base_url = format!("https://www.nesfiles.com/NES/{}", url_title);

        // Try predictable image patterns in order of preference
        let mut image_urls = Vec::new();

        // Try URLs in order, but stop after first success (fast!)
        // 1. Cartridge thumbnail (best for ROM browser)
        let cart_url = format!("{}/{}_thumb_cart.jpg", base_url, url_title);
        if Self::url_exists(&cart_url) {
            log::info!("✓ Found cartridge: {}", normalized_title);
            return Some(vec![cart_url]); // Stop here, we got what we want!
        }

        // 2. Title screen GIF (animated, nice for preview)
        let title_gif = format!("{}/title.gif", base_url);
        if Self::url_exists(&title_gif) {
            log::info!("✓ Found title screen: {}", normalized_title);
            return Some(vec![title_gif]);
        }

        // 3. Alternative title screen format (some games use {name}title.gif)
        let alt_title = format!(
            "{}/{}title.gif",
            base_url,
            url_title.to_lowercase().replace('_', "")
        );
        if Self::url_exists(&alt_title) {
            log::info!("✓ Found alt title: {}", normalized_title);
            return Some(vec![alt_title]);
        }

        // Only check these if nothing else found (less common)
        // 4. Manual thumbnail (fallback)
        let manual_url = format!("{}/{}_thumb_manual.jpg", base_url, url_title);
        if Self::url_exists(&manual_url) {
            log::info!("✓ Found manual: {}", normalized_title);
            image_urls.push(manual_url);
        }

        // 5. Box image (try just one pattern)
        let box_url = format!("{}/{}_thumb_box.jpg", base_url, url_title);
        if Self::url_exists(&box_url) {
            log::info!("✓ Found box: {}", normalized_title);
            image_urls.push(box_url);
        }

        if image_urls.is_empty() {
            log::debug!("✗ No artwork: {}", normalized_title); // Changed to debug level
            None
        } else {
            Some(image_urls)
        }
    }

    /// Check if a URL exists (HEAD request with short timeout)
    fn url_exists(url: &str) -> bool {
        match reqwest::blocking::Client::new()
            .head(url)
            .timeout(std::time::Duration::from_secs(1)) // Reduced from 5 to 1 second
            .send()
        {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    /// Quick heuristic: Skip obvious non-NESFiles games
    fn should_skip_online_search(title: &str) -> bool {
        let lower = title.to_lowercase();

        // Skip multicarts (not on NESFiles)
        if lower.contains("-in-1") || lower.contains(" in 1") {
            return true;
        }

        // Skip unlicensed/pirate markers
        if lower.contains("[p1]")
            || lower.contains("[p2]")
            || lower.contains("(pirate)")
            || lower.contains("(unl)")
        {
            return true;
        }

        // Skip hacked/modified ROMs
        if lower.contains("[h]")
            || lower.contains("(hack)")
            || lower.contains("(mod)")
            || lower.contains("[t]")
        {
            return true;
        }

        false
    }

    /// Download and cache image
    fn download_and_cache_image(
        &mut self,
        url: &str,
        rom_path: &Path,
        rom_title: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        log::debug!("Downloading: {}", url);

        // Download image with timeout
        let response = reqwest::blocking::Client::new()
            .get(url)
            .timeout(std::time::Duration::from_secs(10))
            .send()?;
        if !response.status().is_success() {
            return Err("HTTP request failed".into());
        }

        let bytes = response.bytes()?;

        // Load image
        let img = image::load_from_memory(&bytes)?;

        // Resize to 64x64 (thumbnail size)
        let thumbnail = img.resize_exact(64, 64, image::imageops::FilterType::Lanczos3);

        // Create cache directory
        let cache_dir = Self::artwork_cache_dir();
        fs::create_dir_all(&cache_dir)?;

        // Generate cache filename
        let filename = format!(
            "{}.png",
            rom_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .replace(|c: char| !c.is_alphanumeric(), "_")
        );
        let cache_path = cache_dir.join(filename);

        // Save thumbnail
        thumbnail.save(&cache_path)?;

        // Update cache metadata
        let metadata = ArtworkMetadata {
            url: url.to_string(),
            source: if url.contains("nesfiles.com") {
                "NESFiles.com".to_string()
            } else if url.contains("screenscraper") {
                "ScreenScraper.fr".to_string()
            } else {
                "Unknown".to_string()
            },
            artwork_type: self.preferred_type,
            downloaded: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            cache_path: cache_path.clone(),
        };

        self.cache.entries.insert(rom_path.to_path_buf(), metadata);
        let _ = self.cache.save();

        log::info!("✓ Cached: {}", rom_title);

        // Brief delay to be respectful to server (only on success)
        std::thread::sleep(std::time::Duration::from_millis(100));

        Ok(cache_path)
    }

    /// Get artwork cache directory
    fn artwork_cache_dir() -> PathBuf {
        if let Some(cache_dir) = dirs::cache_dir() {
            let nesium_dir = cache_dir.join("nesium").join("artwork");
            fs::create_dir_all(&nesium_dir).ok();
            nesium_dir
        } else {
            PathBuf::from("artwork_cache")
        }
    }

    /// Normalize game title for searching
    /// Converts "Super Mario Bros (U) [!]" to "Super Mario Bros"
    fn normalize_title(title: &str) -> String {
        let cleaned = title
            .trim()
            // Remove region markers
            .replace("(U)", "")
            .replace("(E)", "")
            .replace("(J)", "")
            .replace("(USA)", "")
            .replace("(Europe)", "")
            .replace("(Japan)", "")
            .replace("(World)", "")
            // Remove special markers
            .replace("[!]", "")
            .replace("[b]", "")
            .replace("[o]", "")
            .replace("[a]", "")
            .replace("[h]", "")
            .replace("[f]", "")
            .replace("[t]", "")
            // Remove version numbers
            .replace("(Rev A)", "")
            .replace("(Rev B)", "")
            .replace("(Rev 1)", "")
            .replace("(Rev 2)", "")
            // Remove parentheses and brackets (now empty)
            .replace("()", "")
            .replace("[]", "")
            // Clean up whitespace
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        // Title case for better matching (NESFiles uses proper casing)
        Self::to_title_case(&cleaned)
    }

    /// Convert to Title Case (first letter of each word capitalized)
    fn to_title_case(s: &str) -> String {
        s.split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => {
                        // Keep acronyms and special words as-is
                        if word.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) {
                            word.to_string()
                        } else {
                            first.to_uppercase().collect::<String>()
                                + &chars.as_str().to_lowercase()
                        }
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

// URL encoding helper
mod urlencoding {
    pub fn encode(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                    c.to_string()
                } else {
                    format!("%{:02X}", c as u8)
                }
            })
            .collect()
    }
}

// Note: This is a framework for future implementation
// Current version still uses CHR ROM extraction as primary method
// To enable online artwork:
// 1. Add dependencies: reqwest, scraper, image
// 2. Implement scraping logic for NESFiles.com
// 3. Add user option in settings to enable/disable online downloads
// 4. Add rate limiting and caching
// 5. Respect robots.txt and site terms of service
