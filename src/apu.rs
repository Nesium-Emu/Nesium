const APU_SAMPLE_RATE: f64 = 1_789_773.0; // CPU clock speed
const OUTPUT_SAMPLE_RATE: u32 = 44_100;

#[derive(Debug, Clone)]
pub struct Apu {
    // Channels
    pub pulse1: PulseChannel,
    pub pulse2: PulseChannel,
    pub triangle: TriangleChannel,
    pub noise: NoiseChannel,
    pub dmc: DmcChannel,

    // Frame sequencer (runs at 240 Hz = every 7457 CPU cycles in NTSC)
    pub frame_counter: u16,  // CPU cycle counter for frame sequencer
    pub frame_counter_period: u16,  // 7457 for NTSC
    pub frame_counter_mode: bool, // false = 4-step, true = 5-step
    pub frame_counter_interrupt: bool,
    pub frame_sequencer_step: u8,  // Current step (0-4 for 4-step, 0-4 for 5-step)
}

#[derive(Debug, Clone)]
pub struct PulseChannel {
    pub enabled: bool,
    pub length_counter: u8,
    pub envelope: Envelope,
    pub sweep: Sweep,
    pub timer: u16,
    pub timer_counter: u16,
    pub duty_cycle: u8,
    pub duty_counter: u8,
    pub constant_volume: bool,
}

#[derive(Debug, Clone)]
pub struct TriangleChannel {
    pub enabled: bool,
    pub length_counter: u8,
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
    pub shift_register: u8,
    pub bits_remaining: u8,
    pub silence: bool,
    pub timer: u16,
    pub timer_counter: u16,
}

#[derive(Debug, Clone)]
pub struct Envelope {
    pub start: bool,
    pub loop_flag: bool,
    pub constant_volume: bool,
    pub volume: u8,
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
    fn new() -> Self {
        Self {
            enabled: false,
            length_counter: 0,
            envelope: Envelope::new(),
            sweep: Sweep::new(),
            timer: 0,
            timer_counter: 0,
            duty_cycle: 0,
            duty_counter: 0,
            constant_volume: false,
        }
    }

    fn step(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;
            self.duty_counter = (self.duty_counter + 1) % 8;
        } else {
            self.timer_counter -= 1;
        }
    }

    fn output(&self) -> f32 {
        if !self.enabled
            || self.length_counter == 0
            || self.timer < 8
            || self.timer > 0x7FF
            || self.sweep.mute(self.timer)
        {
            return 0.0;
        }

        let duty_table = [0.0, 0.125, 0.25, 0.5, 0.5, 0.75, 0.75, 0.875];
        let duty = duty_table[self.duty_cycle as usize];
        let volume = if self.constant_volume {
            self.envelope.volume as f32
        } else {
            self.envelope.decay_counter as f32
        };

        if (self.duty_counter as f32) < duty * 8.0 {
            volume / 15.0
        } else {
            0.0
        }
    }
}

impl TriangleChannel {
    fn new() -> Self {
        Self {
            enabled: false,
            length_counter: 0,
            linear_counter: 0,
            linear_counter_reload: 0,
            linear_counter_reload_flag: false,
            timer: 0,
            timer_counter: 0,
            step: 0,
        }
    }

    fn step(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;
            if self.length_counter > 0 && self.linear_counter > 0 {
                self.step = (self.step + 1) % 32;
            }
        } else {
            self.timer_counter -= 1;
        }
        
        // Linear counter reload flag is cleared every CPU cycle (handled in frame sequencer)
    }

    fn output(&self) -> f32 {
        if !self.enabled || self.length_counter == 0 || self.linear_counter == 0 || self.timer < 2 {
            return 0.0;
        }

        let triangle_wave = [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        (triangle_wave[self.step as usize] as f32) / 15.0
    }
}

impl NoiseChannel {
    fn new() -> Self {
        Self {
            enabled: false,
            length_counter: 0,
            envelope: Envelope::new(),
            timer: 0,
            timer_counter: 0,
            shift_register: 1,
            mode: false,
            constant_volume: false,
        }
    }

