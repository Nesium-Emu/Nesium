// APU (Audio Processing Unit) implementation
// Based on NES APU documentation from nesdev.org

const OUTPUT_SAMPLE_RATE: u32 = 44_100;
const CPU_FREQUENCY: f64 = 1_789_773.0; // NTSC CPU clock

// Duty cycle sequences for pulse channels (8 steps each)
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
    [0, 1, 1, 0, 0, 0, 0, 0], // 25%
    [0, 1, 1, 1, 1, 0, 0, 0], // 50%
    [1, 0, 0, 1, 1, 1, 1, 1], // 25% negated
];

// Triangle wave sequence (32 steps)
const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

// Length counter lookup table
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];

// Noise period table (NTSC)
const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

// DMC rate table (NTSC)
const DMC_PERIOD_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

// Mixer lookup tables (precomputed for efficiency)
const PULSE_LUT_SIZE: usize = 31;
const TND_LUT_SIZE: usize = 203;

fn compute_pulse_lut() -> [f32; PULSE_LUT_SIZE] {
    let mut lut = [0.0f32; PULSE_LUT_SIZE];
    for i in 1..PULSE_LUT_SIZE {
        lut[i] = 95.52 / (8128.0 / (i as f32) + 100.0);
    }
    lut
}

fn compute_tnd_lut() -> [f32; TND_LUT_SIZE] {
    let mut lut = [0.0f32; TND_LUT_SIZE];
    for i in 1..TND_LUT_SIZE {
        lut[i] = 163.67 / (24329.0 / (i as f32) + 100.0);
    }
    lut
}

// Simple first-order filters applied at OUTPUT sample rate (not CPU rate)
#[derive(Debug, Clone)]
pub struct FirstOrderFilter {
    prev_output: f32,
    b0: f32,
    b1: f32,
    a1: f32,
    prev_input: f32,
}

impl FirstOrderFilter {
    // High-pass filter
    fn high_pass(cutoff_hz: f32, sample_rate: f32) -> Self {
        let c = sample_rate / (std::f32::consts::PI * cutoff_hz);
        let a0 = 1.0 / (1.0 + c);
        Self {
            prev_output: 0.0,
            prev_input: 0.0,
            b0: c * a0,
            b1: -c * a0,
            a1: (1.0 - c) * a0,
        }
    }
    
    // Low-pass filter  
    fn low_pass(cutoff_hz: f32, sample_rate: f32) -> Self {
        let c = sample_rate / (std::f32::consts::PI * cutoff_hz);
        let a0 = 1.0 / (1.0 + c);
        Self {
            prev_output: 0.0,
            prev_input: 0.0,
            b0: a0,
            b1: a0,
            a1: (1.0 - c) * a0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.b1 * self.prev_input - self.a1 * self.prev_output;
        self.prev_input = input;
        self.prev_output = output;
        output
    }
}

#[derive(Debug, Clone)]
pub struct Apu {
    // Channels
    pub pulse1: PulseChannel,
    pub pulse2: PulseChannel,
    pub triangle: TriangleChannel,
    pub noise: NoiseChannel,
    pub dmc: DmcChannel,

    // Frame sequencer (tracks exact CPU cycles)
    pub cycle_count: u64,  // Total CPU cycles since reset
    pub frame_counter_mode: bool, // false = 4-step, true = 5-step
    pub frame_counter_interrupt: bool,
    pub irq_inhibit: bool,
    pub reset_sequencer: bool,

    // Audio sampling - downsample from CPU rate to output rate
    pub sample_counter: f64,
    pub cycles_per_sample: f64,
    pub sample_buffer: Vec<f32>,
    
    // Adaptive sampling for sync
    pub sample_adjustment: f64,
    
    // Filters applied at OUTPUT sample rate (44.1kHz)
    pub high_pass_90hz: FirstOrderFilter,   // NES hardware high-pass
    pub high_pass_440hz: FirstOrderFilter,  // NES hardware high-pass
    
