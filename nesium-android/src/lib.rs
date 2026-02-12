//! Nesium Android JNI Bindings
//!
//! This crate provides JNI bindings to expose the Nesium NES emulator core
//! to Android (Kotlin/Java) applications.

use std::panic;
use std::sync::Mutex;

use jni::objects::{JByteArray, JClass, JIntArray, JString};
use jni::sys::{jboolean, jfloatArray, jint, JNI_TRUE};
use jni::JNIEnv;

use nesium::cartridge::Cartridge;
use nesium::cpu::{Cpu, CpuBus, FLAG_I};
use nesium::memory::MemoryBus;
use nesium::trace::TraceState;
use nesium::{CPU_CYCLES_PER_PPU_CYCLE, NES_HEIGHT, NES_PALETTE, NES_WIDTH, PPU_CYCLES_PER_FRAME};

/// NES emulator state
struct NesEmulator {
    cpu: Cpu,
    memory: MemoryBus,
    trace: TraceState,
    ppu_cycles_this_frame: u64,
    cpu_cycle_accumulator: f64,
}

impl NesEmulator {
    fn new(cartridge: Cartridge) -> Self {
        let mut memory = MemoryBus::new(cartridge);
        let mut cpu = Cpu::new();
        cpu.reset(&mut memory as &mut dyn CpuBus);

        log::info!("NES initialized: PC=0x{:04X}", cpu.pc);

        Self {
            cpu,
            memory,
            trace: TraceState::new(false),
            ppu_cycles_this_frame: 0,
            cpu_cycle_accumulator: 0.0,
        }
    }

    /// Run emulation for one frame (same logic as desktop EmulationState::step_frame)
    fn step_frame(&mut self) {
        self.ppu_cycles_this_frame = 0;

        while self.ppu_cycles_this_frame < PPU_CYCLES_PER_FRAME {
            let nmi_triggered = self.memory.step_ppu();

            if nmi_triggered {
                self.cpu.trigger_nmi(&mut self.memory as &mut dyn CpuBus);
            }

            if self.memory.mapper_irq_pending() && (self.cpu.status & FLAG_I) == 0 {
                self.memory.acknowledge_mapper_irq();
                self.cpu.trigger_irq(&mut self.memory as &mut dyn CpuBus);
            }

            self.cpu_cycle_accumulator += CPU_CYCLES_PER_PPU_CYCLE;

            while self.cpu_cycle_accumulator >= 1.0 {
                let cpu_cycles = self
                    .cpu
                    .step(&mut self.memory as &mut dyn CpuBus, &mut self.trace);
                self.cpu_cycle_accumulator -= cpu_cycles as f64;

                let irq = self.memory.step_apu(cpu_cycles as u64);
                if irq && (self.cpu.status & FLAG_I) == 0 {
                    self.cpu.trigger_irq(&mut self.memory as &mut dyn CpuBus);
                }
            }

            self.ppu_cycles_this_frame += 1;
        }
    }

    /// Get the PPU framebuffer (palette indices)
    fn framebuffer(&self) -> &[u8] {
        &self.memory.ppu.framebuffer
    }

    /// Get audio samples from the APU
    fn take_audio_samples(&mut self) -> Vec<f32> {
        self.memory.apu.take_samples()
    }

    /// Press a controller button
    fn press_button(&mut self, button: NesButton) {
        match button {
            NesButton::A => self.memory.input.controller1.a = true,
            NesButton::B => self.memory.input.controller1.b = true,
            NesButton::Select => self.memory.input.controller1.select = true,
            NesButton::Start => self.memory.input.controller1.start = true,
            NesButton::Up => self.memory.input.controller1.up = true,
            NesButton::Down => self.memory.input.controller1.down = true,
            NesButton::Left => self.memory.input.controller1.left = true,
            NesButton::Right => self.memory.input.controller1.right = true,
        }
    }

    /// Release a controller button
    fn release_button(&mut self, button: NesButton) {
        match button {
            NesButton::A => self.memory.input.controller1.a = false,
            NesButton::B => self.memory.input.controller1.b = false,
            NesButton::Select => self.memory.input.controller1.select = false,
            NesButton::Start => self.memory.input.controller1.start = false,
            NesButton::Up => self.memory.input.controller1.up = false,
            NesButton::Down => self.memory.input.controller1.down = false,
            NesButton::Left => self.memory.input.controller1.left = false,
            NesButton::Right => self.memory.input.controller1.right = false,
        }
    }
}

