use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::render::Canvas;
use sdl2::video::Window;
use sdl2::Sdl;

const AUDIO_SAMPLE_RATE: i32 = 44_100;
const AUDIO_BUFFER_SIZE: usize = 512;   // Queue this many samples at a time
const MIN_QUEUE_SAMPLES: usize = 512;   // Start playback with this many samples queued

pub struct Renderer {
    canvas: Canvas<Window>,
    texture_creator: sdl2::render::TextureCreator<sdl2::video::WindowContext>,
    sdl: Sdl,
    audio_device: Option<AudioQueue<f32>>,  // Use f32 directly to avoid conversion issues
    audio_buffer: Vec<f32>,
    audio_started: bool,
}

// NES 2C02 PPU palette (RGB values) - 64 colors
// Matches default 2C02 PPU palette for accurate color reproduction
// Reference: https://www.nesdev.org/wiki/PPU_palettes
const NES_PALETTE: [[u8; 3]; 64] = [
    // Row 0 ($00-$0F) - Darkest luminance level
    [0x66, 0x66, 0x66], [0x00, 0x2A, 0x88], [0x14, 0x12, 0xA7], [0x3B, 0x00, 0xA4],
    [0x5C, 0x00, 0x7E], [0x6E, 0x00, 0x40], [0x6C, 0x06, 0x00], [0x56, 0x1D, 0x00],
    [0x33, 0x35, 0x00], [0x0B, 0x48, 0x00], [0x00, 0x52, 0x00], [0x00, 0x4F, 0x08],
    [0x00, 0x40, 0x4D], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],
    // Row 1 ($10-$1F) - Medium-dark luminance level
    [0xAD, 0xAD, 0xAD], [0x15, 0x5F, 0xD9], [0x42, 0x40, 0xFF], [0x75, 0x27, 0xFE],
    [0xA0, 0x1A, 0xCC], [0xB7, 0x1E, 0x7B], [0xB5, 0x31, 0x20], [0x99, 0x4E, 0x00],
    [0x6B, 0x6D, 0x00], [0x38, 0x87, 0x00], [0x0C, 0x93, 0x00], [0x00, 0x8F, 0x32],
    [0x00, 0x7C, 0x8D], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],
    // Row 2 ($20-$2F) - Medium-bright luminance level
    [0xFF, 0xFE, 0xFF], [0x64, 0xB0, 0xFF], [0x92, 0x90, 0xFF], [0xC6, 0x76, 0xFF],
    [0xF3, 0x6A, 0xFF], [0xFE, 0x6E, 0xCC], [0xFE, 0x81, 0x70], [0xEA, 0x9E, 0x22],
    [0xBC, 0xBE, 0x00], [0x88, 0xD8, 0x00], [0x5C, 0xE4, 0x30], [0x45, 0xE0, 0x82],
    [0x48, 0xCD, 0xDE], [0x4F, 0x4F, 0x4F], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],
    // Row 3 ($30-$3F) - Brightest luminance level
    [0xFF, 0xFE, 0xFF], [0xC0, 0xDF, 0xFF], [0xD3, 0xD2, 0xFF], [0xE8, 0xC8, 0xFF],
    [0xFB, 0xC2, 0xFF], [0xFE, 0xC4, 0xEA], [0xFE, 0xCC, 0xC5], [0xF7, 0xD8, 0xA5],
    [0xE4, 0xE5, 0x94], [0xCF, 0xEF, 0x96], [0xBD, 0xF4, 0xAB], [0xB3, 0xF3, 0xCC],
    [0xB5, 0xEB, 0xF2], [0xB8, 0xB8, 0xB8], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],
];