    fn step(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;
            let feedback = if self.mode {
                (self.shift_register & 0x01) ^ ((self.shift_register >> 6) & 0x01)
            } else {
                (self.shift_register & 0x01) ^ ((self.shift_register >> 1) & 0x01)
            };
            self.shift_register = (self.shift_register >> 1) | (feedback << 14);
        } else {
            self.timer_counter -= 1;
        }
    }

    fn output(&self) -> f32 {
        if !self.enabled || self.length_counter == 0 || (self.shift_register & 0x01) != 0 {
            return 0.0;
        }

        let volume = if self.constant_volume {
            self.envelope.volume as f32
        } else {
            self.envelope.decay_counter as f32
        };
        volume / 15.0
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
            sample_length: 0,
            current_address: 0,
            bytes_remaining: 0,
            shift_register: 0,
            bits_remaining: 0,
            silence: true,
            timer: 0,
            timer_counter: 0,
        }
    }

    fn step(&mut self, cpu_read: &mut impl FnMut(u16) -> u8) -> bool {
        let mut irq = false;
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;
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

            if self.bits_remaining == 0 {
                self.bits_remaining = 8;
                if self.bytes_remaining > 0 {
                    self.silence = false;
                    self.shift_register = cpu_read(self.current_address);
                    self.current_address = if self.current_address == 0xFFFF {
                        0x8000
                    } else {
                        self.current_address + 1
                    };
                    self.bytes_remaining -= 1;
                    if self.bytes_remaining == 0 {
                        if self.loop_flag {
                            self.current_address = self.sample_address;
                            self.bytes_remaining = self.sample_length;
                        } else if self.irq_enabled {
                            self.irq_occurred = true;
                            irq = true;
                        }
                        if self.bytes_remaining == 0 {
                            self.silence = true;
                        }
                    }
                } else {
                    self.silence = true;
                }
            }
        } else {
            self.timer_counter -= 1;
        }
        irq
    }

    fn output(&self) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        (self.output_level as f32) / 127.0
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

    fn step(&mut self) {
        if !self.start {
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
        } else {
            self.start = false;
            self.decay_counter = 15;
            self.divider = self.volume;
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

    fn step(&mut self, channel_timer: &mut u16) {
        if self.reload {
            if self.divider == 0 && self.enabled {
                self.mutate(channel_timer);
            }
            self.divider = self.period;
            self.reload = false;
        } else if self.divider > 0 {
            self.divider -= 1;
        } else {
            if self.enabled {
                self.mutate(channel_timer);
            }
            self.divider = self.period;
        }
    }

    fn mutate(&self, timer: &mut u16) {
        let change = *timer >> self.shift;
        if self.negate {
            *timer = timer.wrapping_sub(change);
            if self.shift == 7 {
                // Pulse 1 adds one more
            }
        } else {
            *timer = timer.wrapping_add(change);
        }
    }

    fn mute(&self, timer: u16) -> bool {
        if !self.enabled {
            return false;
        }
        let change = timer >> self.shift;
        let result = if self.negate {
            timer.wrapping_sub(change)
        } else {
            timer.wrapping_add(change)
        };
        result > 0x7FF
    }
}

impl Apu {
    pub fn new() -> Self {
        Self {
            pulse1: PulseChannel::new(),
            pulse2: PulseChannel::new(),
            triangle: TriangleChannel::new(),
            noise: NoiseChannel::new(),
            dmc: DmcChannel::new(),
            frame_counter: 0,
            frame_counter_period: 7457,  // NTSC: 1.789 MHz / 240 Hz = 7457.125 cycles
            frame_counter_mode: false,
            frame_counter_interrupt: false,
            frame_sequencer_step: 0,
        }
    }

    pub fn read_register(&self, addr: u16) -> u8 {
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
                value
            }
            _ => 0,
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8, _cpu_read: impl Fn(u16) -> u8) {
        match addr {
            0x4000 => {
                self.pulse1.duty_cycle = (value >> 6) & 0x03;
                self.pulse1.envelope.loop_flag = (value & 0x20) != 0;
                self.pulse1.constant_volume = (value & 0x10) != 0;
                self.pulse1.envelope.volume = value & 0x0F;
                self.pulse1.envelope.constant_volume = self.pulse1.constant_volume;
            }
            0x4001 => {
                self.pulse1.sweep.enabled = (value & 0x80) != 0;
                self.pulse1.sweep.period = (value >> 4) & 0x07;
                self.pulse1.sweep.negate = (value & 0x08) != 0;
                self.pulse1.sweep.shift = value & 0x07;
                self.pulse1.sweep.reload = true;
            }
            0x4002 => {
                self.pulse1.timer = (self.pulse1.timer & 0xFF00) | value as u16;
            }
            0x4003 => {
                self.pulse1.timer = (self.pulse1.timer & 0x00FF) | (((value & 0x07) as u16) << 8);
                self.pulse1.duty_counter = 0;
                if self.pulse1.enabled {
                    self.pulse1.length_counter = LENGTH_TABLE[((value >> 3) & 0x1F) as usize];
                }
                self.pulse1.envelope.start = true;
            }
            0x4004 => {
                self.pulse2.duty_cycle = (value >> 6) & 0x03;
                self.pulse2.envelope.loop_flag = (value & 0x20) != 0;
                self.pulse2.constant_volume = (value & 0x10) != 0;
                self.pulse2.envelope.volume = value & 0x0F;
                self.pulse2.envelope.constant_volume = self.pulse2.constant_volume;
            }
            0x4005 => {
                self.pulse2.sweep.enabled = (value & 0x80) != 0;
                self.pulse2.sweep.period = (value >> 4) & 0x07;
                self.pulse2.sweep.negate = (value & 0x08) != 0;
                self.pulse2.sweep.shift = value & 0x07;
                self.pulse2.sweep.reload = true;
            }
            0x4006 => {
                self.pulse2.timer = (self.pulse2.timer & 0xFF00) | value as u16;
            }
            0x4007 => {
                self.pulse2.timer = (self.pulse2.timer & 0x00FF) | (((value & 0x07) as u16) << 8);
                self.pulse2.duty_counter = 0;
                if self.pulse2.enabled {
                    self.pulse2.length_counter = LENGTH_TABLE[((value >> 3) & 0x1F) as usize];
                }
                self.pulse2.envelope.start = true;
            }
            0x4008 => {
                self.triangle.linear_counter_reload_flag = (value & 0x80) != 0;
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
            0x400C => {
                self.noise.envelope.loop_flag = (value & 0x20) != 0;
                self.noise.constant_volume = (value & 0x10) != 0;
                self.noise.envelope.volume = value & 0x0F;
                self.noise.envelope.constant_volume = self.noise.constant_volume;
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
            0x4010 => {
                self.dmc.irq_enabled = (value & 0x80) != 0;
                self.dmc.loop_flag = (value & 0x40) != 0;
                self.dmc.timer = DMC_PERIOD_TABLE[(value & 0x0F) as usize];
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
            0x4017 => {
                self.frame_counter_mode = (value & 0x80) != 0;
                self.frame_counter_interrupt = false;  // Writing to $4017 clears interrupt flag
                if self.frame_counter_mode {
                    // 5-step mode: reset to step 0 and immediately clock
                    self.frame_counter = 0;
                    self.frame_sequencer_step = 0;
                    self.step_frame_counter();
                } else {
                    // 4-step mode: reset to step 0
                    self.frame_counter = 0;
                    self.frame_sequencer_step = 0;
                }
            }
            _ => {}
        }
    }

    pub fn step(&mut self, cpu_cycles: u64, mut cpu_read: impl FnMut(u16) -> u8) -> bool {
        let mut irq = false;

        // Step channels for each CPU cycle
        for _ in 0..cpu_cycles {
            // Clear triangle linear counter reload flag if control flag is clear
            // (control flag is bit 7 of $4008, stored in linear_counter_reload_flag meaning)
            // Actually, the reload flag is cleared every cycle if control is clear
            // For now, we handle this in the frame sequencer
            
            self.pulse1.step();
            self.pulse2.step();
            self.triangle.step();
            self.noise.step();
            if self.dmc.step(&mut cpu_read) {
                irq = true;
            }

            // Frame sequencer runs at 240 Hz (every 7457 CPU cycles in NTSC)
            self.frame_counter += 1;
            if self.frame_counter >= self.frame_counter_period {
                self.frame_counter = 0;
                self.step_frame_counter();
            }
        }

        irq
    }

    fn step_frame_counter(&mut self) {
        if self.frame_counter_mode {
            // 5-step mode: quarter frames at steps 0, 1, 2, 4; half frames at 1, 4
            match self.frame_sequencer_step {
                0 | 2 => {
                    // Quarter frame: envelope and linear counter
                    self.pulse1.envelope.step();
                    self.pulse2.envelope.step();
                    self.noise.envelope.step();
                    // Triangle linear counter
                    if self.triangle.linear_counter_reload_flag {
                        self.triangle.linear_counter = self.triangle.linear_counter_reload;
                    } else if self.triangle.linear_counter > 0 {
                        self.triangle.linear_counter -= 1;
                    }
                    if !self.triangle.linear_counter_reload_flag {
                        self.triangle.linear_counter_reload_flag = false;
                    }
                }
                1 | 4 => {
                    // Quarter frame + half frame: envelope, linear counter, length counter, sweep
                    self.pulse1.envelope.step();
                    self.pulse2.envelope.step();
                    self.noise.envelope.step();
                    // Triangle linear counter
                    if self.triangle.linear_counter_reload_flag {
                        self.triangle.linear_counter = self.triangle.linear_counter_reload;
                    } else if self.triangle.linear_counter > 0 {
                        self.triangle.linear_counter -= 1;
                    }
                    if !self.triangle.linear_counter_reload_flag {
                        self.triangle.linear_counter_reload_flag = false;
                    }
                    // Length counters
                    if self.pulse1.length_counter > 0 {
                        self.pulse1.length_counter -= 1;
                    }
                    if self.pulse2.length_counter > 0 {
                        self.pulse2.length_counter -= 1;
                    }
                    if self.triangle.length_counter > 0 {
                        self.triangle.length_counter -= 1;
                    }
                    if self.noise.length_counter > 0 {
                        self.noise.length_counter -= 1;
                    }
                    // Sweep units
                    self.pulse1.sweep.step(&mut self.pulse1.timer);
                    self.pulse2.sweep.step(&mut self.pulse2.timer);
                }
                3 => {
                    // No operation
                }
                _ => {}
            }
            self.frame_sequencer_step = (self.frame_sequencer_step + 1) % 5;
        } else {
            // 4-step mode: quarter frames at steps 0, 1, 2, 3; half frames at 1, 3
            match self.frame_sequencer_step {
                0 | 2 => {
                    // Quarter frame: envelope and linear counter
                    self.pulse1.envelope.step();
                    self.pulse2.envelope.step();
                    self.noise.envelope.step();
                    // Triangle linear counter
                    if self.triangle.linear_counter_reload_flag {
                        self.triangle.linear_counter = self.triangle.linear_counter_reload;
                    } else if self.triangle.linear_counter > 0 {
                        self.triangle.linear_counter -= 1;
                    }
                    if !self.triangle.linear_counter_reload_flag {
                        self.triangle.linear_counter_reload_flag = false;
                    }
                }
                1 | 3 => {
                    // Quarter frame + half frame: envelope, linear counter, length counter, sweep
                    self.pulse1.envelope.step();
                    self.pulse2.envelope.step();
                    self.noise.envelope.step();
                    // Triangle linear counter
                    if self.triangle.linear_counter_reload_flag {
                        self.triangle.linear_counter = self.triangle.linear_counter_reload;
                    } else if self.triangle.linear_counter > 0 {
                        self.triangle.linear_counter -= 1;
                    }
                    if !self.triangle.linear_counter_reload_flag {
                        self.triangle.linear_counter_reload_flag = false;
                    }
                    // Length counters
                    if self.pulse1.length_counter > 0 {
                        self.pulse1.length_counter -= 1;
                    }
                    if self.pulse2.length_counter > 0 {
                        self.pulse2.length_counter -= 1;
                    }
                    if self.triangle.length_counter > 0 {
                        self.triangle.length_counter -= 1;
                    }
                    if self.noise.length_counter > 0 {
                        self.noise.length_counter -= 1;
                    }
                    // Sweep units
                    self.pulse1.sweep.step(&mut self.pulse1.timer);
                    self.pulse2.sweep.step(&mut self.pulse2.timer);
                }
                _ => {}
            }
            if self.frame_sequencer_step == 3 {
                // Set interrupt flag at end of step 3 (4-step mode)
                if !self.frame_counter_interrupt {
                    self.frame_counter_interrupt = true;
                }
            }
            self.frame_sequencer_step = (self.frame_sequencer_step + 1) % 4;
        }
    }

    pub fn mix_samples(&self) -> f32 {
        let pulse1 = self.pulse1.output();
        let pulse2 = self.pulse2.output();
        let triangle = self.triangle.output();
        let noise = self.noise.output();
        let dmc = self.dmc.output();

        // Mix using approximate NES mixing
        let pulse_out = 95.88 / ((8128.0 / (pulse1 + pulse2)) + 100.0);
        let tnd_out = 159.79 / ((1.0 / ((triangle / 8227.0) + (noise / 12241.0) + (dmc / 22638.0))) + 100.0);
        (pulse_out + tnd_out) / 2.0
    }
}

const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];

const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

const DMC_PERIOD_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];
