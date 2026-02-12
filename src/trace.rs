// CPU instruction tracing for nestest compatibility

pub struct TraceState {
    pub enabled: bool,
    pub ppu_cycle_count: u64, // Track PPU cycles for CYC counter
}

impl TraceState {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ppu_cycle_count: 0,
        }
    }

    pub fn get_cycle_count(&self) -> u64 {
        // nestest uses PPU cycles / 3 (integer division, not rounding)
        // The cycle count represents CPU cycles, which run at 1/3 PPU speed
        // nestest counts from reset, so we need to match the exact counting method
        // Use integer division to match nestest's behavior
        self.ppu_cycle_count / 3
    }

    pub fn increment_ppu_cycles(&mut self, cycles: u64) {
        self.ppu_cycle_count += cycles;
    }
}

// Instruction disassembly helper
pub fn disassemble_instruction(opcode: u8, operand1: Option<u8>, operand2: Option<u8>) -> String {
    let (mnemonic, addr_mode) = get_opcode_info(opcode);

    match addr_mode {
        AddrMode::Implied => mnemonic.to_string(),
        AddrMode::Accumulator => format!("{} A", mnemonic),
        AddrMode::Immediate => {
            if let Some(b) = operand1 {
                format!("{} #${:02X}", mnemonic, b)
            } else {
                format!("{} #$??", mnemonic)
            }
        }
        AddrMode::ZeroPage => {
            if let Some(b) = operand1 {
                format!("{} ${:02X}", mnemonic, b)
            } else {
                format!("{} $??", mnemonic)
            }
        }
        AddrMode::ZeroPageX => {
            if let Some(b) = operand1 {
                format!("{} ${:02X},X", mnemonic, b)
            } else {
                format!("{} $??,X", mnemonic)
            }
        }
        AddrMode::ZeroPageY => {
            if let Some(b) = operand1 {
                format!("{} ${:02X},Y", mnemonic, b)
            } else {
                format!("{} $??,Y", mnemonic)
            }
        }
        AddrMode::Absolute => {
            if let (Some(lo), Some(hi)) = (operand1, operand2) {
                let addr = (hi as u16) << 8 | lo as u16;
                format!("{} ${:04X}", mnemonic, addr)
            } else {
                format!("{} $????", mnemonic)
            }
        }
        AddrMode::AbsoluteX => {
            if let (Some(lo), Some(hi)) = (operand1, operand2) {
                let addr = (hi as u16) << 8 | lo as u16;
                format!("{} ${:04X},X", mnemonic, addr)
            } else {
                format!("{} $????,X", mnemonic)
            }
        }
        AddrMode::AbsoluteY => {
            if let (Some(lo), Some(hi)) = (operand1, operand2) {
                let addr = (hi as u16) << 8 | lo as u16;
                format!("{} ${:04X},Y", mnemonic, addr)
            } else {
                format!("{} $????,Y", mnemonic)
            }
        }
        AddrMode::Indirect => {
            if let (Some(lo), Some(hi)) = (operand1, operand2) {
                let addr = (hi as u16) << 8 | lo as u16;
                format!("{} (${:04X})", mnemonic, addr)
            } else {
                format!("{} ($????)", mnemonic)
            }
        }
        AddrMode::IndirectX => {
            if let Some(b) = operand1 {
                format!("{} (${:02X},X)", mnemonic, b)
            } else {
                format!("{} ($??,X)", mnemonic)
            }
        }
        AddrMode::IndirectY => {
            if let Some(b) = operand1 {
                format!("{} (${:02X}),Y", mnemonic, b)
            } else {
                format!("{} ($??),Y", mnemonic)
            }
        }
        AddrMode::Relative => {
            if let Some(b) = operand1 {
                format!("{} ${:02X}", mnemonic, b)
            } else {
                format!("{} $??", mnemonic)
            }
        }
    }
}

enum AddrMode {
    Implied,
    Accumulator,
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    Indirect,
    IndirectX,
    IndirectY,
    Relative,
}