impl Renderer {
    pub fn new() -> Result<Self, String> {
        let sdl = sdl2::init()?;
        let video_subsystem = sdl.video()?;
        let audio_subsystem = sdl.audio()?;

        let window = video_subsystem
            .window("NES Emulator", 512, 480)
            .position_centered()
            .build()
            .map_err(|e| e.to_string())?;

        let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;
        canvas.set_draw_color(Color::RGB(0, 0, 0));
        canvas.clear();

        let texture_creator = canvas.texture_creator();

        // Set up audio device with f32 samples
        let audio_spec = AudioSpecDesired {
            freq: Some(AUDIO_SAMPLE_RATE),
            channels: Some(1), // Mono
            samples: Some(1024), // SDL buffer size
        };
        
        let audio_device = match audio_subsystem.open_queue::<f32, _>(None, &audio_spec) {
            Ok(device) => {
                // Start playback immediately - SDL will handle underruns with silence
                device.resume();
                log::info!("Audio initialized: {} Hz, mono, f32 format", AUDIO_SAMPLE_RATE);
                Some(device)
            }
            Err(e) => {
                log::warn!("Failed to initialize audio: {}", e);
                None
            }
        };

        Ok(Self {
            canvas,
            texture_creator,
            sdl,
            audio_device,
            audio_buffer: Vec::with_capacity(2048),
            audio_started: true, // We start immediately now
        })
    }
    
    pub fn get_sdl_context(&self) -> &Sdl {
        &self.sdl
    }

    pub fn render_frame(&mut self, framebuffer: &[u8]) {
        // Convert palette indices to RGB
        let mut rgb_buffer = Vec::with_capacity(256 * 240 * 3);
        for &palette_idx in framebuffer.iter().take(256 * 240) {
            let palette_idx = palette_idx & 0x3F;
            let rgb = NES_PALETTE[palette_idx as usize];
            rgb_buffer.push(rgb[0]);
            rgb_buffer.push(rgb[1]);
            rgb_buffer.push(rgb[2]);
        }

        let mut texture = self.texture_creator
            .create_texture_streaming(PixelFormatEnum::RGB24, 256, 240)
            .expect("Failed to create streaming texture");
        
        texture.with_lock(None, |buffer: &mut [u8], _pitch: usize| {
            for y in 0..240 {
                for x in 0..256 {
                    let idx = (y * 256 + x) * 3;
                    let src_idx = (y * 256 + x) * 3;
                    if idx + 2 < buffer.len() && src_idx + 2 < rgb_buffer.len() {
                        buffer[idx] = rgb_buffer[src_idx];
                        buffer[idx + 1] = rgb_buffer[src_idx + 1];
                        buffer[idx + 2] = rgb_buffer[src_idx + 2];
                    }
                }
            }
        }).expect("Failed to lock texture");

        self.canvas.clear();
        self.canvas
            .copy(&texture, None, None)
            .expect("Failed to copy texture");
        
        self.canvas.present();
    }

    pub fn queue_audio_samples(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        
        // Add samples to our buffer
        self.audio_buffer.extend_from_slice(samples);
        
        // Queue to SDL when we have enough samples
        if let Some(ref device) = self.audio_device {
            if self.audio_buffer.len() >= AUDIO_BUFFER_SIZE {
                // Queue all buffered samples
                if let Err(e) = device.queue_audio(&self.audio_buffer) {
                    log::warn!("Failed to queue audio: {}", e);
                }
                self.audio_buffer.clear();
            }
        }
    }
    
    pub fn flush_audio(&mut self) {
        if !self.audio_buffer.is_empty() {
            if let Some(ref device) = self.audio_device {
                device.queue_audio(&self.audio_buffer).ok();
            }
            self.audio_buffer.clear();
        }
    }
    
    // Get the current audio queue size in samples
    pub fn get_audio_queue_size(&self) -> usize {
        if let Some(ref device) = self.audio_device {
            // size() returns bytes, f32 = 4 bytes per sample
            device.size() as usize / 4
        } else {
            0
        }
    }
    
    pub fn get_target_queue_size(&self) -> usize {
        // Target about 2 frames worth of audio (~1500 samples at 60fps)
        1500
    }
}
