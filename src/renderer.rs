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

// Canonical NES palette (RGB values) - 64 colors
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