    // Mixer lookup tables
    pulse_lut: [f32; PULSE_LUT_SIZE],
    tnd_lut: [f32; TND_LUT_SIZE],
}

#[derive(Debug, Clone)]
pub struct PulseChannel {
    pub enabled: bool,
    pub length_counter: u8,
    pub length_counter_halt: bool,
    pub envelope: Envelope,
    pub sweep: Sweep,
    pub timer: u16,
    pub timer_counter: u16,
    pub duty_cycle: u8,
    pub duty_step: u8,  // Current step in 8-step sequence
    pub constant_volume: bool,
    pub channel_id: u8, // 1 or 2 (for sweep negate difference)
    pub mute: bool,
    pub target_period: u16,
}

#[derive(Debug, Clone)]
pub struct TriangleChannel {
    pub enabled: bool,
    pub length_counter: u8,
    pub length_counter_halt: bool,
    pub linear_counter: u8,
    pub linear_counter_reload: u8,
    pub linear_counter_reload_flag: bool,
    pub timer: u16,
    pub timer_counter: u16,
    pub step: u8,
}

#[derive(Debug, Clone)]
pub struct NoiseChannel {
    pub enabled: bool,
    pub length_counter: u8,
    pub length_counter_halt: bool,
    pub envelope: Envelope,
    pub timer: u16,
    pub timer_counter: u16,
    pub shift_register: u16,
    pub mode: bool,
    pub constant_volume: bool,
}

#[derive(Debug, Clone)]
pub struct DmcChannel {
    pub enabled: bool,
    pub loop_flag: bool,
    pub irq_enabled: bool,
    pub irq_occurred: bool,
    pub output_level: u8,
    pub sample_address: u16,
    pub sample_length: u16,
    pub current_address: u16,
    pub bytes_remaining: u16,
    pub sample_buffer: u8,
    pub sample_buffer_empty: bool,
    pub shift_register: u8,
    pub bits_remaining: u8,
    pub silence: bool,
    pub rate: u16,
    pub rate_counter: u16,
}

#[derive(Debug, Clone)]
pub struct Envelope {
    pub start: bool,
    pub loop_flag: bool,
    pub constant_volume: bool,
    pub volume: u8,      // Also divider period
    pub decay_counter: u8,
    pub divider: u8,
}

#[derive(Debug, Clone)]
pub struct Sweep {
    pub enabled: bool,
    pub period: u8,
    pub negate: bool,
    pub shift: u8,
    pub reload: bool,
    pub divider: u8,
}

impl PulseChannel {
    fn new(channel_id: u8) -> Self {
        Self {
            enabled: false,
            length_counter: 0,
            length_counter_halt: false,
            envelope: Envelope::new(),
            sweep: Sweep::new(),
            timer: 0,
            timer_counter: 0,
            duty_cycle: 0,
            duty_step: 0,
            constant_volume: false,
            channel_id,
            mute: false,
            target_period: 0,
        }
    }

    fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;
            // Advance duty step (wraps 0-7)
            self.duty_step = (self.duty_step + 1) & 7;
        } else {
            self.timer_counter -= 1;
        }
    }

    fn update_target_period(&mut self) {
        let change = self.timer >> self.sweep.shift;
        if self.sweep.negate {
            // Pulse 1 uses one's complement (subtracts change + 1)
            // Pulse 2 uses two's complement (subtracts change)
            if self.channel_id == 1 {
                self.target_period = self.timer.wrapping_sub(change).wrapping_sub(1);
            } else {
                self.target_period = self.timer.wrapping_sub(change);
            }
        } else {
            self.target_period = self.timer.wrapping_add(change);
        }
        
        // Mute if timer < 8 or target > 0x7FF
        self.mute = self.timer < 8 || self.target_period > 0x7FF;
    }

    fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || self.mute {
            return 0;
        }

        // Get output from duty table
        let duty_value = DUTY_TABLE[self.duty_cycle as usize][self.duty_step as usize];
        if duty_value == 0 {
            return 0;
        }

        // Return envelope volume
        if self.constant_volume {
            self.envelope.volume
        } else {
            self.envelope.decay_counter
        }
    }
}

