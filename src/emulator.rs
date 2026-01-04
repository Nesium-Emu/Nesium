use crate::cpu::Cpu;
use crate::memory::MemoryBus;
use crate::renderer::Renderer;
use crate::trace::TraceState;
use log::debug;

// NTSC: 262 scanlines × 341 PPU cycles = 89,342 PPU cycles per frame
// CPU runs at 1/3 PPU speed: 89,342 / 3 = 29,780.67 CPU cycles per frame
// Master clock: PPU runs at ~5.37 MHz, CPU at ~1.79 MHz (exactly 1/3)
const PPU_CYCLES_PER_FRAME: u64 = 89_342; // 262 scanlines × 341 cycles
const CPU_CYCLES_PER_PPU_CYCLE: f64 = 1.0 / 3.0; // CPU runs at 1/3 PPU speed

pub struct Emulator {
    cpu: Cpu,
    memory: MemoryBus,
    renderer: Renderer,
    ppu_cycles_this_frame: u64,
    cpu_cycle_accumulator: f64, // Fractional accumulator for precise timing
    frame_count: u64,
    last_frame_time: std::time::Instant,
    fps_counter: u32,
    fps: f32,
    audio_sample_accumulator: f64, // For audio sampling
    trace: TraceState,
    total_ppu_cycles: u64, // Total PPU cycles since reset (for CYC counter)
}

impl Emulator {
    pub fn new(cartridge: crate::cartridge::Cartridge, trace: bool) -> Result<Self, String> {
        let mut memory = MemoryBus::new(cartridge);
        let mut cpu = Cpu::new();
        cpu.reset(&mut memory as &mut dyn crate::cpu::CpuBus);
        let renderer = Renderer::new()?;
        let trace_state = TraceState::new(trace);

        Ok(Self {
            cpu,
            memory,
            renderer,
            ppu_cycles_this_frame: 0,
            cpu_cycle_accumulator: 0.0,
            frame_count: 0,
            last_frame_time: std::time::Instant::now(),
            fps_counter: 0,
            fps: 60.0,
            audio_sample_accumulator: 0.0,
            trace: trace_state,
            total_ppu_cycles: 0,
        })
    }

    pub fn step_frame(&mut self) {
        self.ppu_cycles_this_frame = 0;

        // Master clock: PPU drives everything at ~5.37 MHz
        // CPU runs at exactly 1/3 PPU speed (~1.79 MHz)
        while self.ppu_cycles_this_frame < PPU_CYCLES_PER_FRAME {
            // Step PPU (1 cycle at master clock rate)
            let nmi_triggered = self.memory.step_ppu();
            
            if nmi_triggered {
                debug!("NMI triggered at scanline {} cycle {}", 
                    self.memory.ppu.scanline, self.memory.ppu.cycle);
                self.cpu.trigger_nmi(&mut self.memory as &mut dyn crate::cpu::CpuBus);
            }

            // Accumulate CPU cycles (CPU runs at 1/3 PPU speed)
            self.cpu_cycle_accumulator += CPU_CYCLES_PER_PPU_CYCLE;
            
            // Run CPU cycles when accumulator >= 1.0
            while self.cpu_cycle_accumulator >= 1.0 {
                // Update trace PPU cycle count before instruction
                if self.trace.enabled {
                    self.trace.ppu_cycle_count = self.total_ppu_cycles;
                }
                let cpu_cycles = self.cpu.step(&mut self.memory as &mut dyn crate::cpu::CpuBus, &mut self.trace);
                self.cpu_cycle_accumulator -= 1.0;
                
                // Step APU for each CPU cycle
                for _ in 0..cpu_cycles {
                    let irq = self.memory.step_apu(1);
                    if irq && (self.cpu.status & crate::cpu::FLAG_I) == 0 {
                        self.cpu.trigger_irq(&mut self.memory as &mut dyn crate::cpu::CpuBus);
                    }
                }
                
                // Generate audio samples at ~44.1kHz (sample every ~40.6 CPU cycles)
                // CPU runs at 1.789 MHz, so 1.789 MHz / 44.1 kHz ≈ 40.6
                const AUDIO_SAMPLE_RATE: f64 = 44_100.0;
                const CPU_FREQ: f64 = 1_789_773.0;
                const CYCLES_PER_SAMPLE: f64 = CPU_FREQ / AUDIO_SAMPLE_RATE;
                
                self.audio_sample_accumulator += cpu_cycles as f64;
                while self.audio_sample_accumulator >= CYCLES_PER_SAMPLE {
                    let sample = self.memory.apu.mix_samples();
                    self.renderer.queue_audio_samples(&[sample]);
                    self.audio_sample_accumulator -= CYCLES_PER_SAMPLE;
                }
            }

            self.ppu_cycles_this_frame += 1;
            self.total_ppu_cycles += 1;
        }

        // Copy framebuffer from PPU and render
        self.render_frame();
        
        // FPS calculation
        self.frame_count += 1;
        self.fps_counter += 1;
        let elapsed = self.last_frame_time.elapsed();
        if elapsed.as_secs_f32() >= 1.0 {
            self.fps = self.fps_counter as f32 / elapsed.as_secs_f32();
            self.fps_counter = 0;
            self.last_frame_time = std::time::Instant::now();
        }
    }

    fn render_frame(&mut self) {
        // Framebuffer is already filled during PPU rendering
        self.renderer.render_frame(&self.memory.ppu.framebuffer);
    }
    
    pub fn get_fps(&self) -> f32 {
        self.fps
    }
    
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn handle_input(&mut self, keycode: u32, pressed: bool) {
        self.memory.input.update_from_keyboard(keycode, pressed);
    }

    pub fn get_renderer(&mut self) -> &mut Renderer {
        &mut self.renderer
    }
    
    pub fn dump_framebuffer(&self, path: &str) -> Result<(), String> {
        use std::fs::File;
        use std::io::Write;
        
        let mut file = File::create(path).map_err(|e| e.to_string())?;
        for &pixel in &self.memory.ppu.framebuffer {
            file.write_all(&[pixel]).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
