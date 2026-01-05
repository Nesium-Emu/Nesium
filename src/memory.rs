use crate::cartridge::Cartridge;
use crate::cpu::CpuBus;
use crate::ppu::Ppu;
use crate::apu::Apu;
use crate::input::Input;

pub struct MemoryBus {
    pub ram: [u8; 0x800],
    pub ppu: Ppu,
    pub apu: Apu,
    pub input: Input,
    pub cartridge: Cartridge,
    pub prg_ram: [u8; 0x2000],
    pub chr_ram: [u8; 0x2000],
}

impl MemoryBus {
    pub fn new(cartridge: Cartridge) -> Self {
        let mut ppu = Ppu::new();
        ppu.set_mirroring(cartridge.mirroring);
        log::info!("Nametable mirroring: {:?}", cartridge.mirroring);
        Self {
            ram: [0; 0x800],
            ppu,
            apu: Apu::new(),
            input: Input::new(),
            cartridge,
            prg_ram: [0; 0x2000],
            chr_ram: [0; 0x2000],
        }
    }

    fn mirror_ram_addr(&self, addr: u16) -> usize {
        (addr & 0x07FF) as usize
    }
}

impl CpuBus for MemoryBus {
    fn is_oamdma_addr(&self, addr: u16) -> bool {
        // Check if writing to $4014 (OAMDMA register)
        // This is simplified - in real hardware, any write to $4014 triggers DMA
        addr == 0x4014
    }
    
    fn write_oam(&mut self, oam_addr: u16, value: u8) {
        self.ppu.oam[oam_addr as usize & 0xFF] = value;
    }
    