impl TriangleChannel {
    fn new() -> Self {
        Self {
            enabled: false,
            length_counter: 0,
            length_counter_halt: false,
            linear_counter: 0,
            linear_counter_reload: 0,
            linear_counter_reload_flag: false,
            timer: 0,
            timer_counter: 0,
            step: 0,
        }
    }

    fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;
            // Only advance if both counters are non-zero
            if self.length_counter > 0 && self.linear_counter > 0 {
                self.step = (self.step + 1) & 31;
            }
        } else {
            self.timer_counter -= 1;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || self.linear_counter == 0 {
            return 0;
        }
        // Mute ultrasonic frequencies (timer < 2)
        if self.timer < 2 {
            return 7; // Return middle value to avoid popping
        }
        TRIANGLE_SEQUENCE[self.step as usize]
    }
}

impl NoiseChannel {
    fn new() -> Self {
        Self {
            enabled: false,
            length_counter: 0,
            length_counter_halt: false,
            envelope: Envelope::new(),
            timer: 0,
            timer_counter: 0,
            shift_register: 1, // Must start at 1
            mode: false,
            constant_volume: false,
        }
    }

    fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;
            // Clock LFSR
            let feedback = if self.mode {
                // Mode 1: XOR bits 0 and 6
                (self.shift_register & 0x01) ^ ((self.shift_register >> 6) & 0x01)
            } else {
                // Mode 0: XOR bits 0 and 1
                (self.shift_register & 0x01) ^ ((self.shift_register >> 1) & 0x01)
            };
            self.shift_register >>= 1;
            self.shift_register |= feedback << 14;
        } else {
            self.timer_counter -= 1;
        }
    }

    fn output(&self) -> u8 {
        // Output is 0 if bit 0 of shift register is set
        if !self.enabled || self.length_counter == 0 || (self.shift_register & 0x01) != 0 {
            return 0;
        }

        if self.constant_volume {
            self.envelope.volume
        } else {
            self.envelope.decay_counter
        }
    }
}

impl DmcChannel {
    fn new() -> Self {
        Self {
            enabled: false,
            loop_flag: false,
            irq_enabled: false,
            irq_occurred: false,
            output_level: 0,
            sample_address: 0xC000,
            sample_length: 1,
            current_address: 0xC000,
            bytes_remaining: 0,
            sample_buffer: 0,
            sample_buffer_empty: true,
            shift_register: 0,
            bits_remaining: 8,
            silence: true,
            rate: DMC_PERIOD_TABLE[0],
            rate_counter: DMC_PERIOD_TABLE[0],
        }
    }

    fn clock(&mut self, cpu_read: &mut impl FnMut(u16) -> u8) -> bool {
        let mut irq = false;
        
        // Memory reader - fill sample buffer if empty and bytes remaining
        if self.enabled && self.sample_buffer_empty && self.bytes_remaining > 0 {
            // Read sample byte (this would stall CPU for 4 cycles in real hardware)
            self.sample_buffer = cpu_read(self.current_address);
            self.sample_buffer_empty = false;
            
            // Increment address with wrap
            self.current_address = if self.current_address == 0xFFFF {
                0x8000
            } else {
                self.current_address + 1
            };
            
            self.bytes_remaining -= 1;
            
            // Handle end of sample
            if self.bytes_remaining == 0 {
                if self.loop_flag {
                    self.current_address = self.sample_address;
                    self.bytes_remaining = self.sample_length;
                } else if self.irq_enabled {
                    self.irq_occurred = true;
                    irq = true;
                }
            }
        }
        
        // Output unit timer
        if self.rate_counter == 0 {
            self.rate_counter = self.rate;
            
            // Clock output unit
            if !self.silence {
                if (self.shift_register & 0x01) != 0 {
                    if self.output_level <= 125 {
                        self.output_level += 2;
                    }
                } else {
                    if self.output_level >= 2 {
                        self.output_level -= 2;
                    }
                }
            }
            
            self.shift_register >>= 1;
            self.bits_remaining -= 1;
            
            // Start new output cycle
            if self.bits_remaining == 0 {
                self.bits_remaining = 8;
                if self.sample_buffer_empty {
                    self.silence = true;
                } else {
                    self.silence = false;
                    self.shift_register = self.sample_buffer;
                    self.sample_buffer_empty = true;
                }
            }
        } else {
            self.rate_counter -= 1;
        }
        
        irq
    }

