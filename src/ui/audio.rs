//! Audio output using cpal for cross-platform audio
//!
//! This module provides audio playback independent of SDL2, using cpal
//! for better cross-platform support with the egui-based UI.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

const APU_SAMPLE_RATE: f64 = 44_100.0; // NES APU outputs at this rate
const BUFFER_SIZE: usize = 16384; // Larger buffer for smoother playback

/// Audio output stream for the emulator
pub struct AudioOutput {
    _stream: cpal::Stream,
    sample_buffer: Arc<Mutex<RingBuffer>>,
    output_sample_rate: u32,
}

/// A proper ring buffer for audio samples with resampling support
struct RingBuffer {
    data: Vec<f32>,
    write_pos: usize,
    read_pos: usize,
    count: usize,
    volume: f32,
    muted: bool,
    // Resampling state
    resample_ratio: f64, // output_rate / input_rate
    resample_pos: f64,   // Fractional position in input buffer
    last_sample: f32,    // For linear interpolation
}

impl RingBuffer {
    fn new(capacity: usize, output_sample_rate: u32) -> Self {
        let resample_ratio = output_sample_rate as f64 / APU_SAMPLE_RATE;
        log::info!(
            "Audio initialized: APU rate {}Hz -> Output rate {}Hz (ratio: {:.6})",
            APU_SAMPLE_RATE as u32,
            output_sample_rate,
            resample_ratio
        );

        Self {
            data: vec![0.0; capacity],
            write_pos: 0,
            read_pos: 0,
            count: 0,
            volume: 0.7,
            muted: false,
            resample_ratio,
            resample_pos: 0.0,
            last_sample: 0.0,
        }
    }

    fn capacity(&self) -> usize {
        self.data.len()
    }

    fn available(&self) -> usize {
        self.count
    }

    fn write(&mut self, samples: &[f32]) {
        for &sample in samples {
            if self.count < self.capacity() {
                self.data[self.write_pos] = sample;
                self.write_pos = (self.write_pos + 1) % self.capacity();
                self.count += 1;
            }
            // Drop samples if buffer is full (overflow protection)
        }
    }

    /// Read samples with resampling from APU rate to output rate
    fn read_resampled(&mut self, output: &mut [f32]) {
        let vol = if self.muted { 0.0 } else { self.volume };

        // Input step: how much to advance in input buffer per output sample
        // For upsampling (44100->48000): ratio > 1, so step < 1 (consume slower)
        // For downsampling (44100->44100): ratio = 1, so step = 1 (1:1)
        let input_step = 1.0 / self.resample_ratio;

        for sample in output.iter_mut() {
            if self.count > 1 {
                // Linear interpolation between samples
                let int_pos = self.resample_pos.floor() as usize;
                let frac = self.resample_pos - int_pos as f64;

                // Get current and next sample indices (wrapping around buffer)
                let current_idx = (self.read_pos + int_pos) % self.capacity();
                let next_idx = (self.read_pos + int_pos + 1) % self.capacity();

                let current = self.data[current_idx];
                let next = self.data[next_idx];

                // Linear interpolation
                let interpolated = current + (next - current) * (frac as f32);
                *sample = interpolated * vol;

                // Advance position in input buffer
                self.resample_pos += input_step;

                // Consume input samples we've passed
                while self.resample_pos >= 1.0 && self.count > 0 {
                    self.resample_pos -= 1.0;
                    self.read_pos = (self.read_pos + 1) % self.capacity();
                    self.count -= 1;
                }
            } else if self.count == 1 {
                // Only one sample left - use it with last sample for interpolation
                let frac = self.resample_pos.min(1.0);
                let interpolated = if self.resample_pos < 1.0 {
                    // Interpolate between last and current
                    self.last_sample + (self.data[self.read_pos] - self.last_sample) * (frac as f32)
                } else {
                    // Use current sample
                    self.data[self.read_pos]
                };
                *sample = interpolated * vol;

                self.resample_pos += input_step;
                if self.resample_pos >= 1.0 {
                    self.last_sample = self.data[self.read_pos];
                    self.resample_pos -= 1.0;
                    self.read_pos = (self.read_pos + 1) % self.capacity();
                    self.count -= 1;
                }
            } else {
                // Buffer underrun - output silence to prevent audio glitches
                *sample = 0.0;
                // Reset resample position to prevent accumulation of errors
                self.resample_pos = 0.0;
            }
        }
    }
}

