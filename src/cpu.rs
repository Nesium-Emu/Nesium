use log::trace;
use crate::trace::{TraceState, disassemble_instruction};

#[derive(Debug, Clone)]
pub struct Cpu {
    pub pc: u16,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub status: u8,
    pub cycles: u64,
    pub stall_cycles: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Interrupt {
    None,
    Nmi,
    Irq,
}

// Status flags
pub const FLAG_C: u8 = 0x01; // Carry
pub const FLAG_Z: u8 = 0x02; // Zero
pub const FLAG_I: u8 = 0x04; // Interrupt Disable
pub const FLAG_D: u8 = 0x08; // Decimal Mode (unused on NES)
pub const FLAG_B: u8 = 0x10; // Break
pub const FLAG_U: u8 = 0x20; // Unused (always 1)
pub const FLAG_V: u8 = 0x40; // Overflow
pub const FLAG_N: u8 = 0x80; // Negative

pub trait CpuBus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);
    fn is_oamdma_addr(&self, _addr: u16) -> bool {
        false
    }
    fn write_oam(&mut self, _oam_addr: u16, _value: u8) {
        // Default: no-op
    }
    /// Trigger OAMDMA transfer and return the number of CPU cycles to stall
    /// Returns 513 if current cycle is odd, 514 if even
    fn trigger_oamdma(&mut self, _page: u8, _current_cycle_odd: bool) -> u64 {
        // Default: no-op, return 0 stall cycles
        0
    }
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            pc: 0,
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            status: FLAG_U | FLAG_I,
            cycles: 0,
            stall_cycles: 0,
        }
    }

    pub fn reset(&mut self, bus: &mut dyn CpuBus) {
        self.sp = self.sp.wrapping_sub(3);
        self.status |= FLAG_I;
        self.pc = self.read_word(0xFFFC, bus);
    }

    pub fn step(&mut self, bus: &mut dyn CpuBus, trace_state: &mut TraceState) -> u64 {
        let interrupt = self.check_interrupts(bus);
        let mut cycles = 0;

        if interrupt != Interrupt::None {
            cycles += self.handle_interrupt(interrupt, bus);
            if trace_state.enabled {
                // Trace interrupt handling
                println!("{:04X}  {:02X}     {:20} A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC:{}",
                    self.pc,
                    0x00, // Interrupt marker
                    match interrupt {
                        Interrupt::Nmi => "NMI",
                        Interrupt::Irq => "IRQ",
                        _ => "???",
                    },
                    self.a, self.x, self.y, self.status, self.sp,
                    trace_state.get_cycle_count()
                );
            }
            self.cycles += cycles;
            return cycles;
        }

        if self.stall_cycles > 0 {
            self.stall_cycles -= 1;
            cycles += 1;
            self.cycles += cycles;
            return cycles;
        }

        let pc_before = self.pc;
        let opcode = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        
        // Read operands for disassembly (peek ahead, don't advance PC)
        let operand1 = if self.needs_operand1(opcode) {
            Some(bus.read(self.pc))
        } else {
            None
        };
        let operand2 = if self.needs_operand2(opcode) {
            Some(bus.read(self.pc.wrapping_add(1)))
        } else {
            None
        };
        
        // Restore PC for actual execution
        self.pc = pc_before.wrapping_add(1);

        if trace_state.enabled {
            // nestest format: PC opcode_bytes disassembly A:XX X:XX Y:XX P:XX SP:XX CYC:XXX
            let opcode_bytes = match (operand1, operand2) {
                (Some(b1), Some(b2)) => format!("{:02X} {:02X} {:02X}", opcode, b1, b2),
                (Some(b1), None) => format!("{:02X} {:02X}   ", opcode, b1),
                _ => format!("{:02X}        ", opcode),
            };
            let disasm = disassemble_instruction(opcode, operand1, operand2);
            println!("{:04X} {} {:20} A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC:{}",
                pc_before,
                opcode_bytes,
                disasm,
                self.a, self.x, self.y, self.status, self.sp,
                trace_state.get_cycle_count()
            );
        } else {
            trace!(
                "{:04X} {:02X} A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X}",
                pc_before,
                opcode,
                self.a,
                self.x,
                self.y,
                self.status,
                self.sp
            );
        }

        cycles += self.execute(opcode, bus);
        self.cycles += cycles;
        cycles
    }
    
    fn needs_operand1(&self, opcode: u8) -> bool {
        // Instructions that DON'T need operand bytes (implied/accumulator addressing)
        !matches!(opcode,
            0x00 | 0x08 | 0x0A | 0x18 | 0x28 | 0x2A | 0x38 | 0x40 | 0x48 | 0x4A | 0x58 | 0x60 | 0x68 | 0x6A | 0x78 | 0x88 | 0x8A | 0x98 | 0x9A | 0xA8 | 0xAA | 0xB8 | 0xBA | 0xC8 | 0xCA | 0xD8 | 0xE8 | 0xEA | 0xF8
        ) // All others need at least 1 operand
    }
    
    fn needs_operand2(&self, opcode: u8) -> bool {
        // Instructions that need 2 operand bytes (absolute addressing, JSR, JMP indirect)
        matches!(opcode,
            0x20 | 0x4C | 0x6C // JSR, JMP absolute, JMP indirect
        ) || matches!(opcode,
            0x0D | 0x0E | 0x19 | 0x1D | 0x1E | 0x2C | 0x2D | 0x2E | 0x39 | 0x3D | 0x3E | 0x4D | 0x4E | 0x59 | 0x5D | 0x5E | 0x6D | 0x6E | 0x79 | 0x7D | 0x7E | 0x8C | 0x8D | 0x8E | 0x99 | 0x9D | 0xAC | 0xAD | 0xAE | 0xB9 | 0xBC | 0xBD | 0xBE | 0xCC | 0xCD | 0xCE | 0xD9 | 0xDD | 0xDE | 0xEC | 0xED | 0xEE | 0xF9 | 0xFD | 0xFE
        ) // Absolute addressing modes
    }

    fn check_interrupts(&self, _bus: &mut dyn CpuBus) -> Interrupt {
        // This will be handled by the emulator core checking PPU NMI
        Interrupt::None
    }
    
    /// Helper function to handle writes that might be OAMDMA
    /// Returns true if OAMDMA was triggered (and stall_cycles was set)
    fn handle_write_with_oamdma(&mut self, bus: &mut dyn CpuBus, addr: u16, value: u8) -> bool {
        if addr == 0x4014 {
            // OAMDMA: stall CPU for 513-514 cycles depending on alignment
            // If current cycle count is odd: 513 cycles, if even: 514 cycles
            let current_cycle_odd = (self.cycles & 1) != 0;
            let stall_cycles = bus.trigger_oamdma(value, current_cycle_odd);
            self.stall_cycles = stall_cycles;
            bus.write(addr, value);
            true
        } else {
            bus.write(addr, value);
            false
        }
    }

    pub fn trigger_nmi(&mut self, bus: &mut dyn CpuBus) {
        self.push_word(self.pc, bus);
        // NMI should push status with B flag clear (unlike BRK which sets B)
        self.push((self.status & !FLAG_B) | FLAG_U, bus);
        self.set_flag(FLAG_I, true);
        self.pc = self.read_word(0xFFFA, bus);
        self.cycles += 7;
    }

    pub fn trigger_irq(&mut self, bus: &mut dyn CpuBus) {
        if !self.get_flag(FLAG_I) {
            self.push_word(self.pc, bus);
            self.push(self.status | FLAG_B | FLAG_U, bus);
            self.set_flag(FLAG_I, true);
            self.pc = self.read_word(0xFFFE, bus);
            self.cycles += 7;
        }
    }

    fn handle_interrupt(&mut self, interrupt: Interrupt, bus: &mut dyn CpuBus) -> u64 {
        match interrupt {
            Interrupt::Nmi => {
                self.trigger_nmi(bus);
                7
            }
            Interrupt::Irq => {
                self.trigger_irq(bus);
                7
            }
            _ => 0,
        }
    }

    fn execute(&mut self, opcode: u8, bus: &mut dyn CpuBus) -> u64 {
        match opcode {
            // BRK
            0x00 => {
                self.pc = self.pc.wrapping_add(1);
                self.push_word(self.pc, bus);
                self.push(self.status | FLAG_B | FLAG_U, bus);
                self.set_flag(FLAG_I, true);
                self.pc = self.read_word(0xFFFE, bus);
                7
            }
            // ORA (indirect, X)
            0x01 => {
                let (addr, extra_cycle) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                self.ora(value);
                6 + extra_cycle
            }
            // ORA (zero page)
            0x05 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                self.ora(value);
                3
            }
            // ASL (zero page)
            0x06 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                let result = self.asl(value);
                bus.write(addr, result);
                5
            }
            // PHP
            0x08 => {
                self.push(self.status | FLAG_B | FLAG_U, bus);
                3
            }
            // ORA (immediate)
            0x09 => {
                let value = self.addr_immediate(bus);
                self.ora(value);
                2
            }
            // ASL A
            0x0A => {
                self.a = self.asl(self.a);
                2
            }
            // ORA (absolute)
            0x0D => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                self.ora(value);
                4
            }
            // ASL (absolute)
            0x0E => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                let result = self.asl(value);
                bus.write(addr, result);
                6
            }
            // BPL
            0x10 => self.branch(!self.get_flag(FLAG_N), bus),
            // ORA (indirect), Y
            0x11 => {
                let (addr, extra_cycle) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                self.ora(value);
                5 + extra_cycle
            }
            // ORA (zero page, X)
            0x15 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                self.ora(value);
                4
            }
            // ASL (zero page, X)
            0x16 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                let result = self.asl(value);
                bus.write(addr, result);
                6
            }
            // CLC
            0x18 => {
                self.set_flag(FLAG_C, false);
                2
            }
            // ORA (absolute, Y)
            0x19 => {
                let (addr, extra_cycle) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                self.ora(value);
                4 + extra_cycle
            }
            // ORA (absolute, X)
            0x1D => {
                let (addr, extra_cycle) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                self.ora(value);
                4 + extra_cycle
            }
            // ASL (absolute, X)
            0x1E => {
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                let result = self.asl(value);
                bus.write(addr, result);
                7
            }
            // JSR
            0x20 => {
                let addr = self.addr_absolute(bus);
                self.push_word(self.pc.wrapping_sub(1), bus);
                self.pc = addr;
                6
            }
            // AND (indirect, X)
            0x21 => {
                let (addr, extra_cycle) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                self.and(value);
                6 + extra_cycle
            }
            // BIT (zero page)
            0x24 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                self.bit(value);
                3
            }
            // AND (zero page)
            0x25 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                self.and(value);
                3
            }
            // ROL (zero page)
            0x26 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                let result = self.rol(value);
                bus.write(addr, result);
                5
            }
            // PLP
            0x28 => {
                self.status = (self.pop(bus) & !FLAG_B) | FLAG_U;
                4
            }
            // AND (immediate)
            0x29 => {
                let value = self.addr_immediate(bus);
                self.and(value);
                2
            }
            // ROL A
            0x2A => {
                self.a = self.rol(self.a);
                2
            }
            // BIT (absolute)
            0x2C => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                self.bit(value);
                4
            }
            // AND (absolute)
            0x2D => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                self.and(value);
                4
            }
            // ROL (absolute)
            0x2E => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                let result = self.rol(value);
                bus.write(addr, result);
                6
            }
            // BMI
            0x30 => self.branch(self.get_flag(FLAG_N), bus),
            // AND (indirect), Y
            0x31 => {
                let (addr, extra_cycle) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                self.and(value);
                5 + extra_cycle
            }
            // AND (zero page, X)
            0x35 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                self.and(value);
                4
            }
            // ROL (zero page, X)
            0x36 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                let result = self.rol(value);
                bus.write(addr, result);
                6
            }
            // SEC
            0x38 => {
                self.set_flag(FLAG_C, true);
                2
            }
            // AND (absolute, Y)
            0x39 => {
                let (addr, extra_cycle) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                self.and(value);
                4 + extra_cycle
            }
            // AND (absolute, X)
            0x3D => {
                let (addr, extra_cycle) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                self.and(value);
                4 + extra_cycle
            }
            // ROL (absolute, X)
            0x3E => {
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                let result = self.rol(value);
                bus.write(addr, result);
                7
            }
            // RTI
            0x40 => {
                self.status = (self.pop(bus) & !FLAG_B) | FLAG_U;
                self.pc = self.pop_word(bus);
                6
            }
            // EOR (indirect, X)
            0x41 => {
                let (addr, extra_cycle) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                self.eor(value);
                6 + extra_cycle
            }
            // EOR (zero page)
            0x45 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                self.eor(value);
                3
            }
            // LSR (zero page)
            0x46 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                let result = self.lsr(value);
                bus.write(addr, result);
                5
            }
            // PHA
            0x48 => {
                self.push(self.a, bus);
                3
            }
            // EOR (immediate)
            0x49 => {
                let value = self.addr_immediate(bus);
                self.eor(value);
                2
            }
            // LSR A
            0x4A => {
                self.a = self.lsr(self.a);
                2
            }
            // JMP (absolute)
            0x4C => {
                self.pc = self.addr_absolute(bus);
                3
            }
            // EOR (absolute)
            0x4D => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                self.eor(value);
                4
            }
            // LSR (absolute)
            0x4E => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                let result = self.lsr(value);
                bus.write(addr, result);
                6
            }
            // BVC
            0x50 => self.branch(!self.get_flag(FLAG_V), bus),
            // EOR (indirect), Y
            0x51 => {
                let (addr, extra_cycle) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                self.eor(value);
                5 + extra_cycle
            }
            // EOR (zero page, X)
            0x55 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                self.eor(value);
                4
            }
            // LSR (zero page, X)
            0x56 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                let result = self.lsr(value);
                bus.write(addr, result);
                6
            }
            // CLI
            0x58 => {
                self.set_flag(FLAG_I, false);
                2
            }
            // EOR (absolute, Y)
            0x59 => {
                let (addr, extra_cycle) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                self.eor(value);
                4 + extra_cycle
            }
            // EOR (absolute, X)
            0x5D => {
                let (addr, extra_cycle) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                self.eor(value);
                4 + extra_cycle
            }
            // LSR (absolute, X)
            0x5E => {
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                let result = self.lsr(value);
                bus.write(addr, result);
                7
            }
            // RTS
            0x60 => {
                self.pc = self.pop_word(bus).wrapping_add(1);
                6
            }
            // ADC (indirect, X)
            0x61 => {
                let (addr, extra_cycle) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                self.adc(value);
                6 + extra_cycle
            }
            // ADC (zero page)
            0x65 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                self.adc(value);
                3
            }
            // ROR (zero page)
            0x66 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                let result = self.ror(value);
                bus.write(addr, result);
                5
            }
            // PLA
            0x68 => {
                self.a = self.pop(bus);
                self.update_zero_negative(self.a);
                4
            }
            // ADC (immediate)
            0x69 => {
                let value = self.addr_immediate(bus);
                self.adc(value);
                2
            }
            // ROR A
            0x6A => {
                self.a = self.ror(self.a);
                2
            }
            // JMP (indirect)
            0x6C => {
                let indirect_addr = self.addr_absolute(bus);
                // 6502 bug: doesn't increment page on indirect jump
                let addr = if (indirect_addr & 0xFF) == 0xFF {
                    (bus.read(indirect_addr) as u16)
                        | ((bus.read(indirect_addr & 0xFF00) as u16) << 8)
                } else {
                    self.read_word(indirect_addr, bus)
                };
                self.pc = addr;
                5
            }
            // ADC (absolute)
            0x6D => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                self.adc(value);
                4
            }
            // ROR (absolute)
            0x6E => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                let result = self.ror(value);
                bus.write(addr, result);
                6
            }
            // BVS
            0x70 => self.branch(self.get_flag(FLAG_V), bus),
            // ADC (indirect), Y
            0x71 => {
                let (addr, extra_cycle) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                self.adc(value);
                5 + extra_cycle
            }
            // ADC (zero page, X)
            0x75 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                self.adc(value);
                4
            }
            // ROR (zero page, X)
            0x76 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                let result = self.ror(value);
                bus.write(addr, result);
                6
            }
            // SEI
            0x78 => {
                self.set_flag(FLAG_I, true);
                2
            }
            // ADC (absolute, Y)
            0x79 => {
                let (addr, extra_cycle) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                self.adc(value);
                4 + extra_cycle
            }
            // ADC (absolute, X)
            0x7D => {
                let (addr, extra_cycle) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                self.adc(value);
                4 + extra_cycle
            }
            // ROR (absolute, X)
            0x7E => {
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                let result = self.ror(value);
                bus.write(addr, result);
                7
            }
            // STA (indirect, X)
            0x81 => {
                let (addr, _) = self.addr_indirect_x(bus);
                self.handle_write_with_oamdma(bus, addr, self.a);
                6
            }
            // STY (zero page)
            0x84 => {
                let addr = self.addr_zero_page(bus);
                bus.write(addr, self.y);
                3
            }
            // STA (zero page)
            0x85 => {
                let addr = self.addr_zero_page(bus);
                self.handle_write_with_oamdma(bus, addr, self.a);
                3
            }
            // STX (zero page)
            0x86 => {
                let addr = self.addr_zero_page(bus);
                bus.write(addr, self.x);
                3
            }
            // DEY
            0x88 => {
                self.y = self.y.wrapping_sub(1);
                self.update_zero_negative(self.y);
                2
            }
            // TXA
            0x8A => {
                self.a = self.x;
                self.update_zero_negative(self.a);
                2
            }
            // STY (absolute)
            0x8C => {
                let addr = self.addr_absolute(bus);
                bus.write(addr, self.y);
                4
            }
            // STA (absolute)
            0x8D => {
                let addr = self.addr_absolute(bus);
                self.handle_write_with_oamdma(bus, addr, self.a);
                4
            }
            // STX (absolute)
            0x8E => {
                let addr = self.addr_absolute(bus);
                bus.write(addr, self.x);
                4
            }
            // BCC
            0x90 => self.branch(!self.get_flag(FLAG_C), bus),
            // STA (indirect), Y
            0x91 => {
                let (addr, _) = self.addr_indirect_y(bus);
                self.handle_write_with_oamdma(bus, addr, self.a);
                6
            }
            // STY (zero page, X)
            0x94 => {
                let addr = self.addr_zero_page_x(bus);
                bus.write(addr, self.y);
                4
            }
            // STA (zero page, X)
            0x95 => {
                let addr = self.addr_zero_page_x(bus);
                self.handle_write_with_oamdma(bus, addr, self.a);
                4
            }
            // STX (zero page, Y)
            0x96 => {
                let addr = self.addr_zero_page_y(bus);
                bus.write(addr, self.x);
                4
            }
            // TYA
            0x98 => {
                self.a = self.y;
                self.update_zero_negative(self.a);
                2
            }
            // STA (absolute, Y)
            0x99 => {
                let (addr, _) = self.addr_absolute_y(bus);
                self.handle_write_with_oamdma(bus, addr, self.a);
                5
            }
            // TXS
            0x9A => {
                self.sp = self.x;
                2
            }
            // STA (absolute, X)
            0x9D => {
                let (addr, _) = self.addr_absolute_x(bus);
                self.handle_write_with_oamdma(bus, addr, self.a);
                5
            }
            // LDY (immediate)
            0xA0 => {
                self.y = self.addr_immediate(bus);
                self.update_zero_negative(self.y);
                2
            }
            // LDA (indirect, X)
            0xA1 => {
                let (addr, extra_cycle) = self.addr_indirect_x(bus);
                self.a = bus.read(addr);
                self.update_zero_negative(self.a);
                6 + extra_cycle
            }
            // LDX (immediate)
            0xA2 => {
                self.x = self.addr_immediate(bus);
                self.update_zero_negative(self.x);
                2
            }
            // LDY (zero page)
            0xA4 => {
                let addr = self.addr_zero_page(bus);
                self.y = bus.read(addr);
                self.update_zero_negative(self.y);
                3
            }
            // LDA (zero page)
            0xA5 => {
                let addr = self.addr_zero_page(bus);
                self.a = bus.read(addr);
                self.update_zero_negative(self.a);
                3
            }
            // LDX (zero page)
            0xA6 => {
                let addr = self.addr_zero_page(bus);
                self.x = bus.read(addr);
                self.update_zero_negative(self.x);
                3
            }
            // TAY
            0xA8 => {
                self.y = self.a;
                self.update_zero_negative(self.y);
                2
            }
            // LDA (immediate)
            0xA9 => {
                self.a = self.addr_immediate(bus);
                self.update_zero_negative(self.a);
                2
            }
            // TAX
            0xAA => {
                self.x = self.a;
                self.update_zero_negative(self.x);
                2
            }
            // LDY (absolute)
            0xAC => {
                let addr = self.addr_absolute(bus);
                self.y = bus.read(addr);
                self.update_zero_negative(self.y);
                4
            }
            // LDA (absolute)
            0xAD => {
                let addr = self.addr_absolute(bus);
                self.a = bus.read(addr);
                self.update_zero_negative(self.a);
                4
            }
            // LDX (absolute)
            0xAE => {
                let addr = self.addr_absolute(bus);
                self.x = bus.read(addr);
                self.update_zero_negative(self.x);
                4
            }
            // BCS
            0xB0 => self.branch(self.get_flag(FLAG_C), bus),
            // LDA (indirect), Y
            0xB1 => {
                let (addr, extra_cycle) = self.addr_indirect_y(bus);
                self.a = bus.read(addr);
                self.update_zero_negative(self.a);
                5 + extra_cycle
            }
            // LDY (zero page, X)
            0xB4 => {
                let addr = self.addr_zero_page_x(bus);
                self.y = bus.read(addr);
                self.update_zero_negative(self.y);
                4
            }
            // LDA (zero page, X)
            0xB5 => {
                let addr = self.addr_zero_page_x(bus);
                self.a = bus.read(addr);
                self.update_zero_negative(self.a);
                4
            }
            // LDX (zero page, Y)
            0xB6 => {
                let addr = self.addr_zero_page_y(bus);
                self.x = bus.read(addr);
                self.update_zero_negative(self.x);
                4
            }
            // CLV
            0xB8 => {
                self.set_flag(FLAG_V, false);
                2
            }
            // LDA (absolute, Y)
            0xB9 => {
                let (addr, extra_cycle) = self.addr_absolute_y(bus);
                self.a = bus.read(addr);
                self.update_zero_negative(self.a);
                4 + extra_cycle
            }
            // TSX
            0xBA => {
                self.x = self.sp;
                self.update_zero_negative(self.x);
                2
            }
            // LDY (absolute, X)
            0xBC => {
                let (addr, extra_cycle) = self.addr_absolute_x(bus);
                self.y = bus.read(addr);
                self.update_zero_negative(self.y);
                4 + extra_cycle
            }
            // LDA (absolute, X)
            0xBD => {
                let (addr, extra_cycle) = self.addr_absolute_x(bus);
                self.a = bus.read(addr);
                self.update_zero_negative(self.a);
                4 + extra_cycle
            }
            // LDX (absolute, Y)
            0xBE => {
                let (addr, extra_cycle) = self.addr_absolute_y(bus);
                self.x = bus.read(addr);
                self.update_zero_negative(self.x);
                4 + extra_cycle
            }
            // CPY (immediate)
            0xC0 => {
                let value = self.addr_immediate(bus);
                self.cpy(value);
                2
            }
            // CMP (indirect, X)
            0xC1 => {
                let (addr, extra_cycle) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                self.cmp(value);
                6 + extra_cycle
            }
            // CPY (zero page)
            0xC4 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                self.cpy(value);
                3
            }
            // CMP (zero page)
            0xC5 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                self.cmp(value);
                3
            }
            // DEC (zero page)
            0xC6 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.update_zero_negative(value);
                5
            }
            // INY
            0xC8 => {
                self.y = self.y.wrapping_add(1);
                self.update_zero_negative(self.y);
                2
            }
            // CMP (immediate)
            0xC9 => {
                let value = self.addr_immediate(bus);
                self.cmp(value);
                2
            }
            // DEX
            0xCA => {
                self.x = self.x.wrapping_sub(1);
                self.update_zero_negative(self.x);
                2
            }
            // CPY (absolute)
            0xCC => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                self.cpy(value);
                4
            }
            // CMP (absolute)
            0xCD => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                self.cmp(value);
                4
            }
            // DEC (absolute)
            0xCE => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.update_zero_negative(value);
                6
            }
            // BNE
            0xD0 => self.branch(!self.get_flag(FLAG_Z), bus),
            // CMP (indirect), Y
            0xD1 => {
                let (addr, extra_cycle) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                self.cmp(value);
                5 + extra_cycle
            }
            // CMP (zero page, X)
            0xD5 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                self.cmp(value);
                4
            }
            // DEC (zero page, X)
            0xD6 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.update_zero_negative(value);
                6
            }
            // CLD
            0xD8 => {
                self.set_flag(FLAG_D, false);
                2
            }
            // CMP (absolute, Y)
            0xD9 => {
                let (addr, extra_cycle) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                self.cmp(value);
                4 + extra_cycle
            }
            // CMP (absolute, X)
            0xDD => {
                let (addr, extra_cycle) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                self.cmp(value);
                4 + extra_cycle
            }
            // DEC (absolute, X)
            0xDE => {
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.update_zero_negative(value);
                7
            }
            // CPX (immediate)
            0xE0 => {
                let value = self.addr_immediate(bus);
                self.cpx(value);
                2
            }
            // SBC (indirect, X)
            0xE1 => {
                let (addr, extra_cycle) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                self.sbc(value);
                6 + extra_cycle
            }
            // CPX (zero page)
            0xE4 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                self.cpx(value);
                3
            }
            // SBC (zero page)
            0xE5 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                self.sbc(value);
                3
            }
            // INC (zero page)
            0xE6 => {
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.update_zero_negative(value);
                5
            }
            // INX
            0xE8 => {
                self.x = self.x.wrapping_add(1);
                self.update_zero_negative(self.x);
                2
            }
            // SBC (immediate)
            0xE9 => {
                let value = self.addr_immediate(bus);
                self.sbc(value);
                2
            }
            // NOP
            0xEA => 2,
            // CPX (absolute)
            0xEC => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                self.cpx(value);
                4
            }
            // SBC (absolute)
            0xED => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                self.sbc(value);
                4
            }
            // INC (absolute)
            0xEE => {
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.update_zero_negative(value);
                6
            }
            // BEQ
            0xF0 => self.branch(self.get_flag(FLAG_Z), bus),
            // SBC (indirect), Y
            0xF1 => {
                let (addr, extra_cycle) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                self.sbc(value);
                5 + extra_cycle
            }
            // SBC (zero page, X)
            0xF5 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                self.sbc(value);
                4
            }
            // INC (zero page, X)
            0xF6 => {
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.update_zero_negative(value);
                6
            }
            // SED
            0xF8 => {
                self.set_flag(FLAG_D, true);
                2
            }
            // SBC (absolute, Y)
            0xF9 => {
                let (addr, extra_cycle) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                self.sbc(value);
                4 + extra_cycle
            }
            // SBC (absolute, X)
            0xFD => {
                let (addr, extra_cycle) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                self.sbc(value);
                4 + extra_cycle
            }
            // INC (absolute, X)
            0xFE => {
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.update_zero_negative(value);
                7
            }

            // ============================================
            // UNOFFICIAL/ILLEGAL OPCODES
            // ============================================

            // *SLO - ASL + ORA (Shift Left then OR with Accumulator)
            0x03 => { // (indirect, X)
                let (addr, _) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                let shifted = self.asl(value);
                bus.write(addr, shifted);
                self.ora(shifted);
                8
            }
            0x07 => { // zero page
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                let shifted = self.asl(value);
                bus.write(addr, shifted);
                self.ora(shifted);
                5
            }
            0x0F => { // absolute
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                let shifted = self.asl(value);
                bus.write(addr, shifted);
                self.ora(shifted);
                6
            }
            0x13 => { // (indirect), Y
                let (addr, _) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                let shifted = self.asl(value);
                bus.write(addr, shifted);
                self.ora(shifted);
                8
            }
            0x17 => { // zero page, X
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                let shifted = self.asl(value);
                bus.write(addr, shifted);
                self.ora(shifted);
                6
            }
            0x1B => { // absolute, Y
                let (addr, _) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                let shifted = self.asl(value);
                bus.write(addr, shifted);
                self.ora(shifted);
                7
            }
            0x1F => { // absolute, X
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                let shifted = self.asl(value);
                bus.write(addr, shifted);
                self.ora(shifted);
                7
            }

            // *RLA - ROL + AND (Rotate Left then AND with Accumulator)
            0x23 => { // (indirect, X)
                let (addr, _) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                let rotated = self.rol(value);
                bus.write(addr, rotated);
                self.and(rotated);
                8
            }
            0x27 => { // zero page
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                let rotated = self.rol(value);
                bus.write(addr, rotated);
                self.and(rotated);
                5
            }
            0x2F => { // absolute
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                let rotated = self.rol(value);
                bus.write(addr, rotated);
                self.and(rotated);
                6
            }
            0x33 => { // (indirect), Y
                let (addr, _) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                let rotated = self.rol(value);
                bus.write(addr, rotated);
                self.and(rotated);
                8
            }
            0x37 => { // zero page, X
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                let rotated = self.rol(value);
                bus.write(addr, rotated);
                self.and(rotated);
                6
            }
            0x3B => { // absolute, Y
                let (addr, _) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                let rotated = self.rol(value);
                bus.write(addr, rotated);
                self.and(rotated);
                7
            }
            0x3F => { // absolute, X
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                let rotated = self.rol(value);
                bus.write(addr, rotated);
                self.and(rotated);
                7
            }

            // *SRE - LSR + EOR (Shift Right then XOR with Accumulator)
            0x43 => { // (indirect, X)
                let (addr, _) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                let shifted = self.lsr(value);
                bus.write(addr, shifted);
                self.eor(shifted);
                8
            }
            0x47 => { // zero page
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                let shifted = self.lsr(value);
                bus.write(addr, shifted);
                self.eor(shifted);
                5
            }
            0x4F => { // absolute
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                let shifted = self.lsr(value);
                bus.write(addr, shifted);
                self.eor(shifted);
                6
            }
            0x53 => { // (indirect), Y
                let (addr, _) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                let shifted = self.lsr(value);
                bus.write(addr, shifted);
                self.eor(shifted);
                8
            }
            0x57 => { // zero page, X
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                let shifted = self.lsr(value);
                bus.write(addr, shifted);
                self.eor(shifted);
                6
            }
            0x5B => { // absolute, Y
                let (addr, _) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                let shifted = self.lsr(value);
                bus.write(addr, shifted);
                self.eor(shifted);
                7
            }
            0x5F => { // absolute, X
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                let shifted = self.lsr(value);
                bus.write(addr, shifted);
                self.eor(shifted);
                7
            }

            // *RRA - ROR + ADC (Rotate Right then Add with Carry)
            0x63 => { // (indirect, X)
                let (addr, _) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                let rotated = self.ror(value);
                bus.write(addr, rotated);
                self.adc(rotated);
                8
            }
            0x67 => { // zero page
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                let rotated = self.ror(value);
                bus.write(addr, rotated);
                self.adc(rotated);
                5
            }
            0x6F => { // absolute
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                let rotated = self.ror(value);
                bus.write(addr, rotated);
                self.adc(rotated);
                6
            }
            0x73 => { // (indirect), Y
                let (addr, _) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                let rotated = self.ror(value);
                bus.write(addr, rotated);
                self.adc(rotated);
                8
            }
            0x77 => { // zero page, X
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr);
                let rotated = self.ror(value);
                bus.write(addr, rotated);
                self.adc(rotated);
                6
            }
            0x7B => { // absolute, Y
                let (addr, _) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                let rotated = self.ror(value);
                bus.write(addr, rotated);
                self.adc(rotated);
                7
            }
            0x7F => { // absolute, X
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr);
                let rotated = self.ror(value);
                bus.write(addr, rotated);
                self.adc(rotated);
                7
            }

            // *SAX - Store A & X (AND A with X, store result)
            0x83 => { // (indirect, X)
                let (addr, _) = self.addr_indirect_x(bus);
                bus.write(addr, self.a & self.x);
                6
            }
            0x87 => { // zero page
                let addr = self.addr_zero_page(bus);
                bus.write(addr, self.a & self.x);
                3
            }
            0x8F => { // absolute
                let addr = self.addr_absolute(bus);
                bus.write(addr, self.a & self.x);
                4
            }
            0x97 => { // zero page, Y
                let addr = self.addr_zero_page_y(bus);
                bus.write(addr, self.a & self.x);
                4
            }

            // *LAX - LDA + LDX (Load A and X with same value)
            0xA3 => { // (indirect, X)
                let (addr, extra_cycle) = self.addr_indirect_x(bus);
                let value = bus.read(addr);
                self.a = value;
                self.x = value;
                self.update_zero_negative(value);
                6 + extra_cycle
            }
            0xA7 => { // zero page
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr);
                self.a = value;
                self.x = value;
                self.update_zero_negative(value);
                3
            }
            0xAB => { // immediate (unstable)
                let value = self.addr_immediate(bus);
                self.a = value;
                self.x = value;
                self.update_zero_negative(value);
                2
            }
            0xAF => { // absolute
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr);
                self.a = value;
                self.x = value;
                self.update_zero_negative(value);
                4
            }
            0xB3 => { // (indirect), Y
                let (addr, extra_cycle) = self.addr_indirect_y(bus);
                let value = bus.read(addr);
                self.a = value;
                self.x = value;
                self.update_zero_negative(value);
                5 + extra_cycle
            }
            0xB7 => { // zero page, Y
                let addr = self.addr_zero_page_y(bus);
                let value = bus.read(addr);
                self.a = value;
                self.x = value;
                self.update_zero_negative(value);
                4
            }
            0xBF => { // absolute, Y
                let (addr, extra_cycle) = self.addr_absolute_y(bus);
                let value = bus.read(addr);
                self.a = value;
                self.x = value;
                self.update_zero_negative(value);
                4 + extra_cycle
            }

            // *DCP - DEC + CMP (Decrement memory then Compare)
            0xC3 => { // (indirect, X)
                let (addr, _) = self.addr_indirect_x(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.cmp(value);
                8
            }
            0xC7 => { // zero page
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.cmp(value);
                5
            }
            0xCF => { // absolute
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.cmp(value);
                6
            }
            0xD3 => { // (indirect), Y
                let (addr, _) = self.addr_indirect_y(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.cmp(value);
                8
            }
            0xD7 => { // zero page, X
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.cmp(value);
                6
            }
            0xDB => { // absolute, Y
                let (addr, _) = self.addr_absolute_y(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.cmp(value);
                7
            }
            0xDF => { // absolute, X
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr).wrapping_sub(1);
                bus.write(addr, value);
                self.cmp(value);
                7
            }

            // *ISB/ISC/INS - INC + SBC (Increment memory then Subtract)
            0xE3 => { // (indirect, X)
                let (addr, _) = self.addr_indirect_x(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.sbc(value);
                8
            }
            0xE7 => { // zero page
                let addr = self.addr_zero_page(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.sbc(value);
                5
            }
            0xEB => { // immediate (same as SBC #imm, unofficial)
                let value = self.addr_immediate(bus);
                self.sbc(value);
                2
            }
            0xEF => { // absolute
                let addr = self.addr_absolute(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.sbc(value);
                6
            }
            0xF3 => { // (indirect), Y
                let (addr, _) = self.addr_indirect_y(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.sbc(value);
                8
            }
            0xF7 => { // zero page, X
                let addr = self.addr_zero_page_x(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.sbc(value);
                6
            }
            0xFB => { // absolute, Y
                let (addr, _) = self.addr_absolute_y(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.sbc(value);
                7
            }
            0xFF => { // absolute, X (THIS IS THE ZELDA OPCODE!)
                let (addr, _) = self.addr_absolute_x(bus);
                let value = bus.read(addr).wrapping_add(1);
                bus.write(addr, value);
                self.sbc(value);
                7
            }

            // *ANC - AND + set Carry to bit 7
            0x0B | 0x2B => {
                let value = self.addr_immediate(bus);
                self.and(value);
                self.set_flag(FLAG_C, (self.a & 0x80) != 0);
                2
            }

            // *ALR/ASR - AND + LSR
            0x4B => {
                let value = self.addr_immediate(bus);
                self.a &= value;
                self.a = self.lsr(self.a);
                2
            }

            // *ARR - AND + ROR (with weird flag behavior)
            0x6B => {
                let value = self.addr_immediate(bus);
                self.a &= value;
                self.a = self.ror(self.a);
                // Special flag handling for ARR
                self.set_flag(FLAG_C, (self.a & 0x40) != 0);
                self.set_flag(FLAG_V, ((self.a & 0x40) ^ ((self.a & 0x20) << 1)) != 0);
                2
            }

            // *AXS/SBX - (A & X) - immediate -> X
            0xCB => {
                let value = self.addr_immediate(bus);
                let temp = (self.a & self.x).wrapping_sub(value);
                self.set_flag(FLAG_C, (self.a & self.x) >= value);
                self.x = temp;
                self.update_zero_negative(self.x);
                2
            }

            // *NOP variants (read and discard)
            0x04 | 0x44 | 0x64 => { // zero page NOP
                let _ = self.addr_zero_page(bus);
                3
            }
            0x0C => { // absolute NOP
                let addr = self.addr_absolute(bus);
                let _ = bus.read(addr);
                4
            }
            0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => { // zero page, X NOP
                let _ = self.addr_zero_page_x(bus);
                4
            }
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => { // implied NOP
                2
            }
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => { // absolute, X NOP
                let (addr, extra_cycle) = self.addr_absolute_x(bus);
                let _ = bus.read(addr);
                4 + extra_cycle
            }
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => { // immediate NOP
                let _ = self.addr_immediate(bus);
                2
            }

            _ => {
                // Remaining illegal opcodes - treat as NOP but log
                log::warn!("Unknown opcode: 0x{:02X} at PC 0x{:04X}", opcode, self.pc.wrapping_sub(1));
                2
            }
        }
    }

    // Addressing modes
    fn addr_immediate(&mut self, bus: &mut dyn CpuBus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    fn addr_zero_page(&mut self, bus: &mut dyn CpuBus) -> u16 {
        let addr = bus.read(self.pc) as u16;
        self.pc = self.pc.wrapping_add(1);
        addr
    }

    fn addr_zero_page_x(&mut self, bus: &mut dyn CpuBus) -> u16 {
        let base = bus.read(self.pc) as u16;
        self.pc = self.pc.wrapping_add(1);
        (base.wrapping_add(self.x as u16)) & 0xFF
    }

    fn addr_zero_page_y(&mut self, bus: &mut dyn CpuBus) -> u16 {
        let base = bus.read(self.pc) as u16;
        self.pc = self.pc.wrapping_add(1);
        (base.wrapping_add(self.y as u16)) & 0xFF
    }

    fn addr_absolute(&mut self, bus: &mut dyn CpuBus) -> u16 {
        let low = bus.read(self.pc) as u16;
        self.pc = self.pc.wrapping_add(1);
        let high = bus.read(self.pc) as u16;
        self.pc = self.pc.wrapping_add(1);
        (high << 8) | low
    }

    fn addr_absolute_x(&mut self, bus: &mut dyn CpuBus) -> (u16, u64) {
        let base = self.addr_absolute(bus);
        let addr = base.wrapping_add(self.x as u16);
        let extra_cycle = if (base & 0xFF00) != (addr & 0xFF00) { 1 } else { 0 };
        (addr, extra_cycle)
    }

    fn addr_absolute_y(&mut self, bus: &mut dyn CpuBus) -> (u16, u64) {
        let base = self.addr_absolute(bus);
        let addr = base.wrapping_add(self.y as u16);
        let extra_cycle = if (base & 0xFF00) != (addr & 0xFF00) { 1 } else { 0 };
        (addr, extra_cycle)
    }

    fn addr_indirect_x(&mut self, bus: &mut dyn CpuBus) -> (u16, u64) {
        let base = bus.read(self.pc) as u16;
        self.pc = self.pc.wrapping_add(1);
        let ptr = (base.wrapping_add(self.x as u16)) & 0xFF;
        let low = bus.read(ptr) as u16;
        let high = bus.read(ptr.wrapping_add(1) & 0xFF) as u16;
        ((high << 8) | low, 0)
    }

    fn addr_indirect_y(&mut self, bus: &mut dyn CpuBus) -> (u16, u64) {
        let base = bus.read(self.pc) as u16;
        self.pc = self.pc.wrapping_add(1);
        let low = bus.read(base) as u16;
        let high = bus.read((base + 1) & 0xFF) as u16;
        let indirect = (high << 8) | low;
        let addr = indirect.wrapping_add(self.y as u16);
        let extra_cycle = if (indirect & 0xFF00) != (addr & 0xFF00) { 1 } else { 0 };
        (addr, extra_cycle)
    }

    // Stack operations
    fn push(&mut self, value: u8, bus: &mut dyn CpuBus) {
        bus.write(0x100 + self.sp as u16, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pop(&mut self, bus: &mut dyn CpuBus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(0x100 + self.sp as u16)
    }

    fn push_word(&mut self, value: u16, bus: &mut dyn CpuBus) {
        self.push((value >> 8) as u8, bus);
        self.push(value as u8, bus);
    }

    fn pop_word(&mut self, bus: &mut dyn CpuBus) -> u16 {
        let low = self.pop(bus) as u16;
        let high = self.pop(bus) as u16;
        (high << 8) | low
    }

    fn read_word(&self, addr: u16, bus: &mut dyn CpuBus) -> u16 {
        let low = bus.read(addr) as u16;
        let high = bus.read(addr.wrapping_add(1)) as u16;
        (high << 8) | low
    }

    // Branch instructions
    fn branch(&mut self, condition: bool, bus: &mut dyn CpuBus) -> u64 {
        if condition {
            let offset = bus.read(self.pc) as i8;
            self.pc = self.pc.wrapping_add(1);
            let old_pc = self.pc;
            self.pc = self.pc.wrapping_add(offset as u16);
            let extra_cycle = if (old_pc & 0xFF00) != (self.pc & 0xFF00) { 1 } else { 0 };
            3 + extra_cycle
        } else {
            self.pc = self.pc.wrapping_add(1);
            2
        }
    }

    // ALU operations
    fn ora(&mut self, value: u8) {
        self.a |= value;
        self.update_zero_negative(self.a);
    }

    fn and(&mut self, value: u8) {
        self.a &= value;
        self.update_zero_negative(self.a);
    }

    fn eor(&mut self, value: u8) {
        self.a ^= value;
        self.update_zero_negative(self.a);
    }

    fn adc(&mut self, value: u8) {
        let carry = if self.get_flag(FLAG_C) { 1 } else { 0 };
        let sum = self.a as u16 + value as u16 + carry;
        let result = sum as u8;

        self.set_flag(FLAG_C, sum > 0xFF);
        self.set_flag(FLAG_V, ((self.a ^ result) & (value ^ result) & 0x80) != 0);
        self.a = result;
        self.update_zero_negative(self.a);
    }

    fn sbc(&mut self, value: u8) {
        let carry = if self.get_flag(FLAG_C) { 1 } else { 0 };
        let diff = self.a as i16 - value as i16 - (1 - carry);
        let result = diff as u8;

        self.set_flag(FLAG_C, diff >= 0);
        self.set_flag(FLAG_V, ((self.a ^ result) & (!value ^ result) & 0x80) != 0);
        self.a = result;
        self.update_zero_negative(self.a);
    }

    fn cmp(&mut self, value: u8) {
        let diff = self.a.wrapping_sub(value);
        self.set_flag(FLAG_C, self.a >= value);
        self.update_zero_negative(diff);
    }

    fn cpx(&mut self, value: u8) {
        let diff = self.x.wrapping_sub(value);
        self.set_flag(FLAG_C, self.x >= value);
        self.update_zero_negative(diff);
    }

    fn cpy(&mut self, value: u8) {
        let diff = self.y.wrapping_sub(value);
        self.set_flag(FLAG_C, self.y >= value);
        self.update_zero_negative(diff);
    }

    fn bit(&mut self, value: u8) {
        let result = self.a & value;
        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, (value & FLAG_N) != 0);
        self.set_flag(FLAG_V, (value & FLAG_V) != 0);
    }

    fn asl(&mut self, value: u8) -> u8 {
        self.set_flag(FLAG_C, (value & 0x80) != 0);
        let result = value << 1;
        self.update_zero_negative(result);
        result
    }

    fn lsr(&mut self, value: u8) -> u8 {
        self.set_flag(FLAG_C, (value & 0x01) != 0);
        let result = value >> 1;
        self.update_zero_negative(result);
        result
    }

    fn rol(&mut self, value: u8) -> u8 {
        let carry = if self.get_flag(FLAG_C) { 1 } else { 0 };
        self.set_flag(FLAG_C, (value & 0x80) != 0);
        let result = (value << 1) | carry;
        self.update_zero_negative(result);
        result
    }

    fn ror(&mut self, value: u8) -> u8 {
        let carry = if self.get_flag(FLAG_C) { 0x80 } else { 0 };
        self.set_flag(FLAG_C, (value & 0x01) != 0);
        let result = (value >> 1) | carry;
        self.update_zero_negative(result);
        result
    }

    // Flag operations
    fn get_flag(&self, flag: u8) -> bool {
        (self.status & flag) != 0
    }

    fn set_flag(&mut self, flag: u8, value: bool) {
        if value {
            self.status |= flag;
        } else {
            self.status &= !flag;
        }
    }

    fn update_zero_negative(&mut self, value: u8) {
        self.set_flag(FLAG_Z, value == 0);
        self.set_flag(FLAG_N, (value & FLAG_N) != 0);
    }
}