    fn output(&self) -> u8 {
        self.output_level
    }
}

impl Envelope {
    fn new() -> Self {
        Self {
            start: false,
            loop_flag: false,
            constant_volume: false,
            volume: 0,
            decay_counter: 0,
            divider: 0,
        }
    }

    fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.decay_counter = 15;
            self.divider = self.volume;
        } else {
            if self.divider == 0 {
                self.divider = self.volume;
                if self.decay_counter == 0 {
                    if self.loop_flag {
                        self.decay_counter = 15;
                    }
                } else {
                    self.decay_counter -= 1;
                }
            } else {
                self.divider -= 1;
            }
        }
    }
}

impl Sweep {
    fn new() -> Self {
        Self {
            enabled: false,
            period: 0,
            negate: false,
            shift: 0,
            reload: false,
            divider: 0,
        }
    }

    fn clock(&mut self, timer: &mut u16, mute: bool, channel_id: u8) {
        // Calculate target period
        let change = *timer >> self.shift;
        let target = if self.negate {
            if channel_id == 1 {
                timer.wrapping_sub(change).wrapping_sub(1)
            } else {
                timer.wrapping_sub(change)
            }
        } else {
            timer.wrapping_add(change)
        };
        
        if self.divider == 0 && self.enabled && self.shift > 0 && !mute {
            if target <= 0x7FF {
                *timer = target;
            }
        }
        
        if self.divider == 0 || self.reload {
            self.divider = self.period;
            self.reload = false;
        } else {
            self.divider -= 1;
        }
    }
}

