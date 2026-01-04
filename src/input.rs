#[derive(Debug, Clone)]
pub struct Input {
    pub controller1: ControllerState,
    pub controller2: ControllerState,
    pub strobe: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ControllerState {
    pub a: bool,
    pub b: bool,
    pub select: bool,
    pub start: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    shift_register: u8,
    read_count: u8,
}

impl ControllerState {
    pub fn new() -> Self {
        Self {
            a: false,
            b: false,
            select: false,
            start: false,
            up: false,
            down: false,
            left: false,
            right: false,
            shift_register: 0,
            read_count: 0,
        }
    }

    /// Latch current button states into shift register
    /// Button order: A, B, Select, Start, Up, Down, Left, Right (bits 0-7)
    pub fn latch(&mut self) {
        self.shift_register = 0
            | (if self.a { 0x01 } else { 0 })
            | (if self.b { 0x02 } else { 0 })
            | (if self.select { 0x04 } else { 0 })
            | (if self.start { 0x08 } else { 0 })
            | (if self.up { 0x10 } else { 0 })
            | (if self.down { 0x20 } else { 0 })
            | (if self.left { 0x40 } else { 0 })
            | (if self.right { 0x80 } else { 0 });
        self.read_count = 0;
    }

    /// Read one bit from the shift register
    /// When strobe is active, continuously returns A button state
    /// After 8 reads, returns 0 (open bus behavior)
    pub fn read(&mut self, strobe_active: bool) -> u8 {
        if strobe_active {
            // Strobe active: continuously return A button state
            if self.a { 0x01 } else { 0x00 }
        } else {
            // Strobe inactive: read from shift register
            if self.read_count < 8 {
                let bit = (self.shift_register >> self.read_count) & 0x01;
                self.read_count += 1;
                // Optional debug: log::debug!("Controller read: bit {} (count: {})", bit, self.read_count);
                bit
            } else {
                // After 8 reads, return 0 (open bus)
                0
            }
        }
    }
}

impl Input {
    pub fn new() -> Self {
        Self {
            controller1: ControllerState::new(),
            controller2: ControllerState::new(),
            strobe: false,
        }
    }

    pub fn write(&mut self, value: u8) {
        let new_strobe = (value & 0x01) != 0;
        
        // Latch button states on falling edge of strobe (1 -> 0)
        if self.strobe && !new_strobe {
            // Strobe falling edge: latch current button states into shift registers
            self.controller1.latch();
            self.controller2.latch();
        }
        
        self.strobe = new_strobe;
    }

    pub fn read(&mut self, port: u8) -> u8 {
        if port == 0 {
            self.controller1.read(self.strobe)
        } else {
            self.controller2.read(self.strobe)
        }
    }

    pub fn update_from_keyboard(&mut self, scancode: u32, pressed: bool) {
        // Map SDL2 scancodes to NES buttons
        // Button order: A, B, Select, Start, Up, Down, Left, Right
        match scancode {
            1073742048 | 97 => self.controller1.a = pressed,      // A key -> A button
            1073742050 | 115 => self.controller1.b = pressed,    // S key -> B button
            1073742052 | 13 => self.controller1.start = pressed,  // Enter -> Start
            1073742053 => self.controller1.select = pressed,     // Right Shift -> Select
            1073741904 => self.controller1.up = pressed,        // Up arrow -> Up
            1073741905 => self.controller1.down = pressed,       // Down arrow -> Down
            1073741903 => self.controller1.left = pressed,       // Left arrow -> Left
            1073741906 => self.controller1.right = pressed,       // Right arrow -> Right
            _ => {}
        }
        
        // Note: We don't latch here - latching only happens on strobe falling edge
        // The shift register will be updated the next time strobe goes from 1 to 0
    }
}