/// NES controller buttons
#[derive(Debug, Clone, Copy)]
enum NesButton {
    A,
    B,
    Select,
    Start,
    Right,
    Left,
    Up,
    Down,
}

fn int_to_button(button: jint) -> Option<NesButton> {
    match button {
        0 => Some(NesButton::A),
        1 => Some(NesButton::B),
        2 => Some(NesButton::Select),
        3 => Some(NesButton::Start),
        4 => Some(NesButton::Right),
        5 => Some(NesButton::Left),
        6 => Some(NesButton::Up),
        7 => Some(NesButton::Down),
        _ => None,
    }
}

/// Global emulator instance protected by a mutex
static EMULATOR: Mutex<Option<NesEmulator>> = Mutex::new(None);

// ============================================================================
// JNI Functions
// ============================================================================

/// Initialize Android logging
#[no_mangle]
pub extern "system" fn Java_com_nesium_NesiumCore_initLogging(_env: JNIEnv, _class: JClass) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("Nesium"),
    );
    log::info!("Nesium Android logging initialized");
}

/// Load a ROM from bytes - preferred method on Android
#[no_mangle]
pub extern "system" fn Java_com_nesium_NesiumCore_loadRomFromBytes<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    rom_data: JByteArray<'local>,
) -> jboolean {
    log::info!("loadRomFromBytes called");

    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        load_rom_from_bytes_inner(env, rom_data)
    }));

    match result {
        Ok(success) => success,
        Err(e) => {
            log::error!("Panic in loadRomFromBytes: {:?}", e);
            0
        }
    }
}

fn load_rom_from_bytes_inner(env: JNIEnv, rom_data: JByteArray) -> jboolean {
    let rom_bytes: Vec<u8> = match env.convert_byte_array(rom_data) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::error!("Failed to convert ROM bytes: {}", e);
            return 0;
        }
    };

    log::info!("ROM bytes received: {} bytes", rom_bytes.len());

    if rom_bytes.len() < 16 {
        log::error!("ROM too small: {} bytes", rom_bytes.len());
        return 0;
    }

    match Cartridge::load_from_bytes(rom_bytes) {
        Ok(cartridge) => {
            log::info!(
                "Cartridge loaded: mapper={}, prg={}KB, chr={}KB",
                cartridge.mapper_id,
                cartridge.prg_rom.len() / 1024,
                cartridge.chr_rom.len() / 1024
            );

            match EMULATOR.lock() {
                Ok(mut emu) => {
                    *emu = Some(NesEmulator::new(cartridge));
                    log::info!("ROM loaded and NES emulator initialized");
                    JNI_TRUE as jboolean
                }
                Err(e) => {
                    log::error!("Failed to lock emulator mutex: {}", e);
                    0
                }
            }
        }
        Err(e) => {
            log::error!("Failed to load ROM: {}", e);
            0
        }
    }
}

/// Load a ROM from a file path
#[no_mangle]
pub extern "system" fn Java_com_nesium_NesiumCore_loadRomFromPath<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
) -> jboolean {
    log::info!("loadRomFromPath called");

    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        load_rom_from_path_inner(&mut env, &path)
    }));

    match result {
        Ok(success) => success,
        Err(e) => {
            log::error!("Panic in loadRomFromPath: {:?}", e);
            0
        }
    }
}

fn load_rom_from_path_inner(env: &mut JNIEnv, path: &JString) -> jboolean {
    let path_str: String = match env.get_string(path) {
        Ok(s) => s.into(),
        Err(e) => {
            log::error!("Failed to get path string: {}", e);
            return 0;
        }
    };

    log::info!("Loading ROM from path: {}", path_str);

    match Cartridge::load(&path_str) {
        Ok(cartridge) => match EMULATOR.lock() {
            Ok(mut emu) => {
                *emu = Some(NesEmulator::new(cartridge));
                log::info!("ROM loaded from path successfully");
                JNI_TRUE as jboolean
            }
            Err(e) => {
                log::error!("Failed to lock emulator mutex: {}", e);
                0
            }
        },
        Err(e) => {
            log::error!("Failed to load ROM from path: {}", e);
            0
        }
    }
}