impl Apu {
    pub fn new() -> Self {
        Self {
            pulse1: PulseChannel::new(1),
            pulse2: PulseChannel::new(2),
            triangle: TriangleChannel::new(),
            noise: NoiseChannel::new(),
            dmc: DmcChannel::new(),
            cycle_count: 0,
            frame_counter_mode: false,
            frame_counter_interrupt: false,
            irq_inhibit: false,
            reset_sequencer: false,
            sample_counter: 0.0,
            cycles_per_sample: CPU_FREQUENCY / (OUTPUT_SAMPLE_RATE as f64),
            sample_buffer: Vec::with_capacity(1024),
            sample_adjustment: 0.0,
            // NES has two first-order high-pass filters in the audio path
            high_pass_90hz: FirstOrderFilter::high_pass(90.0, OUTPUT_SAMPLE_RATE as f32),
            high_pass_440hz: FirstOrderFilter::high_pass(440.0, OUTPUT_SAMPLE_RATE as f32),
            pulse_lut: compute_pulse_lut(),
            tnd_lut: compute_tnd_lut(),
        }
    }

    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => {
                let mut value = 0u8;
                if self.pulse1.length_counter > 0 {
                    value |= 0x01;
                }
                if self.pulse2.length_counter > 0 {
                    value |= 0x02;
                }
                if self.triangle.length_counter > 0 {
                    value |= 0x04;
                }
                if self.noise.length_counter > 0 {
                    value |= 0x08;
                }
                if self.dmc.bytes_remaining > 0 {
                    value |= 0x10;
                }
                if self.frame_counter_interrupt {
                    value |= 0x40;
                }
                if self.dmc.irq_occurred {
                    value |= 0x80;
                }
                // Reading $4015 clears frame interrupt flag
                self.frame_counter_interrupt = false;
                value
            }
            _ => 0,
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8, _cpu_read: impl Fn(u16) -> u8) {
        match addr {
            // Pulse 1: $4000-$4003
            0x4000 => {
                self.pulse1.duty_cycle = (value >> 6) & 0x03;
                self.pulse1.length_counter_halt = (value & 0x20) != 0;
                self.pulse1.envelope.loop_flag = (value & 0x20) != 0;
                self.pulse1.constant_volume = (value & 0x10) != 0;
                self.pulse1.envelope.volume = value & 0x0F;
            }
            0x4001 => {
                self.pulse1.sweep.enabled = (value & 0x80) != 0;
                self.pulse1.sweep.period = (value >> 4) & 0x07;
                self.pulse1.sweep.negate = (value & 0x08) != 0;
                self.pulse1.sweep.shift = value & 0x07;
                self.pulse1.sweep.reload = true;
                self.pulse1.update_target_period();
            }
            0x4002 => {
                self.pulse1.timer = (self.pulse1.timer & 0xFF00) | value as u16;
                self.pulse1.update_target_period();
            }
            0x4003 => {
                self.pulse1.timer = (self.pulse1.timer & 0x00FF) | (((value & 0x07) as u16) << 8);
                if self.pulse1.enabled {
                    self.pulse1.length_counter = LENGTH_TABLE[((value >> 3) & 0x1F) as usize];
                }
                self.pulse1.duty_step = 0; // Reset sequencer
                self.pulse1.envelope.start = true;
                self.pulse1.update_target_period();
            }
            
            // Pulse 2: $4004-$4007
            0x4004 => {
                self.pulse2.duty_cycle = (value >> 6) & 0x03;
                self.pulse2.length_counter_halt = (value & 0x20) != 0;
                self.pulse2.envelope.loop_flag = (value & 0x20) != 0;
                self.pulse2.constant_volume = (value & 0x10) != 0;
                self.pulse2.envelope.volume = value & 0x0F;
            }
            0x4005 => {
                self.pulse2.sweep.enabled = (value & 0x80) != 0;
                self.pulse2.sweep.period = (value >> 4) & 0x07;
                self.pulse2.sweep.negate = (value & 0x08) != 0;
                self.pulse2.sweep.shift = value & 0x07;
                self.pulse2.sweep.reload = true;
                self.pulse2.update_target_period();
            }
            0x4006 => {
                self.pulse2.timer = (self.pulse2.timer & 0xFF00) | value as u16;
                self.pulse2.update_target_period();
            }
            0x4007 => {
                self.pulse2.timer = (self.pulse2.timer & 0x00FF) | (((value & 0x07) as u16) << 8);
                if self.pulse2.enabled {
                    self.pulse2.length_counter = LENGTH_TABLE[((value >> 3) & 0x1F) as usize];
                }
                self.pulse2.duty_step = 0;
                self.pulse2.envelope.start = true;
                self.pulse2.update_target_period();
            }
            
            // Triangle: $4008-$400B
            0x4008 => {
                self.triangle.length_counter_halt = (value & 0x80) != 0;
                self.triangle.linear_counter_reload = value & 0x7F;
            }
            0x400A => {
                self.triangle.timer = (self.triangle.timer & 0xFF00) | value as u16;
            }
            0x400B => {
                self.triangle.timer = (self.triangle.timer & 0x00FF) | (((value & 0x07) as u16) << 8);
                if self.triangle.enabled {
                    self.triangle.length_counter = LENGTH_TABLE[((value >> 3) & 0x1F) as usize];
                }
                self.triangle.linear_counter_reload_flag = true;
            }
            
            // Noise: $400C-$400F
            0x400C => {
                self.noise.length_counter_halt = (value & 0x20) != 0;
                self.noise.envelope.loop_flag = (value & 0x20) != 0;
                self.noise.constant_volume = (value & 0x10) != 0;
                self.noise.envelope.volume = value & 0x0F;
            }
            0x400E => {
                self.noise.mode = (value & 0x80) != 0;
                self.noise.timer = NOISE_PERIOD_TABLE[(value & 0x0F) as usize];
            }
            0x400F => {
                if self.noise.enabled {
                    self.noise.length_counter = LENGTH_TABLE[((value >> 3) & 0x1F) as usize];
                }
                self.noise.envelope.start = true;
            }
            
            // DMC: $4010-$4013
            0x4010 => {
                self.dmc.irq_enabled = (value & 0x80) != 0;
                self.dmc.loop_flag = (value & 0x40) != 0;
                self.dmc.rate = DMC_PERIOD_TABLE[(value & 0x0F) as usize];
                if !self.dmc.irq_enabled {
                    self.dmc.irq_occurred = false;
                }
            }
            0x4011 => {
                self.dmc.output_level = value & 0x7F;
            }
            0x4012 => {
                self.dmc.sample_address = 0xC000 | ((value as u16) << 6);
            }
            0x4013 => {
                self.dmc.sample_length = ((value as u16) << 4) | 1;
            }
            
            // Status: $4015
            0x4015 => {
                self.pulse1.enabled = (value & 0x01) != 0;
                self.pulse2.enabled = (value & 0x02) != 0;
                self.triangle.enabled = (value & 0x04) != 0;
                self.noise.enabled = (value & 0x08) != 0;
                self.dmc.enabled = (value & 0x10) != 0;
                
                if !self.pulse1.enabled {
                    self.pulse1.length_counter = 0;
                }
                if !self.pulse2.enabled {
                    self.pulse2.length_counter = 0;
                }
                if !self.triangle.enabled {
                    self.triangle.length_counter = 0;
                }
                if !self.noise.enabled {
                    self.noise.length_counter = 0;
                }
                if !self.dmc.enabled {
                    self.dmc.bytes_remaining = 0;
                } else if self.dmc.bytes_remaining == 0 {
                    self.dmc.current_address = self.dmc.sample_address;
                    self.dmc.bytes_remaining = self.dmc.sample_length;
                }
                self.dmc.irq_occurred = false;
            }
            
            // Frame counter: $4017
            0x4017 => {
                self.frame_counter_mode = (value & 0x80) != 0;
                self.irq_inhibit = (value & 0x40) != 0;
                if self.irq_inhibit {
                    self.frame_counter_interrupt = false;
                }
                self.reset_sequencer = true;
            }
            _ => {}
        }
    }

    pub fn step(&mut self, cpu_cycles: u64, mut cpu_read: impl FnMut(u16) -> u8) -> bool {
        let mut irq = false;

        for _ in 0..cpu_cycles {
            // Handle sequencer reset
            if self.reset_sequencer {
                self.reset_sequencer = false;
                if self.frame_counter_mode {
                    // 5-step mode: immediately clock quarter and half frame
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }
                self.cycle_count = 0;
            }
            
            // Clock pulse timers every other CPU cycle (they run at APU rate = CPU/2)
            if self.cycle_count & 1 == 1 {
                self.pulse1.clock_timer();
                self.pulse2.clock_timer();
                self.noise.clock_timer();
            }
            
            // Triangle timer runs at CPU rate
            self.triangle.clock_timer();
            
            // DMC runs at CPU rate
            if self.dmc.clock(&mut cpu_read) {
                irq = true;
            }

            // Frame sequencer - use exact cycle counts for NTSC
            if self.frame_counter_mode {
                // 5-step mode (37281 CPU cycles per sequence)
                match self.cycle_count {
                    7457 => self.clock_quarter_frame(),
                    14913 => {
                        self.clock_quarter_frame();
                        self.clock_half_frame();
                    }
                    22371 => self.clock_quarter_frame(),
                    37281 => {
                        self.clock_quarter_frame();
                        self.clock_half_frame();
                        self.cycle_count = 0;
                    }
                    _ => {}
                }
            } else {
                // 4-step mode (29830 CPU cycles per sequence)
                match self.cycle_count {
                    7457 => self.clock_quarter_frame(),
                    14913 => {
                        self.clock_quarter_frame();
                        self.clock_half_frame();
                    }
                    22371 => self.clock_quarter_frame(),
                    29829 => {
                        self.clock_quarter_frame();
                        self.clock_half_frame();
                        if !self.irq_inhibit {
                            self.frame_counter_interrupt = true;
                            irq = true;
                        }
                        self.cycle_count = 0;
                    }
                    _ => {}
                }
            }

            // Generate output samples at target rate (downsample from ~1.79MHz to 44.1kHz)
            self.sample_counter += 1.0;
            let adjusted_rate = self.cycles_per_sample + self.sample_adjustment;
            if self.sample_counter >= adjusted_rate {
                self.sample_counter -= adjusted_rate;
                
                // Get mixed sample
                let raw_sample = self.get_sample();
                
                // Apply NES high-pass filters (at output sample rate)
                let sample = self.high_pass_90hz.process(raw_sample);
                let sample = self.high_pass_440hz.process(sample);
                
                self.sample_buffer.push(sample);
            }

            if self.cycle_count < u64::MAX {
                self.cycle_count += 1;
            }
        }

        irq
    }

    fn clock_quarter_frame(&mut self) {
        // Clock envelopes
        self.pulse1.envelope.clock();
        self.pulse2.envelope.clock();
        self.noise.envelope.clock();
        
        // Clock triangle linear counter
        if self.triangle.linear_counter_reload_flag {
            self.triangle.linear_counter = self.triangle.linear_counter_reload;
        } else if self.triangle.linear_counter > 0 {
            self.triangle.linear_counter -= 1;
        }
        
        // Clear reload flag if halt is clear
        if !self.triangle.length_counter_halt {
            self.triangle.linear_counter_reload_flag = false;
        }
    }

    fn clock_half_frame(&mut self) {
        // Clock length counters
        if !self.pulse1.length_counter_halt && self.pulse1.length_counter > 0 {
            self.pulse1.length_counter -= 1;
        }
        if !self.pulse2.length_counter_halt && self.pulse2.length_counter > 0 {
            self.pulse2.length_counter -= 1;
        }
        if !self.triangle.length_counter_halt && self.triangle.length_counter > 0 {
            self.triangle.length_counter -= 1;
        }
        if !self.noise.length_counter_halt && self.noise.length_counter > 0 {
            self.noise.length_counter -= 1;
        }
        
        // Clock sweep units
        self.pulse1.sweep.clock(&mut self.pulse1.timer, self.pulse1.mute, 1);
        self.pulse1.update_target_period();
        self.pulse2.sweep.clock(&mut self.pulse2.timer, self.pulse2.mute, 2);
        self.pulse2.update_target_period();
    }

    fn get_sample(&self) -> f32 {
        // Get channel outputs
        let pulse1 = self.pulse1.output() as usize;
        let pulse2 = self.pulse2.output() as usize;
        let triangle = self.triangle.output() as usize;
        let noise = self.noise.output() as usize;
        let dmc = self.dmc.output() as usize;
        
        // Use lookup tables for mixing (avoids division by zero)
        let pulse_out = pulse1 + pulse2;
        let pulse_mix = if pulse_out < PULSE_LUT_SIZE {
            self.pulse_lut[pulse_out]
        } else {
            self.pulse_lut[PULSE_LUT_SIZE - 1]
        };
        
        // TND = 3 * triangle + 2 * noise + dmc
        let tnd_out = triangle * 3 + noise * 2 + dmc;
        let tnd_mix = if tnd_out < TND_LUT_SIZE {
            self.tnd_lut[tnd_out]
        } else {
            self.tnd_lut[TND_LUT_SIZE - 1]
        };
        
        pulse_mix + tnd_mix
    }

    pub fn mix_samples(&self) -> f32 {
        self.get_sample()
    }
    
    // Get buffered samples and clear the buffer
    pub fn take_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.sample_buffer)
    }
    
    // Adjust sample rate for audio sync
    pub fn adjust_sample_rate(&mut self, queue_size: usize, target_queue_size: usize) {
        // Simple proportional control to match audio production to consumption
        let error = queue_size as f64 - target_queue_size as f64;
        // Small adjustment factor to avoid oscillation
        self.sample_adjustment = error * 0.00001;
        // Clamp adjustment to prevent extreme values
        self.sample_adjustment = self.sample_adjustment.clamp(-0.5, 0.5);
    }
}
