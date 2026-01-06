use std::fs::File;
use std::io::Read;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CartridgeError {
    #[error("Invalid iNES header")]
    InvalidHeader,
    #[error("Unsupported mapper: {0}")]
    UnsupportedMapper(u8),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub struct Cartridge {
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub mapper: Box<dyn Mapper>,
    pub mapper_id: u8,
    pub has_ram: bool,
    pub mirroring: Mirroring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    OneScreenLower,
    OneScreenUpper,
}

pub trait Mapper {
    fn cpu_read(&self, addr: u16, prg_rom: &[u8]) -> u8;
    fn cpu_write(&mut self, addr: u16, value: u8, prg_rom: &[u8], prg_ram: &mut [u8]);
    fn ppu_read(&self, addr: u16, chr_rom: &[u8], chr_ram: &[u8]) -> u8;
    fn ppu_write(&mut self, addr: u16, value: u8, chr_ram: &mut [u8]);
    fn mirroring(&self) -> Mirroring;
    /// Return true if mirroring changed during this write (for dynamic mirroring updates)
    fn mirroring_changed(&self) -> bool {
        false
    }
    /// Clock the scanline counter (for MMC3 IRQ). Returns true if IRQ should be triggered.
    fn clock_scanline(&mut self) -> bool {
        false
    }
    /// Check if mapper has a pending IRQ
    fn irq_pending(&self) -> bool {
        false
    }
    /// Acknowledge/clear pending IRQ
    fn acknowledge_irq(&mut self) {}
}

pub struct NromMapper {
    mirroring: Mirroring,
    has_chr_ram: bool,
}

pub struct UxromMapper {
    mirroring: Mirroring,
    has_chr_ram: bool,
    prg_bank: u8, // Current bank selected for 0x8000-0xBFFF
    prg_banks: u8, // Total number of PRG banks (16KB each)
}

pub struct CnromMapper {
    mirroring: Mirroring,
    has_chr_ram: bool,
    chr_bank: u8, // Current CHR bank selected (8KB banks)
    chr_banks: u8, // Total number of CHR banks (8KB each)
}

impl NromMapper {
    pub fn new(mirroring: Mirroring, has_chr_ram: bool) -> Self {
        Self {
            mirroring,
            has_chr_ram,
        }
    }
}

impl Mapper for NromMapper {
    fn cpu_read(&self, addr: u16, prg_rom: &[u8]) -> u8 {
        let addr = addr - 0x8000;
        if prg_rom.len() == 0x4000 {
            // 16KB PRG ROM, mirrored
            prg_rom[addr as usize % 0x4000]
        } else {
            // 32KB PRG ROM
            prg_rom[addr as usize]
        }
    }

    fn cpu_write(&mut self, _addr: u16, _value: u8, _prg_rom: &[u8], _prg_ram: &mut [u8]) {
        // NROM has no mapper registers
    }

    fn ppu_read(&self, addr: u16, chr_rom: &[u8], chr_ram: &[u8]) -> u8 {
        // PPU addresses 0x0000-0x1FFF map to pattern tables
        // Mask address to pattern table range (0x0000-0x1FFF)
        let pattern_addr = addr & 0x1FFF;
        
        if self.has_chr_ram {
            chr_ram[pattern_addr as usize]
        } else {
            // CHR ROM: use modulo to handle mirroring if address exceeds ROM size
            // For 8KB CHR ROM: addresses 0x0000-0x1FFF map directly
            // For 4KB CHR ROM: addresses 0x1000-0x1FFF mirror 0x0000-0x0FFF
            let idx = pattern_addr as usize;
            if idx < chr_rom.len() {
                chr_rom[idx]
            } else {
                // Mirror: if CHR ROM is 4KB and we're accessing 0x1000+, mirror to 0x0000+
                let mirrored_idx = idx % chr_rom.len();
                chr_rom[mirrored_idx]
            }
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8, chr_ram: &mut [u8]) {
        if self.has_chr_ram {
            chr_ram[addr as usize % 0x2000] = value;
        }
        // CHR ROM is read-only
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
    
    fn mirroring_changed(&self) -> bool {
        false
    }
}

impl UxromMapper {
    pub fn new(mirroring: Mirroring, has_chr_ram: bool, prg_rom_size: usize) -> Self {
        let prg_banks = (prg_rom_size / 0x4000) as u8; // Number of 16KB banks
        Self {
            mirroring,
            has_chr_ram,
            prg_bank: 0, // Start with first bank
            prg_banks,
        }
    }
}

impl CnromMapper {
    pub fn new(mirroring: Mirroring, has_chr_ram: bool, chr_rom_size: usize) -> Self {
        let chr_banks = if has_chr_ram { 0 } else { (chr_rom_size / 0x2000) as u8 }; // Number of 8KB banks
        Self {
            mirroring,
            has_chr_ram,
            chr_bank: 0, // Start with first bank
            chr_banks,
        }
    }
}

impl Mapper for UxromMapper {
    fn cpu_read(&self, addr: u16, prg_rom: &[u8]) -> u8 {
        if addr < 0xC000 {
            // First 16KB: bank-switchable (0x8000-0xBFFF)
            let bank_offset = (self.prg_bank as usize * 0x4000) % prg_rom.len();
            let addr_in_bank = (addr - 0x8000) as usize;
            prg_rom[(bank_offset + addr_in_bank) % prg_rom.len()]
        } else {
            // Last 16KB: fixed to last bank (0xC000-0xFFFF)
            let last_bank_start = ((self.prg_banks - 1) as usize * 0x4000) % prg_rom.len();
            let addr_in_bank = (addr - 0xC000) as usize;
            prg_rom[(last_bank_start + addr_in_bank) % prg_rom.len()]
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _prg_rom: &[u8], _prg_ram: &mut [u8]) {
        // Writing to 0x8000-0xFFFF selects the PRG bank for 0x8000-0xBFFF
        // From C reference: mapper->PRG_ptrs[0] = mapper->PRG_ROM + (value & 0x7) * 0x4000;
        if addr >= 0x8000 {
            // Match C reference: use 3 bits (0-7), modulo by available banks
            let selected_bank = (value & 0x07) as usize;
            self.prg_bank = (selected_bank % self.prg_banks as usize) as u8;
        }
    }

    fn ppu_read(&self, addr: u16, chr_rom: &[u8], chr_ram: &[u8]) -> u8 {
        // PPU addresses 0x0000-0x1FFF map to pattern tables
        let pattern_addr = addr & 0x1FFF;
        
        if self.has_chr_ram {
            chr_ram[pattern_addr as usize]
        } else {
            let idx = pattern_addr as usize;
            if idx < chr_rom.len() {
                chr_rom[idx]
            } else {
                // Mirror if CHR ROM is smaller than 8KB
                let mirrored_idx = idx % chr_rom.len();
                chr_rom[mirrored_idx]
            }
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8, chr_ram: &mut [u8]) {
        if self.has_chr_ram {
            chr_ram[addr as usize % 0x2000] = value;
        }
        // CHR ROM is read-only
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
    
    fn mirroring_changed(&self) -> bool {
        false
    }
}

impl Mapper for CnromMapper {
    fn cpu_read(&self, addr: u16, prg_rom: &[u8]) -> u8 {
        // CNROM: PRG ROM is not banked, always 32KB (or 16KB mirrored)
        let addr = addr - 0x8000;
        if prg_rom.len() == 0x4000 {
            // 16KB PRG ROM, mirrored
            prg_rom[addr as usize % 0x4000]
        } else {
            // 32KB PRG ROM
            prg_rom[addr as usize]
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _prg_rom: &[u8], _prg_ram: &mut [u8]) {
        // CNROM: Writing to ANY address in $8000-$FFFF selects CHR bank (8KB banks)
        // C reference: mask = mapper->CHR_banks > 4? 0xf : 0x3;
        // C reference: CHR_ptrs[0] = CHR_ROM + 0x2000 * (value & mask);
        // Critical: Must trigger on ANY write >= $8000, not just specific addresses
        // Critical: Use mask 0x03 for 4 banks (Paperboy), 0x0F for >4 banks
        if addr >= 0x8000 && !self.has_chr_ram {
            // Determine mask: 0x03 for <=4 banks, 0x0F for >4 banks
            let mask = if self.chr_banks > 4 { 0x0F } else { 0x03 };
            let new_bank = (value & mask) as u8;
            
            // Always update bank (even if same) to match C reference behavior
            if new_bank != self.chr_bank {
                log::info!("CNROM CHR bank switch: {} -> {} (value=0x{:02X}, mask=0x{:02X}, chr_banks={})", 
                    self.chr_bank, new_bank, value, mask, self.chr_banks);
            }
            self.chr_bank = new_bank;
        }
    }

    fn ppu_read(&self, addr: u16, chr_rom: &[u8], chr_ram: &[u8]) -> u8 {
        // CNROM: C reference does: return *(mapper->CHR_ptrs[0] + address);
        // The address is added directly to the bank pointer without masking
        // Address should be in 0x0000-0x1FFF range, but we mask for safety
        let pattern_addr = addr & 0x1FFF;
        
        if self.has_chr_ram {
            chr_ram[pattern_addr as usize]
        } else {
            // Bank-switchable CHR ROM (8KB banks)
            // C reference: CHR_ptrs[0] = CHR_ROM + 0x2000 * (value & mask)
            // Then: return *(CHR_ptrs[0] + address)
            // The address is added directly to the bank pointer
            let bank_offset = self.chr_bank as usize * 0x2000;
            let idx = bank_offset + (pattern_addr as usize);
            
            // Log first few reads to verify bank selection
            static mut READ_COUNT: u32 = 0;
            unsafe {
                if READ_COUNT < 20 {
                    log::info!("CNROM ppu_read: addr=0x{:04X}, pattern_addr=0x{:04X}, bank={}, bank_offset=0x{:04X}, idx=0x{:04X}, chr_rom_len=0x{:04X}", 
                        addr, pattern_addr, self.chr_bank, bank_offset, idx, chr_rom.len());
                    READ_COUNT += 1;
                }
            }
            
            // Bounds check - should never exceed ROM size with proper banking
            if idx < chr_rom.len() {
                chr_rom[idx]
            } else {
                // Safety fallback - shouldn't happen with proper banking
                log::warn!("CNROM ppu_read: idx 0x{:04X} exceeds chr_rom.len() 0x{:04X}", idx, chr_rom.len());
                0
            }
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8, chr_ram: &mut [u8]) {
        if self.has_chr_ram {
            chr_ram[addr as usize % 0x2000] = value;
        }
        // CHR ROM is read-only
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
    
    fn mirroring_changed(&self) -> bool {
        false
    }
}

// Helper function: next power of 2
fn next_power_of_2(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let mut power = 1;
    while power < n {
        power *= 2;
    }
    power
}

pub struct Mmc1Mapper {
    mirroring: Mirroring,
    has_chr_ram: bool,
    prg_banks: usize,  // Number of 16KB PRG banks
    chr_banks: usize,  // Number of 8KB CHR banks (or 0 if CHR RAM)
    prg_rom_size: usize, // Total PRG ROM size in bytes
    
    // Shift register state
    shift_reg: u8,  // 5-bit shift register (stored in lower 5 bits)
    reg_init: u8,   // Initial value (0b100000 = 32)
    
    // Control register (from 0x8000 writes)
    chr_mode: u8,    // 0 = 8KB mode, 1 = 4KB mode
    prg_mode: u8,    // 0/1 = 32KB, 2 = fix first, 3 = fix last
    
    // Bank registers
    prg_reg: u8,     // PRG bank register
    chr1_reg: u8,    // CHR bank 0 register
    chr2_reg: u8,    // CHR bank 1 register
    
    // Clamp values (power of 2 - 1)
    prg_clamp: u8,
    chr_clamp: u8,
    
    // Current bank pointers (as offsets)
    prg_bank1_offset: usize,
    prg_bank2_offset: usize,
    chr_bank1_offset: usize,
    chr_bank2_offset: usize,
    
    mirroring_changed_flag: bool,
    
    // For ignoring consecutive writes on same cycle
    last_write_cycle: u64,
}

impl Mmc1Mapper {
    pub fn new(mirroring: Mirroring, has_chr_ram: bool, prg_rom_size: usize, chr_rom_size: usize) -> Self {
        let prg_banks = prg_rom_size / 0x4000; // 16KB banks
        let chr_banks = if has_chr_ram { 0 } else { chr_rom_size / 0x2000 }; // 8KB banks
        
        // Calculate clamps (next power of 2 - 1) - matches C reference
        let prg_clamp = if prg_banks > 0 {
            let next_pow2 = next_power_of_2(prg_banks);
            (if next_pow2 > 0 { next_pow2 - 1 } else { 0 }) as u8
        } else {
            0
        };
        
        // CHR clamp: banks * 2 because CHR is in 4KB chunks for banking
        let chr_clamp = if chr_banks > 0 {
            let next_pow2 = next_power_of_2(chr_banks * 2);
            (if next_pow2 > 0 { next_pow2 - 1 } else { 0 }) as u8
        } else {
            0
        };
        
        log::info!("MMC1 mapper initialized: prg_banks={}, chr_banks={}, prg_clamp={}, chr_clamp={}, has_chr_ram={}",
            prg_banks, chr_banks, prg_clamp, chr_clamp, has_chr_ram);
        
        // Initial state: PRG mode 3 (fix last bank), PRG bank 0
        let mut mapper = Self {
            mirroring,
            has_chr_ram,
            prg_banks,
            chr_banks,
            prg_rom_size,
            shift_reg: 0b100000, // REG_INIT
            reg_init: 0b100000,
            chr_mode: 0,
            prg_mode: 3,
            prg_reg: 0,
            chr1_reg: 0,
            chr2_reg: 0,
            prg_clamp,
            chr_clamp,
            prg_bank1_offset: 0,
            prg_bank2_offset: prg_banks.saturating_sub(1) * 0x4000, // Last bank
            chr_bank1_offset: 0,
            chr_bank2_offset: 0x1000, // Second 4KB if in 8KB mode
            mirroring_changed_flag: false,
            last_write_cycle: u64::MAX, // Different from any valid cycle
        };
        
        // Initialize bank offsets
        mapper.update_prg_banks(prg_rom_size);
        mapper.update_chr_banks(chr_rom_size);
        
        log::info!("MMC1 initial banks: prg_bank1_offset=0x{:X}, prg_bank2_offset=0x{:X}",
            mapper.prg_bank1_offset, mapper.prg_bank2_offset);
        
        mapper
    }
    
    fn update_prg_banks(&mut self, prg_rom_size: usize) {
        // Match C reference implementation exactly
        match self.prg_mode {
            0 | 1 => {
                // 32KB mode: both banks switch together (PRG_reg & ~1)
                let bank_num = (self.prg_reg & !0x01) as usize;
                self.prg_bank1_offset = 0x4000 * bank_num;
                self.prg_bank2_offset = self.prg_bank1_offset + 0x4000;
            }
            2 => {
                // Fix first bank, switch second bank
                // First bank is at offset based on bit 4 (for 256KB banking)
                self.prg_bank1_offset = 0x4000 * ((self.prg_reg & 0x10) as usize);
                self.prg_bank2_offset = 0x4000 * (self.prg_reg as usize);
            }
            3 => {
                // Switch first bank, fix second bank (most common mode)
                self.prg_bank1_offset = 0x4000 * (self.prg_reg as usize);
                
                if self.prg_banks > 16 {
                    // Large ROM (>256KB): use bit 4 to select 256KB region
                    let bank256 = if (self.prg_reg & 0x10) != 0 { 1usize } else { 0 };
                    self.prg_bank2_offset = (bank256 + 1) * 0x40000 - 0x4000;
                } else {
                    // Normal: last bank is fixed
                    self.prg_bank2_offset = (self.prg_banks.saturating_sub(1)) * 0x4000;
                }
            }
            _ => {}
        }
        
        // Ensure offsets are within ROM bounds
        if prg_rom_size > 0 {
            self.prg_bank1_offset %= prg_rom_size;
            self.prg_bank2_offset %= prg_rom_size;
        }
    }
    
    fn update_chr_banks(&mut self, chr_rom_size: usize) {
        // Skip CHR banking if using CHR RAM
        if self.has_chr_ram || self.chr_banks == 0 {
            self.chr_bank1_offset = 0;
            self.chr_bank2_offset = 0x1000;
            return;
        }
        
        if self.chr_mode == 1 {
            // 4KB mode: two independent 4KB banks
            self.chr_bank1_offset = 0x1000 * (self.chr1_reg as usize);
            self.chr_bank2_offset = 0x1000 * (self.chr2_reg as usize);
        } else {
            // 8KB mode: one 8KB bank (CHR1_reg & ~1)
            let bank_num = (self.chr1_reg & !0x01) as usize;
            self.chr_bank1_offset = 0x1000 * bank_num;
            self.chr_bank2_offset = self.chr_bank1_offset + 0x1000;
        }
        
        // Ensure offsets are within ROM bounds
        if chr_rom_size > 0 {
            self.chr_bank1_offset %= chr_rom_size;
            self.chr_bank2_offset %= chr_rom_size;
        }
    }
}

impl Mapper for Mmc1Mapper {
    fn cpu_read(&self, addr: u16, prg_rom: &[u8]) -> u8 {
        if prg_rom.is_empty() {
            return 0xFF;
        }
        
        if addr < 0xC000 {
            // First 16KB bank (0x8000-0xBFFF)
            let offset = self.prg_bank1_offset + (addr as usize & 0x3FFF);
            prg_rom[offset % prg_rom.len()]
        } else {
            // Second 16KB bank (0xC000-0xFFFF)
            let offset = self.prg_bank2_offset + (addr as usize & 0x3FFF);
            prg_rom[offset % prg_rom.len()]
        }
    }
    
    fn cpu_write(&mut self, addr: u16, value: u8, _prg_rom: &[u8], _prg_ram: &mut [u8]) {
        self.mirroring_changed_flag = false;
        
        // MMC1 only responds to writes in the $8000-$FFFF range
        // Writes to $6000-$7FFF are PRG-RAM only, not mapper registers
        if addr < 0x8000 {
            return;
        }
        
        let prg_rom_size = self.prg_rom_size;
        let chr_rom_size = self.chr_banks * 0x2000;
        
        // Check for reset (bit 7 set)
        if (value & 0x80) != 0 {
            self.shift_reg = self.reg_init;
            self.prg_mode = 3;
            self.update_prg_banks(prg_rom_size);
            return;
        }
        
        // Shift register: accumulate bits (5 bits total)
        // Each write shifts right and adds the LSB of value to bit 5
        self.shift_reg = (self.shift_reg >> 1) | ((value & 0x01) << 5);
        
        // Check if register is full (bit 0 is set after 5 shifts)
        if (self.shift_reg & 0x01) == 0 {
            return; // Not full yet
        }
        
        // Register is full - remove the completion bit
        let reg_value = self.shift_reg >> 1;
        
        // Route to appropriate register based on address (matching C reference)
        match addr & 0xE000 {
            0x8000 => {
                // Control register: mirroring, CHR mode, PRG mode
                let mirroring_bits = reg_value & 0x03;
                let new_mirroring = match mirroring_bits {
                    0 => Mirroring::OneScreenLower,
                    1 => Mirroring::OneScreenUpper,
                    2 => Mirroring::Vertical,
                    3 => Mirroring::Horizontal,
                    _ => unreachable!(),
                };
                if new_mirroring != self.mirroring {
                    self.mirroring = new_mirroring;
                    self.mirroring_changed_flag = true;
                }
                
                self.chr_mode = (reg_value >> 4) & 0x01;
                self.prg_mode = (reg_value >> 2) & 0x03;
                
                self.update_prg_banks(prg_rom_size);
                self.update_chr_banks(chr_rom_size);
            }
            0xA000 => {
                // CHR bank 0 (or 256KB PRG bank select if CHR RAM present)
                if self.has_chr_ram {
                    // If CHR RAM, bit 4 controls 256KB PRG bank selection
                    self.prg_reg &= !0x10;
                    self.prg_reg |= reg_value & 0x10;
                    self.prg_reg &= self.prg_clamp;
                    self.update_prg_banks(prg_rom_size);
                } else {
                    // CHR bank 0 register
                    self.chr1_reg = reg_value & 0x1F;
                    self.chr1_reg &= self.chr_clamp;
                    self.update_chr_banks(chr_rom_size);
                }
            }
            0xC000 => {
                // CHR bank 1 (only in 4KB CHR mode)
                if self.chr_mode == 0 {
                    // Reset shift register and return - ignored in 8KB mode
                    self.shift_reg = self.reg_init;
                    return;
                }
                if self.has_chr_ram {
                    // If CHR RAM, bit 4 controls 256KB PRG bank selection
                    self.prg_reg &= !0x10;
                    self.prg_reg |= reg_value & 0x10;
                    self.prg_reg &= self.prg_clamp;
                    self.update_prg_banks(prg_rom_size);
                } else {
                    // CHR bank 1 register
                    self.chr2_reg = reg_value & 0x1F;
                    self.chr2_reg &= self.chr_clamp;
                    self.update_chr_banks(chr_rom_size);
                }
            }
            0xE000 => {
                // PRG bank register (lower 4 bits)
                self.prg_reg &= !0x0F;
                self.prg_reg |= reg_value & 0x0F;
                self.prg_reg &= self.prg_clamp;
                self.update_prg_banks(prg_rom_size);
            }
            _ => {}
        }
        
        // Reset shift register
        self.shift_reg = self.reg_init;
    }
    
    fn ppu_read(&self, addr: u16, chr_rom: &[u8], chr_ram: &[u8]) -> u8 {
        if self.has_chr_ram {
            return chr_ram[addr as usize % chr_ram.len().max(1)];
        }
        
        if chr_rom.is_empty() {
            return 0;
        }
        
        let pattern_addr = addr & 0x1FFF;
        if pattern_addr < 0x1000 {
            let offset = self.chr_bank1_offset + pattern_addr as usize;
            chr_rom[offset % chr_rom.len()]
        } else {
            let offset = self.chr_bank2_offset + (pattern_addr as usize & 0x0FFF);
            chr_rom[offset % chr_rom.len()]
        }
    }
    
    fn ppu_write(&mut self, addr: u16, value: u8, chr_ram: &mut [u8]) {
        if self.has_chr_ram && !chr_ram.is_empty() {
            chr_ram[addr as usize % chr_ram.len()] = value;
        }
        // CHR ROM is read-only
    }
    
    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
    
    fn mirroring_changed(&self) -> bool {
        self.mirroring_changed_flag
    }
}

// MMC3 Mapper (Mapper 4) - used by SMB3, Kirby's Adventure, etc.
pub struct Mmc3Mapper {
    mirroring: Mirroring,
    has_chr_ram: bool,
    prg_rom_size: usize,
    chr_rom_size: usize,
    
    // PRG banking: 4x 8KB banks
    // Bank 0: $8000-$9FFF (switchable or fixed to 2nd-last)
    // Bank 1: $A000-$BFFF (switchable R7)
    // Bank 2: $C000-$DFFF (fixed to 2nd-last or switchable)
    // Bank 3: $E000-$FFFF (fixed to last)
    prg_bank_offsets: [usize; 4],
    
    // CHR banking: 8x 1KB banks
    chr_bank_offsets: [usize; 8],
    
    // Bank select register
    bank_select: u8,      // Which bank register to update next
    prg_mode: bool,       // false: $8000 switchable, true: $C000 switchable  
    chr_inversion: bool,  // Swap CHR bank regions
    
    // Bank data registers R0-R7
    bank_data: [u8; 8],
    
    // IRQ counter
    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
    
    // Clamp values
    prg_clamp: u8,
    chr_clamp: u8,
    
    mirroring_changed_flag: bool,
}

impl Mmc3Mapper {
    pub fn new(mirroring: Mirroring, has_chr_ram: bool, prg_rom_size: usize, chr_rom_size: usize) -> Self {
        let prg_banks_8k = prg_rom_size / 0x2000; // 8KB banks
        let chr_banks_1k = if has_chr_ram { 8 } else { chr_rom_size / 0x400 }; // 1KB banks
        
        // Calculate clamps (next power of 2 - 1)
        let prg_clamp = if prg_banks_8k > 0 {
            (next_power_of_2(prg_banks_8k) - 1) as u8
        } else {
            0
        };
        let chr_clamp = if chr_banks_1k > 0 {
            (next_power_of_2(chr_banks_1k) - 1) as u8
        } else {
            0
        };
        
        log::info!("MMC3 mapper initialized: prg_8k_banks={}, chr_1k_banks={}, prg_clamp={}, chr_clamp={}, has_chr_ram={}",
            prg_banks_8k, chr_banks_1k, prg_clamp, chr_clamp, has_chr_ram);
        
        let mut mapper = Self {
            mirroring,
            has_chr_ram,
            prg_rom_size,
            chr_rom_size: if has_chr_ram { 0x2000 } else { chr_rom_size },
            prg_bank_offsets: [0; 4],
            chr_bank_offsets: [0; 8],
            bank_select: 0,
            prg_mode: false,
            chr_inversion: false,
            bank_data: [0; 8],
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
            prg_clamp,
            chr_clamp,
            mirroring_changed_flag: false,
        };
        
        // Initialize PRG banks: last two 8KB banks fixed
        let last_bank = prg_rom_size.saturating_sub(0x2000);
        let second_last = prg_rom_size.saturating_sub(0x4000);
        mapper.prg_bank_offsets[2] = second_last;
        mapper.prg_bank_offsets[3] = last_bank;
        
        // Initialize CHR banks
        for i in 0..8 {
            mapper.chr_bank_offsets[i] = i * 0x400;
        }
        
        mapper
    }
    
    fn update_prg_banks(&mut self) {
        let second_last = self.prg_rom_size.saturating_sub(0x4000);
        let last = self.prg_rom_size.saturating_sub(0x2000);
        
        let r6 = (self.bank_data[6] & self.prg_clamp) as usize * 0x2000;
        let r7 = (self.bank_data[7] & self.prg_clamp) as usize * 0x2000;
        
        if self.prg_mode {
            // PRG mode 1: $8000 = 2nd-last, $C000 = R6
            self.prg_bank_offsets[0] = second_last;
            self.prg_bank_offsets[2] = r6;
        } else {
            // PRG mode 0: $8000 = R6, $C000 = 2nd-last
            self.prg_bank_offsets[0] = r6;
            self.prg_bank_offsets[2] = second_last;
        }
        self.prg_bank_offsets[1] = r7;
        self.prg_bank_offsets[3] = last;
        
        // Ensure within bounds
        for offset in &mut self.prg_bank_offsets {
            if self.prg_rom_size > 0 {
                *offset %= self.prg_rom_size;
            }
        }
    }
    
    fn update_chr_banks(&mut self) {
        if self.has_chr_ram {
            // CHR RAM: simple linear mapping
            for i in 0..8 {
                self.chr_bank_offsets[i] = (i * 0x400) % 0x2000;
            }
            return;
        }
        
        // R0 and R1 are 2KB banks (bits 0 ignored)
        let r0 = (self.bank_data[0] & 0xFE & self.chr_clamp) as usize * 0x400;
        let r1 = (self.bank_data[1] & 0xFE & self.chr_clamp) as usize * 0x400;
        // R2-R5 are 1KB banks
        let r2 = (self.bank_data[2] & self.chr_clamp) as usize * 0x400;
        let r3 = (self.bank_data[3] & self.chr_clamp) as usize * 0x400;
        let r4 = (self.bank_data[4] & self.chr_clamp) as usize * 0x400;
        let r5 = (self.bank_data[5] & self.chr_clamp) as usize * 0x400;
        
        if self.chr_inversion {
            // CHR A12 inversion: swap 2KB and 1KB regions
            // $0000-$0FFF: R2,R3,R4,R5 (1KB each)
            // $1000-$1FFF: R0,R0+1,R1,R1+1 (2KB each)
            self.chr_bank_offsets[0] = r2;
            self.chr_bank_offsets[1] = r3;
            self.chr_bank_offsets[2] = r4;
            self.chr_bank_offsets[3] = r5;
            self.chr_bank_offsets[4] = r0;
            self.chr_bank_offsets[5] = r0 + 0x400;
            self.chr_bank_offsets[6] = r1;
            self.chr_bank_offsets[7] = r1 + 0x400;
        } else {
            // Normal: 
            // $0000-$0FFF: R0,R0+1,R1,R1+1 (2KB each)
            // $1000-$1FFF: R2,R3,R4,R5 (1KB each)
            self.chr_bank_offsets[0] = r0;
            self.chr_bank_offsets[1] = r0 + 0x400;
            self.chr_bank_offsets[2] = r1;
            self.chr_bank_offsets[3] = r1 + 0x400;
            self.chr_bank_offsets[4] = r2;
            self.chr_bank_offsets[5] = r3;
            self.chr_bank_offsets[6] = r4;
            self.chr_bank_offsets[7] = r5;
        }
        
        // Ensure within bounds
        let chr_size = self.chr_rom_size.max(1);
        for offset in &mut self.chr_bank_offsets {
            *offset %= chr_size;
        }
    }
    
    /// Called by PPU on each scanline when rendering is enabled
    pub fn clock_irq(&mut self) -> bool {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }
        
        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
            return true;
        }
        false
    }
    
    pub fn irq_pending(&self) -> bool {
        self.irq_pending
    }
    
    pub fn acknowledge_irq(&mut self) {
        self.irq_pending = false;
    }
}

impl Mapper for Mmc3Mapper {
    fn cpu_read(&self, addr: u16, prg_rom: &[u8]) -> u8 {
        if prg_rom.is_empty() {
            return 0xFF;
        }
        
        let bank = ((addr - 0x8000) / 0x2000) as usize;
        let offset = self.prg_bank_offsets[bank] + (addr as usize & 0x1FFF);
        prg_rom[offset % prg_rom.len()]
    }
    
    fn cpu_write(&mut self, addr: u16, value: u8, _prg_rom: &[u8], _prg_ram: &mut [u8]) {
        self.mirroring_changed_flag = false;
        
        if addr < 0x8000 {
            return;
        }
        
        match addr & 0xE001 {
            0x8000 => {
                // Bank select
                self.bank_select = value & 0x07;
                self.prg_mode = (value & 0x40) != 0;
                self.chr_inversion = (value & 0x80) != 0;
                self.update_prg_banks();
                self.update_chr_banks();
            }
            0x8001 => {
                // Bank data
                self.bank_data[self.bank_select as usize] = value;
                if self.bank_select < 6 {
                    self.update_chr_banks();
                } else {
                    self.update_prg_banks();
                }
            }
            0xA000 => {
                // Mirroring (ignored for 4-screen)
                if self.mirroring != Mirroring::FourScreen {
                    let new_mirroring = if (value & 0x01) != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                    if new_mirroring != self.mirroring {
                        self.mirroring = new_mirroring;
                        self.mirroring_changed_flag = true;
                    }
                }
            }
            0xA001 => {
                // PRG RAM protect (not implemented)
            }
            0xC000 => {
                // IRQ latch
                self.irq_latch = value;
            }
            0xC001 => {
                // IRQ reload
                self.irq_counter = 0;
                self.irq_reload = true;
            }
            0xE000 => {
                // IRQ disable and acknowledge
                self.irq_enabled = false;
                self.irq_pending = false;
            }
            0xE001 => {
                // IRQ enable
                self.irq_enabled = true;
            }
            _ => {}
        }
    }
    
    fn ppu_read(&self, addr: u16, chr_rom: &[u8], chr_ram: &[u8]) -> u8 {
        if self.has_chr_ram {
            return chr_ram[addr as usize % chr_ram.len().max(1)];
        }
        
        if chr_rom.is_empty() {
            return 0;
        }
        
        // Each 1KB bank
        let bank = (addr / 0x400) as usize;
        let offset = self.chr_bank_offsets[bank] + (addr as usize & 0x3FF);
        chr_rom[offset % chr_rom.len()]
    }
    
    fn ppu_write(&mut self, addr: u16, value: u8, chr_ram: &mut [u8]) {
        if self.has_chr_ram && !chr_ram.is_empty() {
            chr_ram[addr as usize % chr_ram.len()] = value;
        }
    }
    
    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
    
    fn mirroring_changed(&self) -> bool {
        self.mirroring_changed_flag
    }
    
    fn clock_scanline(&mut self) -> bool {
        self.clock_irq()
    }
    
    fn irq_pending(&self) -> bool {
        self.irq_pending
    }
    
    fn acknowledge_irq(&mut self) {
        self.irq_pending = false;
    }
}

impl Cartridge {
    pub fn load(path: &str) -> Result<Self, CartridgeError> {
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        if data.len() < 16 {
            return Err(CartridgeError::InvalidHeader);
        }

        // Check iNES header
        if &data[0..4] != b"NES\x1A" {
            return Err(CartridgeError::InvalidHeader);
        }

        let prg_rom_size = data[4] as usize * 0x4000; // 16KB units
        let chr_rom_size = data[5] as usize * 0x2000; // 8KB units

        let flags6 = data[6];
        let flags7 = data[7];

        let mapper_id = ((flags7 & 0xF0) >> 4) << 4 | (flags6 >> 4);
        // iNES byte 6 bit 0: 0 = Horizontal, 1 = Vertical
        // iNES byte 6 bit 3: 1 = Four-screen (ignores bit 0)
        let mirroring = if (flags6 & 0x08) != 0 {
            Mirroring::FourScreen
        } else if (flags6 & 0x01) == 0 {
            // Bit 0 = 0 -> Horizontal (for horizontal scrolling games like SMB)
            Mirroring::Horizontal
        } else {
            // Bit 0 = 1 -> Vertical (for vertical scrolling games)
            Mirroring::Vertical
        };
        log::info!("iNES flags6: 0x{:02X}, mirroring bit (bit 0): {}, mirroring: {:?}", 
            flags6, flags6 & 0x01, mirroring);
        
        // Note: Horizontal scrolling games use VERTICAL mirroring (counterintuitive!)
        // Vertical mirroring: nametables 0+2 share, 1+3 share - allows horizontal scrolling
        // Horizontal mirroring: nametables 0+1 share, 2+3 share - allows vertical scrolling
        log::info!("Final mirroring: {:?}", mirroring);

        let has_ram = (flags6 & 0x02) != 0;
        let has_chr_ram = chr_rom_size == 0;

        // Skip trainer if present
        let header_size = if (flags6 & 0x04) != 0 { 16 + 512 } else { 16 };

        if data.len() < header_size + prg_rom_size + chr_rom_size {
            return Err(CartridgeError::InvalidHeader);
        }

        let prg_start = header_size;
        let prg_end = prg_start + prg_rom_size;
        let chr_start = prg_end;
        let chr_end = chr_start + chr_rom_size;

        let prg_rom = data[prg_start..prg_end].to_vec();
        let chr_rom = if has_chr_ram {
            vec![0; 0x2000] // Allocate CHR RAM
        } else {
            data[chr_start..chr_end].to_vec()
        };

        let mapper: Box<dyn Mapper> = match mapper_id {
            0 => Box::new(NromMapper::new(mirroring, has_chr_ram)),
            1 => Box::new(Mmc1Mapper::new(mirroring, has_chr_ram, prg_rom_size, chr_rom_size)),
            2 => Box::new(UxromMapper::new(mirroring, has_chr_ram, prg_rom_size)),
            3 => Box::new(CnromMapper::new(mirroring, has_chr_ram, chr_rom_size)),
            4 => Box::new(Mmc3Mapper::new(mirroring, has_chr_ram, prg_rom_size, chr_rom_size)),
            _ => return Err(CartridgeError::UnsupportedMapper(mapper_id)),
        };

        // Log reset vector for debugging
        let cart = Cartridge {
            prg_rom,
            chr_rom,
            mapper,
            mapper_id,
            has_ram,
            mirroring,
        };
        
        // Read reset vector (at 0xFFFC-0xFFFD)
        let reset_low = cart.mapper.cpu_read(0xFFFC, &cart.prg_rom);
        let reset_high = cart.mapper.cpu_read(0xFFFD, &cart.prg_rom);
        let reset_vector = (reset_high as u16) << 8 | reset_low as u16;
        
        // Read first instruction at reset vector
        let first_opcode = cart.mapper.cpu_read(reset_vector, &cart.prg_rom);
        
        log::info!("Cartridge loaded: mapper={}, prg_size={}KB, chr_size={}KB, mirroring={:?}",
            mapper_id, prg_rom_size / 1024, chr_rom_size / 1024, mirroring);
        log::info!("Reset vector: 0x{:04X}, first opcode: 0x{:02X}", reset_vector, first_opcode);
        
        Ok(cart)
    }

    pub fn cpu_read(&mut self, addr: u16, _prg_ram: &mut [u8]) -> u8 {
        self.mapper.cpu_read(addr, &self.prg_rom)
    }

    pub fn cpu_write(&mut self, addr: u16, value: u8, prg_ram: &mut [u8]) -> bool {
        // Write to mapper, return true if mirroring changed
        let old_mirroring = self.mapper.mirroring();
        self.mapper.cpu_write(addr, value, &self.prg_rom, prg_ram);
        self.mapper.mirroring() != old_mirroring || self.mapper.mirroring_changed()
    }

    pub fn ppu_read(&self, addr: u16, chr_ram: &[u8]) -> u8 {
        self.mapper.ppu_read(addr, &self.chr_rom, chr_ram)
    }

    pub fn ppu_write(&mut self, addr: u16, value: u8, chr_ram: &mut [u8]) {
        self.mapper.ppu_write(addr, value, chr_ram);
    }
    
    /// Clock the mapper's scanline counter (for MMC3 IRQ)
    pub fn clock_scanline(&mut self) -> bool {
        self.mapper.clock_scanline()
    }
    
    /// Check if mapper has a pending IRQ
    pub fn irq_pending(&self) -> bool {
        self.mapper.irq_pending()
    }
    
    /// Acknowledge mapper IRQ
    pub fn acknowledge_irq(&mut self) {
        self.mapper.acknowledge_irq();
    }
}