impl AudioOutput {
    /// Create a new audio output stream
    pub fn new() -> Option<Self> {
        let host = cpal::default_host();

        log::info!("Available audio hosts: {:?}", cpal::available_hosts());
        log::info!("Using audio host: {:?}", host.id());

        let device = host.default_output_device()?;
        log::info!(
            "Using audio device: {:?}",
            device.name().unwrap_or_default()
        );

        // Get the device's preferred config
        let supported_config = device.default_output_config().ok()?;
        log::info!("Supported audio config: {:?}", supported_config);

        let sample_format = supported_config.sample_format();
        let config: cpal::StreamConfig = supported_config.into();
        let output_sample_rate = config.sample_rate.0;

        log::info!(
            "Audio config: channels={}, sample_rate={}",
            config.channels,
            output_sample_rate
        );

        let sample_buffer = Arc::new(Mutex::new(RingBuffer::new(BUFFER_SIZE, output_sample_rate)));
        let buffer_clone = Arc::clone(&sample_buffer);
        let channels = config.channels as usize;

        // Build stream based on sample format
        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                device
                    .build_output_stream(
                        &config,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            if let Ok(mut buffer) = buffer_clone.lock() {
                                if channels == 2 {
                                    // Stereo: read mono and duplicate to both channels
                                    let mono_samples = data.len() / 2;
                                    let mut mono = vec![0.0f32; mono_samples];
                                    buffer.read_resampled(&mut mono);
                                    for (i, chunk) in data.chunks_mut(2).enumerate() {
                                        chunk[0] = mono[i];
                                        chunk[1] = mono[i];
                                    }
                                } else {
                                    buffer.read_resampled(data);
                                }
                            }
                        },
                        |err| {
                            log::error!("Audio stream error: {}", err);
                        },
                        None,
                    )
                    .ok()?
            }
            cpal::SampleFormat::I16 => {
                let buffer_clone = Arc::clone(&sample_buffer);
                device
                    .build_output_stream(
                        &config,
                        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                            if let Ok(mut buffer) = buffer_clone.lock() {
                                let sample_count = if channels == 2 {
                                    data.len() / 2
                                } else {
                                    data.len()
                                };
                                let mut temp = vec![0.0f32; sample_count];
                                buffer.read_resampled(&mut temp);

                                if channels == 2 {
                                    for (i, chunk) in data.chunks_mut(2).enumerate() {
                                        let sample = (temp[i] * 32767.0) as i16;
                                        chunk[0] = sample;
                                        chunk[1] = sample;
                                    }
                                } else {
                                    for (out, &sample) in data.iter_mut().zip(temp.iter()) {
                                        *out = (sample * 32767.0) as i16;
                                    }
                                }
                            }
                        },
                        |err| {
                            log::error!("Audio stream error: {}", err);
                        },
                        None,
                    )
                    .ok()?
            }
            _ => {
                log::error!("Unsupported sample format: {:?}", sample_format);
                return None;
            }
        };

        stream.play().ok()?;
        log::info!("Audio stream started successfully");

        Some(Self {
            _stream: stream,
            sample_buffer,
            output_sample_rate,
        })
    }

    /// Queue audio samples for playback
    pub fn queue_samples(&self, samples: &[f32]) {
        if let Ok(mut buffer) = self.sample_buffer.lock() {
            buffer.write(samples);
        }
    }

    /// Set the audio volume (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) {
        if let Ok(mut buffer) = self.sample_buffer.lock() {
            buffer.volume = volume.clamp(0.0, 1.0);
        }
    }

    /// Set muted state
    pub fn set_muted(&self, muted: bool) {
        if let Ok(mut buffer) = self.sample_buffer.lock() {
            buffer.muted = muted;
        }
    }

    /// Get the number of samples currently queued
    pub fn queued_samples(&self) -> usize {
        if let Ok(buffer) = self.sample_buffer.lock() {
            buffer.available()
        } else {
            0
        }
    }

    /// Get target queue size for sync
    /// Target is about 2 frames worth of audio at output sample rate
    pub fn target_queue_size(&self) -> usize {
        // 2 frames at 60fps = ~33.33ms of audio
        // At 44100Hz: ~1470 samples, at 48000Hz: ~1600 samples
        // Use a rate-appropriate target
        (self.output_sample_rate as f64 / 60.0 * 2.0) as usize
    }

    /// Get the output sample rate
    #[allow(dead_code)]
    pub fn sample_rate(&self) -> u32 {
        self.output_sample_rate
    }
}