/// Run emulation for one frame and return the framebuffer as ARGB
#[no_mangle]
pub extern "system" fn Java_com_nesium_NesiumCore_runFrame<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    framebuffer: JIntArray<'local>,
) -> jint {
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        run_frame_inner(env, framebuffer)
    }));

    match result {
        Ok(cycles) => cycles,
        Err(e) => {
            log::error!("Panic in runFrame: {:?}", e);
            0
        }
    }
}

fn run_frame_inner(env: JNIEnv, framebuffer: JIntArray) -> jint {
    let mut emu = match EMULATOR.lock() {
        Ok(e) => e,
        Err(e) => {
            log::error!("Failed to lock emulator: {}", e);
            return -1;
        }
    };

    if let Some(ref mut nes) = *emu {
        // Run one frame
        nes.step_frame();

        // Get the PPU framebuffer (palette indices)
        let fb = nes.framebuffer();

        // Convert palette indices to ARGB8888 for Android Bitmap
        let pixel_count = NES_WIDTH * NES_HEIGHT;
        let mut argb_buffer = vec![0i32; pixel_count];

        for i in 0..pixel_count {
            let palette_idx = (fb[i] as usize) & 0x3F; // Mask to 64 colors
            let [r, g, b] = NES_PALETTE[palette_idx];
            // ARGB format for Android
            argb_buffer[i] =
                ((0xFF_u32 << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)) as i32;
        }

        if let Err(e) = env.set_int_array_region(&framebuffer, 0, &argb_buffer) {
            log::error!("Failed to set framebuffer: {}", e);
            return -2;
        }

        // Log periodically
        static FRAME_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let frame = FRAME_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if frame % 60 == 0 {
            let mut color_counts = std::collections::HashMap::new();
            for i in 0..pixel_count {
                *color_counts.entry(fb[i]).or_insert(0) += 1;
            }
            log::info!("Frame {}: unique_colors={}", frame, color_counts.len());
        }

        1 // Success
    } else {
        log::warn!("runFrame called but no ROM loaded");
        -3
    }
}

/// Get audio samples from the emulator
#[no_mangle]
pub extern "system" fn Java_com_nesium_NesiumCore_getAudioSamples<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    samples: jfloatArray,
) -> jint {
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        get_audio_samples_inner(env, samples)
    }));

    match result {
        Ok(count) => count,
        Err(e) => {
            log::error!("Panic in getAudioSamples: {:?}", e);
            0
        }
    }
}

fn get_audio_samples_inner(env: JNIEnv, samples: jfloatArray) -> jint {
    let mut emu = match EMULATOR.lock() {
        Ok(e) => e,
        Err(_) => return 0,
    };

    if let Some(ref mut nes) = *emu {
        let audio_samples = nes.take_audio_samples();
        let count = audio_samples.len().min(4096);

        if count > 0 {
            if let Err(e) = unsafe {
                env.set_float_array_region(
                    &jni::objects::JFloatArray::from_raw(samples),
                    0,
                    &audio_samples[..count],
                )
            } {
                log::error!("Failed to set audio samples: {}", e);
                return 0;
            }
        }

        count as jint
    } else {
        0
    }
}

/// Press a button
#[no_mangle]
pub extern "system" fn Java_com_nesium_NesiumCore_pressButton(
    _env: JNIEnv,
    _class: JClass,
    button: jint,
) {
    if let Ok(mut emu) = EMULATOR.lock() {
        if let Some(ref mut nes) = *emu {
            if let Some(btn) = int_to_button(button) {
                nes.press_button(btn);
            }
        }
    }
}

/// Release a button
#[no_mangle]
pub extern "system" fn Java_com_nesium_NesiumCore_releaseButton(
    _env: JNIEnv,
    _class: JClass,
    button: jint,
) {
    if let Ok(mut emu) = EMULATOR.lock() {
        if let Some(ref mut nes) = *emu {
            if let Some(btn) = int_to_button(button) {
                nes.release_button(btn);
            }
        }
    }
}

/// Check if a ROM is loaded
#[no_mangle]
pub extern "system" fn Java_com_nesium_NesiumCore_isRomLoaded(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if let Ok(emu) = EMULATOR.lock() {
        if emu.is_some() {
            JNI_TRUE as jboolean
        } else {
            0
        }
    } else {
        0
    }
}

/// Unload the current ROM
#[no_mangle]
pub extern "system" fn Java_com_nesium_NesiumCore_unloadRom(_env: JNIEnv, _class: JClass) {
    if let Ok(mut emu) = EMULATOR.lock() {
        *emu = None;
        log::info!("ROM unloaded");
    }
}