    fn trigger_oamdma(&mut self, page: u8, current_cycle_odd: bool) -> u64 {
        // OAMDMA: Copy 256 bytes from CPU RAM page to OAM
        // The DMA transfer takes 512 cycles (256 bytes, 2 cycles per byte on alternating cycles)
        // Total stall: 512 cycles if write was on odd cycle, 513 if even
        // This is because the write cycle itself is counted, and there's an extra cycle
        // if the write happened on an even cycle
        let page_addr = (page as u16) << 8;
        
        // Perform DMA transfer: copy 256 bytes from CPU RAM to OAM
        for i in 0..256u16 {
            let byte = match page_addr + i {
                0x0000..=0x1FFF => self.ram[self.mirror_ram_addr(page_addr + i)],
                _ => 0, // Can't DMA from other areas (returns 0xFF on real hardware, but 0 is safer)
            };
            self.ppu.oam[i as usize] = byte;
        }
        
        // Return stall cycles: 512 if odd cycle, 513 if even cycle
        // (The write cycle is already counted in the instruction, so we stall for the remaining cycles)
        if current_cycle_odd {
            512 // Total: 513 cycles (1 write + 512 stall)
        } else {
            513 // Total: 514 cycles (1 write + 513 stall)
        }
    }
    
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                // Internal RAM (mirrored)
                self.ram[self.mirror_ram_addr(addr)]
            }
            0x2000..=0x3FFF => {
                // PPU registers (mirrored every 8 bytes)
                // For PPUDATA reads, we need cartridge access for pattern tables
                if (addr & 0x2007) == 0x2007 {
                    // PPUDATA read - provide cartridge read function
                    let cartridge = &self.cartridge;
                    let chr_ram = &self.chr_ram;
                    let mut chr_read_fn = |a: u16| -> u8 {
                        cartridge.ppu_read(a, chr_ram)
                    };
                    let mut chr_read_opt = Some(&mut chr_read_fn as &mut dyn FnMut(u16) -> u8);
                    self.ppu.read_register(addr, &mut chr_read_opt)
                } else {
                    let mut no_chr_read = None;
                    self.ppu.read_register(addr, &mut no_chr_read)
                }
            }
            0x4000..=0x4013 | 0x4015 => {
                // APU registers
                self.apu.read_register(addr)
            }
            0x4014 => {
                // OAMDMA register read (not commonly used, but return value)
                0x40 // Open bus value
            }
            0x4016 => {
                // Controller 1
                self.input.read(0)
            }
            0x4017 => {
                // Controller 2 + APU frame counter
                self.apu.read_register(0x4015) | self.input.read(1)
            }
            0x4018..=0x401F => {
                // APU and I/O test registers
                0
            }
            0x4020..=0x5FFF => {
                // Expansion ROM (unused in most games)
                0
            }
            0x6000..=0x7FFF => {
                // Cartridge RAM
                let addr = addr - 0x6000;
                if addr < self.prg_ram.len() as u16 {
                    self.prg_ram[addr as usize]
                } else {
                    0
                }
            }
            0x8000..=0xFFFF => {
                // Cartridge PRG ROM
                self.cartridge.cpu_read(addr, &mut self.prg_ram)
            }
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => {
                // Internal RAM (mirrored)
                self.ram[self.mirror_ram_addr(addr)] = value;
            }
            0x2000..=0x3FFF => {
                // PPU registers (mirrored every 8 bytes)
                // write_register returns Some((chr_addr, value)) for CHR-RAM writes
                if let Some((chr_addr, chr_value)) = self.ppu.write_register(addr, value) {
                    // Pattern table write (0x0000-0x1FFF) - write to CHR-RAM
                    self.cartridge.ppu_write(chr_addr, chr_value, &mut self.chr_ram);
                }
            }
            0x4000..=0x4013 | 0x4015 => {
                // APU registers
                let bus_ptr = self as *const Self;
                self.apu.write_register(addr, value, move |a: u16| {
                    unsafe { (*bus_ptr).cpu_read_for_apu(a) }
                });
            }
            0x4014 => {
                // OAMDMA: Write page number, triggers DMA from CPU RAM to OAM
                // The actual DMA transfer is handled by trigger_oamdma() in the CPU
                // This write is just a placeholder - the CPU will detect $4014 and call trigger_oamdma
                // We still perform the write here for compatibility, but the stall is handled by CPU
                // Note: In real hardware, the write triggers DMA and the CPU is stalled
            }
            0x4016 => {
                // Controller strobe
                self.input.write(value);
            }
            0x4017 => {
                // APU frame counter
                // Use unsafe to avoid borrow checker issues - this is safe because
                // cpu_read_for_apu only reads from self, doesn't modify it
                let bus_ptr = self as *const Self;
                self.apu.write_register(addr, value, move |a: u16| {
                    unsafe { (*bus_ptr).cpu_read_for_apu(a) }
                });
            }
            0x4018..=0x401F => {
                // APU and I/O test registers
            }
            0x4020..=0x5FFF => {
                // Expansion ROM
            }
            0x6000..=0x7FFF => {
                // Cartridge RAM
                let addr = addr - 0x6000;
                if addr < self.prg_ram.len() as u16 {
                    self.prg_ram[addr as usize] = value;
                    self.cartridge.cpu_write(addr + 0x6000, value, &mut self.prg_ram);
                }
            }
            0x8000..=0xFFFF => {
                // Cartridge PRG ROM (mapper registers)
                let mirroring_changed = self.cartridge.cpu_write(addr, value, &mut self.prg_ram);
                // Check if mirroring changed (for MMC1 and other mappers that support dynamic mirroring)
                if mirroring_changed {
                    self.ppu.set_mirroring(self.cartridge.mapper.mirroring());
                }
            }
        }
    }
}

impl MemoryBus {
    fn cpu_read_for_apu(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[self.mirror_ram_addr(addr)],
            0x8000..=0xFFFF => {
                // For DMC channel
                if addr < self.cartridge.prg_rom.len() as u16 + 0x8000 {
                    let rom_addr = addr - 0x8000;
                    if rom_addr < self.cartridge.prg_rom.len() as u16 {
                        self.cartridge.prg_rom[rom_addr as usize]
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    pub fn step_ppu(&mut self) -> bool {
        let chr_read = |addr: u16| {
            self.cartridge.ppu_read(addr, &self.chr_ram)
        };
        self.ppu.step(chr_read)
    }

    pub fn step_apu(&mut self, cpu_cycles: u64) -> bool {
        let bus_ptr = self as *const Self;
        self.apu.step(cpu_cycles, move |addr: u16| {
            unsafe { (*bus_ptr).cpu_read_for_apu(addr) }
        })
    }
}