fn get_opcode_info(opcode: u8) -> (&'static str, AddrMode) {
    match opcode {
        0x00 => ("BRK", AddrMode::Implied),
        0x01 => ("ORA", AddrMode::IndirectX),
        0x05 => ("ORA", AddrMode::ZeroPage),
        0x06 => ("ASL", AddrMode::ZeroPage),
        0x08 => ("PHP", AddrMode::Implied),
        0x09 => ("ORA", AddrMode::Immediate),
        0x0A => ("ASL", AddrMode::Accumulator),
        0x0D => ("ORA", AddrMode::Absolute),
        0x0E => ("ASL", AddrMode::Absolute),
        0x10 => ("BPL", AddrMode::Relative),
        0x11 => ("ORA", AddrMode::IndirectY),
        0x15 => ("ORA", AddrMode::ZeroPageX),
        0x16 => ("ASL", AddrMode::ZeroPageX),
        0x18 => ("CLC", AddrMode::Implied),
        0x19 => ("ORA", AddrMode::AbsoluteY),
        0x1D => ("ORA", AddrMode::AbsoluteX),
        0x1E => ("ASL", AddrMode::AbsoluteX),
        0x20 => ("JSR", AddrMode::Absolute),
        0x21 => ("AND", AddrMode::IndirectX),
        0x24 => ("BIT", AddrMode::ZeroPage),
        0x25 => ("AND", AddrMode::ZeroPage),
        0x26 => ("ROL", AddrMode::ZeroPage),
        0x28 => ("PLP", AddrMode::Implied),
        0x29 => ("AND", AddrMode::Immediate),
        0x2A => ("ROL", AddrMode::Accumulator),
        0x2C => ("BIT", AddrMode::Absolute),
        0x2D => ("AND", AddrMode::Absolute),
        0x2E => ("ROL", AddrMode::Absolute),
        0x30 => ("BMI", AddrMode::Relative),
        0x31 => ("AND", AddrMode::IndirectY),
        0x35 => ("AND", AddrMode::ZeroPageX),
        0x36 => ("ROL", AddrMode::ZeroPageX),
        0x38 => ("SEC", AddrMode::Implied),
        0x39 => ("AND", AddrMode::AbsoluteY),
        0x3D => ("AND", AddrMode::AbsoluteX),
        0x3E => ("ROL", AddrMode::AbsoluteX),
        0x40 => ("RTI", AddrMode::Implied),
        0x41 => ("EOR", AddrMode::IndirectX),
        0x45 => ("EOR", AddrMode::ZeroPage),
        0x46 => ("LSR", AddrMode::ZeroPage),
        0x48 => ("PHA", AddrMode::Implied),
        0x49 => ("EOR", AddrMode::Immediate),
        0x4A => ("LSR", AddrMode::Accumulator),
        0x4C => ("JMP", AddrMode::Absolute),
        0x4D => ("EOR", AddrMode::Absolute),
        0x4E => ("LSR", AddrMode::Absolute),
        0x50 => ("BVC", AddrMode::Relative),
        0x51 => ("EOR", AddrMode::IndirectY),
        0x55 => ("EOR", AddrMode::ZeroPageX),
        0x56 => ("LSR", AddrMode::ZeroPageX),
        0x58 => ("CLI", AddrMode::Implied),
        0x59 => ("EOR", AddrMode::AbsoluteY),
        0x5D => ("EOR", AddrMode::AbsoluteX),
        0x5E => ("LSR", AddrMode::AbsoluteX),
        0x60 => ("RTS", AddrMode::Implied),
        0x61 => ("ADC", AddrMode::IndirectX),
        0x65 => ("ADC", AddrMode::ZeroPage),
        0x66 => ("ROR", AddrMode::ZeroPage),
        0x68 => ("PLA", AddrMode::Implied),
        0x69 => ("ADC", AddrMode::Immediate),
        0x6A => ("ROR", AddrMode::Accumulator),
        0x6C => ("JMP", AddrMode::Indirect),
        0x6D => ("ADC", AddrMode::Absolute),
        0x6E => ("ROR", AddrMode::Absolute),
        0x70 => ("BVS", AddrMode::Relative),
        0x71 => ("ADC", AddrMode::IndirectY),
        0x75 => ("ADC", AddrMode::ZeroPageX),
        0x76 => ("ROR", AddrMode::ZeroPageX),
        0x78 => ("SEI", AddrMode::Implied),
        0x79 => ("ADC", AddrMode::AbsoluteY),
        0x7D => ("ADC", AddrMode::AbsoluteX),
        0x7E => ("ROR", AddrMode::AbsoluteX),
        0x81 => ("STA", AddrMode::IndirectX),
        0x84 => ("STY", AddrMode::ZeroPage),
        0x85 => ("STA", AddrMode::ZeroPage),
        0x86 => ("STX", AddrMode::ZeroPage),
        0x88 => ("DEY", AddrMode::Implied),
        0x8A => ("TXA", AddrMode::Implied),
        0x8C => ("STY", AddrMode::Absolute),
        0x8D => ("STA", AddrMode::Absolute),
        0x8E => ("STX", AddrMode::Absolute),
        0x90 => ("BCC", AddrMode::Relative),
        0x91 => ("STA", AddrMode::IndirectY),
        0x94 => ("STY", AddrMode::ZeroPageX),
        0x95 => ("STA", AddrMode::ZeroPageX),
        0x96 => ("STX", AddrMode::ZeroPageY),
        0x98 => ("TYA", AddrMode::Implied),
        0x99 => ("STA", AddrMode::AbsoluteY),
        0x9A => ("TXS", AddrMode::Implied),
        0x9D => ("STA", AddrMode::AbsoluteX),
        0xA0 => ("LDY", AddrMode::Immediate),
        0xA1 => ("LDA", AddrMode::IndirectX),
        0xA2 => ("LDX", AddrMode::Immediate),
        0xA4 => ("LDY", AddrMode::ZeroPage),
        0xA5 => ("LDA", AddrMode::ZeroPage),
        0xA6 => ("LDX", AddrMode::ZeroPage),
        0xA8 => ("TAY", AddrMode::Implied),
        0xA9 => ("LDA", AddrMode::Immediate),
        0xAA => ("TAX", AddrMode::Implied),
        0xAC => ("LDY", AddrMode::Absolute),
        0xAD => ("LDA", AddrMode::Absolute),
        0xAE => ("LDX", AddrMode::Absolute),
        0xB0 => ("BCS", AddrMode::Relative),
        0xB1 => ("LDA", AddrMode::IndirectY),
        0xB4 => ("LDY", AddrMode::ZeroPageX),
        0xB5 => ("LDA", AddrMode::ZeroPageX),
        0xB6 => ("LDX", AddrMode::ZeroPageY),
        0xB8 => ("CLV", AddrMode::Implied),
        0xB9 => ("LDA", AddrMode::AbsoluteY),
        0xBA => ("TSX", AddrMode::Implied),
        0xBC => ("LDY", AddrMode::AbsoluteX),
        0xBD => ("LDA", AddrMode::AbsoluteX),
        0xBE => ("LDX", AddrMode::AbsoluteY),
        0xC0 => ("CPY", AddrMode::Immediate),
        0xC1 => ("CMP", AddrMode::IndirectX),
        0xC4 => ("CPY", AddrMode::ZeroPage),
        0xC5 => ("CMP", AddrMode::ZeroPage),
        0xC6 => ("DEC", AddrMode::ZeroPage),
        0xC8 => ("INY", AddrMode::Implied),
        0xC9 => ("CMP", AddrMode::Immediate),
        0xCA => ("DEX", AddrMode::Implied),
        0xCC => ("CPY", AddrMode::Absolute),
        0xCD => ("CMP", AddrMode::Absolute),
        0xCE => ("DEC", AddrMode::Absolute),
        0xD0 => ("BNE", AddrMode::Relative),
        0xD1 => ("CMP", AddrMode::IndirectY),
        0xD5 => ("CMP", AddrMode::ZeroPageX),
        0xD6 => ("DEC", AddrMode::ZeroPageX),
        0xD8 => ("CLD", AddrMode::Implied),
        0xD9 => ("CMP", AddrMode::AbsoluteY),
        0xDD => ("CMP", AddrMode::AbsoluteX),
        0xDE => ("DEC", AddrMode::AbsoluteX),
        0xE0 => ("CPX", AddrMode::Immediate),
        0xE1 => ("SBC", AddrMode::IndirectX),
        0xE4 => ("CPX", AddrMode::ZeroPage),
        0xE5 => ("SBC", AddrMode::ZeroPage),
        0xE6 => ("INC", AddrMode::ZeroPage),
        0xE8 => ("INX", AddrMode::Implied),
        0xE9 => ("SBC", AddrMode::Immediate),
        0xEA => ("NOP", AddrMode::Implied),
        0xEC => ("CPX", AddrMode::Absolute),
        0xED => ("SBC", AddrMode::Absolute),
        0xEE => ("INC", AddrMode::Absolute),
        0xF0 => ("BEQ", AddrMode::Relative),
        0xF1 => ("SBC", AddrMode::IndirectY),
        0xF5 => ("SBC", AddrMode::ZeroPageX),
        0xF6 => ("INC", AddrMode::ZeroPageX),
        0xF8 => ("SED", AddrMode::Implied),
        0xF9 => ("SBC", AddrMode::AbsoluteY),
        0xFD => ("SBC", AddrMode::AbsoluteX),
        0xFE => ("INC", AddrMode::AbsoluteX),

        // Unofficial opcodes
        // SLO
        0x03 => ("*SLO", AddrMode::IndirectX),
        0x07 => ("*SLO", AddrMode::ZeroPage),
        0x0F => ("*SLO", AddrMode::Absolute),
        0x13 => ("*SLO", AddrMode::IndirectY),
        0x17 => ("*SLO", AddrMode::ZeroPageX),
        0x1B => ("*SLO", AddrMode::AbsoluteY),
        0x1F => ("*SLO", AddrMode::AbsoluteX),
        // RLA
        0x23 => ("*RLA", AddrMode::IndirectX),
        0x27 => ("*RLA", AddrMode::ZeroPage),
        0x2F => ("*RLA", AddrMode::Absolute),
        0x33 => ("*RLA", AddrMode::IndirectY),
        0x37 => ("*RLA", AddrMode::ZeroPageX),
        0x3B => ("*RLA", AddrMode::AbsoluteY),
        0x3F => ("*RLA", AddrMode::AbsoluteX),
        // SRE
        0x43 => ("*SRE", AddrMode::IndirectX),
        0x47 => ("*SRE", AddrMode::ZeroPage),
        0x4F => ("*SRE", AddrMode::Absolute),
        0x53 => ("*SRE", AddrMode::IndirectY),
        0x57 => ("*SRE", AddrMode::ZeroPageX),
        0x5B => ("*SRE", AddrMode::AbsoluteY),
        0x5F => ("*SRE", AddrMode::AbsoluteX),
        // RRA
        0x63 => ("*RRA", AddrMode::IndirectX),
        0x67 => ("*RRA", AddrMode::ZeroPage),
        0x6F => ("*RRA", AddrMode::Absolute),
        0x73 => ("*RRA", AddrMode::IndirectY),
        0x77 => ("*RRA", AddrMode::ZeroPageX),
        0x7B => ("*RRA", AddrMode::AbsoluteY),
        0x7F => ("*RRA", AddrMode::AbsoluteX),
        // SAX
        0x83 => ("*SAX", AddrMode::IndirectX),
        0x87 => ("*SAX", AddrMode::ZeroPage),
        0x8F => ("*SAX", AddrMode::Absolute),
        0x97 => ("*SAX", AddrMode::ZeroPageY),
        // LAX
        0xA3 => ("*LAX", AddrMode::IndirectX),
        0xA7 => ("*LAX", AddrMode::ZeroPage),
        0xAB => ("*LAX", AddrMode::Immediate),
        0xAF => ("*LAX", AddrMode::Absolute),
        0xB3 => ("*LAX", AddrMode::IndirectY),
        0xB7 => ("*LAX", AddrMode::ZeroPageY),
        0xBF => ("*LAX", AddrMode::AbsoluteY),
        // DCP
        0xC3 => ("*DCP", AddrMode::IndirectX),
        0xC7 => ("*DCP", AddrMode::ZeroPage),
        0xCF => ("*DCP", AddrMode::Absolute),
        0xD3 => ("*DCP", AddrMode::IndirectY),
        0xD7 => ("*DCP", AddrMode::ZeroPageX),
        0xDB => ("*DCP", AddrMode::AbsoluteY),
        0xDF => ("*DCP", AddrMode::AbsoluteX),
        // ISB/ISC
        0xE3 => ("*ISB", AddrMode::IndirectX),
        0xE7 => ("*ISB", AddrMode::ZeroPage),
        0xEB => ("*SBC", AddrMode::Immediate), // unofficial SBC
        0xEF => ("*ISB", AddrMode::Absolute),
        0xF3 => ("*ISB", AddrMode::IndirectY),
        0xF7 => ("*ISB", AddrMode::ZeroPageX),
        0xFB => ("*ISB", AddrMode::AbsoluteY),
        0xFF => ("*ISB", AddrMode::AbsoluteX),
        // ANC
        0x0B | 0x2B => ("*ANC", AddrMode::Immediate),
        // ALR
        0x4B => ("*ALR", AddrMode::Immediate),
        // ARR
        0x6B => ("*ARR", AddrMode::Immediate),
        // AXS/SBX
        0xCB => ("*AXS", AddrMode::Immediate),
        // NOP variants
        0x04 | 0x44 | 0x64 => ("*NOP", AddrMode::ZeroPage),
        0x0C => ("*NOP", AddrMode::Absolute),
        0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => ("*NOP", AddrMode::ZeroPageX),
        0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => ("*NOP", AddrMode::Implied),
        0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => ("*NOP", AddrMode::AbsoluteX),
        0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => ("*NOP", AddrMode::Immediate),

        _ => ("???", AddrMode::Implied),
    }
}
