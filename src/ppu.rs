use log::debug;
use crate::cartridge::Mirroring;

#[derive(Debug, Clone)]
pub struct Ppu {
    // Registers
    pub ctrl: u8,    // PPUCTRL (0x2000)
    pub mask: u8,    // PPUMASK (0x2001)
    pub status: u8,  // PPUSTATUS (0x2002)
    pub oam_addr: u8, // OAMADDR (0x2003)
    pub oam_data: u8, // OAMDATA (0x2004)
    pub scroll: u8,  // PPUSCROLL (0x2005)
    pub addr: u8,    // PPUADDR (0x2006)
    pub data: u8,    // PPUDATA (0x2007)

    // Internal state
    pub vram: [u8; 0x800],        // 2KB VRAM
    pub palette: [u8; 0x20],      // 32 bytes palette RAM
    pub mirroring: Mirroring,     // Nametable mirroring mode
    pub oam: [u8; 0x100],         // 256 bytes OAM (Object Attribute Memory)
    pub secondary_oam: [u8; 0x20], // Secondary OAM for current scanline
    pub vram_read_buffer: u8,     // PPUDATA read buffer (one-read delay for < $3F00)
    
    // Debug counters for logging
    pub ppudata_read_count: u32,
    pub ppudata_write_count: u32,
    pub ppuaddr_write_count: u32,

    // Rendering state
    pub scanline: i32,
    pub cycle: u32,
    pub frame: u64,
    pub nmi_occurred: bool,
    pub nmi_output: bool,

    // Temporary registers (t, v, x, w)
    pub vram_addr_temp: u16, // t
    pub vram_addr: u16,      // v
    pub fine_x: u8,          // x
    pub write_toggle: bool,   // w

    // Rendering state
    pub next_tile_id: u8,
    pub next_tile_attr: u8,
    pub next_tile_low: u8,
    pub next_tile_high: u8,
    pub tile_id: u8,
    pub tile_attr: u8,
    pub tile_low: u8,
    pub tile_high: u8,
    pub shift_pattern_low: u16,
    pub shift_pattern_high: u16,
    pub shift_attr_low: u16,
    pub shift_attr_high: u16,

    // Sprites
    pub sprite_count: u8,
    pub sprite_indices: [u8; 8],
    pub sprite_positions: [u8; 8],
    pub sprite_patterns_low: [u8; 8],
    pub sprite_patterns_high: [u8; 8],
    pub sprite_attributes: [u8; 8],
    
    // Framebuffer for pixel output
    pub framebuffer: [u8; 256 * 240],
}

impl Ppu {
    pub fn new() -> Self {
        log::info!("PPU initialized: VRAM set to 0xFF (garbage) for SMB compatibility");
        Self {
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            oam_data: 0,
            scroll: 0,
            addr: 0,
            data: 0,
            vram: [0xFF; 0x800], // Initialize to 0xFF (garbage/random) to match real hardware power-on state
            palette: [0xFF; 0x20], // Initialize to 0xFF (will be overwritten by game, but avoids 0x00 issues)
            mirroring: Mirroring::Horizontal, // Default to horizontal, will be set from cartridge
            oam: [0; 0x100],
            secondary_oam: [0; 0x20],
            vram_read_buffer: 0xFF, // PPUDATA read buffer (initialized to 0xFF to match real hardware garbage state)
            ppudata_read_count: 0,
            ppudata_write_count: 0,
            ppuaddr_write_count: 0,
            scanline: -1,
            cycle: 0,
            frame: 0,
            nmi_occurred: false,
            nmi_output: false,
            vram_addr_temp: 0,
            vram_addr: 0,
            fine_x: 0,
            write_toggle: false,
            next_tile_id: 0,
            next_tile_attr: 0,
            next_tile_low: 0,
            next_tile_high: 0,
            tile_id: 0,
            tile_attr: 0,
            tile_low: 0,
            tile_high: 0,
            shift_pattern_low: 0,
            shift_pattern_high: 0,
            shift_attr_low: 0,
            shift_attr_high: 0,
            sprite_count: 0,
            sprite_indices: [0; 8],
            sprite_positions: [0; 8],
            sprite_patterns_low: [0; 8],
            sprite_patterns_high: [0; 8],
            sprite_attributes: [0; 8],
            framebuffer: [0; 256 * 240],
        }
    }
    
    pub fn set_mirroring(&mut self, mirroring: Mirroring) {
        self.mirroring = mirroring;
    }

    fn increment_vram_addr(&mut self) {
        // PPUCTRL bit 2 (0x04) controls VRAM address increment:
        // - Bit 2 = 0: increment by 1 (horizontal fill) - used by most games
        // - Bit 2 = 1: increment by 32 (vertical fill)
        let increment = if (self.ctrl & 0x04) != 0 { 32 } else { 1 };
        // Internal v register is 15 bits (0x0000-0x7FFF)
        // When reading/writing VRAM, only bits 0-13 are used (0x0000-0x3FFF address space)
        // The internal register wraps at 0x7FFF (15-bit boundary)
        // Important: This is the correct behavior - the internal register can be 0x4000-0x7FFF
        // for fine Y scroll bits, but VRAM access masks it to 0x3FFF
        self.vram_addr = (self.vram_addr.wrapping_add(increment)) & 0x7FFF;
    }

    pub fn read_register(&mut self, addr: u16, mut chr_read: &mut Option<&mut dyn FnMut(u16) -> u8>) -> u8 {
        match addr & 0x2007 {
            0x2002 => {
                // PPUSTATUS
                let value = self.status;
                self.status &= 0x7F; // Clear VBlank flag
                self.write_toggle = false;
                value
            }
            0x2004 => {
                // OAMDATA
                self.oam_data
            }
            0x2007 => {
                // PPUDATA read ($2007)
                // NES hardware quirk: reads from addresses < $3F00 have a one-read delay
                // The read returns the buffer value, then fills buffer with actual fetch
                // Palette reads ($3F00+) are immediate, but buffer still gets filled with mirrored nametable byte
                let addr = self.vram_addr & 0x3FFF;
                let result = if addr >= 0x3F00 {
                    // Palette read: immediate, no delay
                    // Return palette value (with mirroring handled in read_vram)
                    let palette_value = self.read_vram(addr, &mut chr_read);
                    // Fill buffer with mirrored nametable byte (for next read)
                    // Mirror: $3F00-$3FFF -> $2000-$2FFF (nametable range)
                    let mirrored_addr = addr & 0x2FFF; // Mirror palette address down to nametable
                    self.vram_read_buffer = self.read_vram(mirrored_addr, &mut chr_read);
                    palette_value
                } else {
                    // Nametable/pattern table read: return buffer, then fill buffer
                    let buffered_value = self.vram_read_buffer;
                    // Fill buffer with actual read (for next read)
                    // Pattern tables (0x0000-0x1FFF) must be read from cartridge
                    let actual_value = self.read_vram(addr, &mut chr_read);
                    self.vram_read_buffer = actual_value;
                    
                    // Detailed logging for first 500 reads or until frame 30 (use INFO so it shows without --debug)
                    self.ppudata_read_count += 1;
                    if self.ppudata_read_count <= 500 && self.frame < 30 {
                        // Log all reads (pattern tables, nametables, attributes)
                        // Especially important: pattern table reads ($0000-$1FFF) for title logo decompression
                        if addr < 0x2000 || self.ppudata_read_count <= 200 {
                            log::info!("PPUDATA read #{}: frame={}, addr=0x{:04X}, buffer_before=0x{:02X}, actual=0x{:02X}, returned=0x{:02X}, buffer_after=0x{:02X}",
                                self.ppudata_read_count, self.frame, addr, buffered_value, actual_value, buffered_value, self.vram_read_buffer);
                        }
                    }
                    
                    buffered_value
                };
                
                // Increment VRAM address (by 1 or 32 based on PPUCTRL bit 2)
                self.increment_vram_addr();
                
                result
            }
            _ => 0,
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr & 0x2007 {
            0x2000 => {
                // PPUCTRL ($2000)
                // Bit 3 (0x08): Sprite pattern table address (0 = $0000, 1 = $1000)
                // Bit 4 (0x10): Background pattern table address (0 = $0000, 1 = $1000)
                let bg_pt = if (value & 0x10) != 0 { "$1000" } else { "$0000" };
                let sprite_pt = if (value & 0x08) != 0 { "$1000" } else { "$0000" };
                log::info!("PPUCTRL write: 0x{:02X} | bg_pt: {} sprite_pt: {} | NMI: {} | Increment: {} | Nametable: {}",
                    value, bg_pt, sprite_pt,
                    if (value & 0x80) != 0 { "enabled" } else { "disabled" },
                    if (value & 0x04) != 0 { "32" } else { "1" },
                    value & 0x03);
                self.ctrl = value;
                self.vram_addr_temp = (self.vram_addr_temp & 0xF3FF) | ((value as u16 & 0x03) << 10);
            }
            0x2001 => {
                // PPUMASK
                let render_bg = (value & 0x08) != 0;
                let render_sprites = (value & 0x10) != 0;
                debug!("PPU $2001 write: 0x{:02X} (render_bg={}, render_sprites={})", 
                    value, render_bg, render_sprites);
                if render_bg || render_sprites {
                    log::info!("PPU rendering ENABLED: bg={}, sprites={}", render_bg, render_sprites);
                }
                self.mask = value;
            }
            0x2003 => {
                // OAMADDR
                self.oam_addr = value;
            }
            0x2004 => {
                // OAMDATA
                self.oam[self.oam_addr as usize] = value;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            0x2005 => {
                // PPUSCROLL
                if !self.write_toggle {
                    // First write: fine X scroll
                    self.fine_x = value & 0x07;
                    self.vram_addr_temp = (self.vram_addr_temp & 0xFFE0) | ((value >> 3) as u16);
                } else {
                    // Second write: fine Y scroll
                    self.vram_addr_temp = (self.vram_addr_temp & 0x8C1F)
                        | (((value & 0x07) as u16) << 12)
                        | (((value >> 3) as u16) << 5);
                }
                self.write_toggle = !self.write_toggle;
            }
            0x2006 => {
                // PPUADDR
                // ALWAYS log first 20 writes (both high and low) to verify they're happening
                if !self.write_toggle {
                    // First write: high byte
                    log::info!("PPUADDR write (HIGH, count={}): frame={}, value=0x{:02X}, temp_before=0x{:04X}", 
                        self.ppuaddr_write_count, self.frame, value, self.vram_addr_temp);
                    self.vram_addr_temp = (self.vram_addr_temp & 0x00FF) | ((value & 0x3F) as u16) << 8;
                } else {
                    // Second write: low byte
                    self.vram_addr_temp = (self.vram_addr_temp & 0xFF00) | value as u16;
                    self.vram_addr = self.vram_addr_temp;
                    self.ppuaddr_write_count += 1;
                    // Log first 200 PPUADDR writes (complete address)
                    if self.ppuaddr_write_count <= 200 {
                        log::info!("PPUADDR write #{} (LOW): frame={}, value=0x{:02X}, vram_addr=0x{:04X} (final)", 
                            self.ppuaddr_write_count, self.frame, value, self.vram_addr);
                    }
                    // Note: Buffer is NOT reset when PPUADDR is written
                    // The buffer persists and the first read after PPUADDR will return the buffer value
                }
                self.write_toggle = !self.write_toggle;
            }
            0x2007 => {
                // PPUDATA write
                // Read address BEFORE increment (for this write)
                let addr = self.vram_addr & 0x3FFF;
                
                // Calculate increment based on PPUCTRL bit 2 (for logging)
                // Bit 2 = 0: increment by 1 (horizontal fill) - used by most games like SMB, DK
                // Bit 2 = 1: increment by 32 (vertical fill)
                let increment = if (self.ctrl & 0x04) != 0 { 32 } else { 1 };
                
                // ALWAYS log first 20 writes unconditionally to verify they're happening
                self.ppudata_write_count += 1;
                if self.ppudata_write_count <= 20 {
                    log::info!("PPUDATA write #{} (UNCONDITIONAL): frame={}, vram_addr=0x{:04X}, value=0x{:02X}, increment={}, PPUCTRL=0x{:02X} (bit2={})", 
                        self.ppudata_write_count, self.frame, addr, value, increment, self.ctrl, (self.ctrl >> 2) & 1);
                }
                
                // Detailed logging for first 200 writes (use INFO so it shows without --debug)
                if self.ppudata_write_count <= 200 {
                    if addr >= 0x2000 && addr < 0x3F00 {
                        // Calculate actual VRAM index after mirroring
                        let nt_addr = addr & 0x2FFF;
                        let nt_index = match self.mirroring {
                            Mirroring::Horizontal => {
                                let base = nt_addr & 0x03FF;
                                if nt_addr >= 0x2800 { base + 0x0400 } else { base }
                            }
                            Mirroring::Vertical => {
                                let base = nt_addr & 0x03FF;
                                if (nt_addr & 0x0400) != 0 { base + 0x0400 } else { base }
                            }
                            Mirroring::FourScreen => {
                                let base = nt_addr & 0x03FF;
                                if nt_addr >= 0x2800 { base + 0x0400 } else { base }
                            }
                            Mirroring::OneScreenLower | Mirroring::OneScreenUpper => {
                                // Single screen: all nametables map to same 1KB
                                nt_addr & 0x03FF
                            }
                        };
                        // Calculate next address for logging (before increment)
                        let next_addr = (self.vram_addr.wrapping_add(increment)) & 0x7FFF; // 15-bit internal register
                        log::info!("PPUDATA write #{}: frame={}, vram_addr=0x{:04X} -> VRAM[0x{:03X}], value=0x{:02X} (tile_id={}), increment={}, next_addr=0x{:04X}",
                            self.ppudata_write_count, self.frame, addr, nt_index, value, value, increment, next_addr);
                    } else if addr >= 0x3F00 && addr < 0x3F20 {
                        log::info!("PPUDATA write #{}: frame={}, addr=0x{:04X} (palette[0x{:02X}]), value=0x{:02X}, increment={}",
                            self.ppudata_write_count, self.frame, addr, (addr & 0x1F) as u8, value, increment);
                    } else {
                        log::info!("PPUDATA write #{}: frame={}, addr=0x{:04X}, value=0x{:02X}, increment={}",
                            self.ppudata_write_count, self.frame, addr, value, increment);
                    }
                }
                
                // Write to VRAM, then increment address
                // CRITICAL: increment MUST happen after write, and MUST update vram_addr correctly
                self.write_vram(addr, value);
                self.increment_vram_addr();
            }
            _ => {}
        }
    }

    pub fn read_vram(&self, addr: u16, chr_read: &mut Option<&mut dyn FnMut(u16) -> u8>) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                // Pattern tables: read from cartridge if available
                if let Some(chr_read_fn) = chr_read {
                    let data = chr_read_fn(addr);
                    // Log CHR reads during early frames (especially high addresses $1EC0-$1FFF for title data)
                    if self.frame < 30 {
                        if addr >= 0x1EC0 || self.frame < 10 {
                            log::info!("CHR read: frame={}, addr=0x{:04X}, data=0x{:02X}", self.frame, addr, data);
                        }
                    }
                    data
                } else {
                    0 // Fallback if no cartridge read function provided
                }
            }
            0x2000..=0x3EFF => {
                // Nametables (mirrored based on cartridge setting)
                // VRAM layout: $0000-$03FF = NT0, $0400-$07FF = NT1
                let nt_addr = addr & 0x2FFF; // Mask to nametable range ($2000-$2FFF)
                let nt_index = match self.mirroring {
                    Mirroring::Horizontal => {
                        // Horizontal: $2000/$2400 = NT0, $2800/$2C00 = NT1
                        // $2000-$23FF -> NT0 ($0000-$03FF in VRAM)
                        // $2400-$27FF -> NT0 mirror ($0000-$03FF in VRAM)
                        // $2800-$2BFF -> NT1 ($0400-$07FF in VRAM)
                        // $2C00-$2FFF -> NT1 mirror ($0400-$07FF in VRAM)
                        let base = nt_addr & 0x03FF; // Get offset within nametable (0-1023)
                        if nt_addr >= 0x2800 {
                            base + 0x0400 // NT1: add $0400 offset
                        } else {
                            base // NT0: no offset
                        }
                    }
                    Mirroring::Vertical => {
                        // Vertical: $2000/$2800 = NT0, $2400/$2C00 = NT1
                        // $2000-$23FF -> NT0 ($0000-$03FF in VRAM)
                        // $2400-$27FF -> NT1 ($0400-$07FF in VRAM)
                        // $2800-$2BFF -> NT0 mirror ($0000-$03FF in VRAM)
                        // $2C00-$2FFF -> NT1 mirror ($0400-$07FF in VRAM)
                        let base = nt_addr & 0x03FF; // Get offset within nametable (0-1023)
                        if (nt_addr & 0x0400) != 0 {
                            base + 0x0400 // NT1: add $0400 offset
                        } else {
                            base // NT0: no offset
                        }
                    }
                    Mirroring::FourScreen => {
                        // Four-screen: no mirroring, each 1KB nametable is unique
                        // This requires 4KB VRAM, but we only have 2KB, so treat as horizontal
                        let base = nt_addr & 0x03FF;
                        if nt_addr >= 0x2800 {
                            base + 0x0400
                        } else {
                            base
                        }
                    }
                    Mirroring::OneScreenLower | Mirroring::OneScreenUpper => {
                        // Single screen: all nametables map to same 1KB
                        nt_addr & 0x03FF
                    }
                };
                self.vram[nt_index as usize]
            }
            0x3F00..=0x3FFF => {
                // Palette RAM
                let index = addr & 0x001F;
                let index = if (index & 0x03) == 0 { index & !0x10 } else { index };
                self.palette[index as usize]
            }
            _ => 0,
        }
    }

    pub fn write_vram(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {} // Pattern tables handled by cartridge (read-only)
            0x2000..=0x3EFF => {
                // Nametables (mirrored based on cartridge setting)
                // VRAM layout: $0000-$03FF = NT0, $0400-$07FF = NT1
                let nt_addr = addr & 0x2FFF; // Mask to nametable range ($2000-$2FFF)
                let nt_index = match self.mirroring {
                    Mirroring::Horizontal => {
                        // Horizontal: $2000/$2400 = NT0, $2800/$2C00 = NT1
                        let base = nt_addr & 0x03FF; // Get offset within nametable (0-1023)
                        if nt_addr >= 0x2800 {
                            base + 0x0400 // NT1: add $0400 offset
                        } else {
                            base // NT0: no offset
                        }
                    }
                    Mirroring::Vertical => {
                        // Vertical: $2000/$2800 = NT0, $2400/$2C00 = NT1
                        let base = nt_addr & 0x03FF; // Get offset within nametable (0-1023)
                        if (nt_addr & 0x0400) != 0 {
                            base + 0x0400 // NT1: add $0400 offset
                        } else {
                            base // NT0: no offset
                        }
                    }
                    Mirroring::FourScreen => {
                        // Four-screen: no mirroring, each 1KB nametable is unique
                        // This requires 4KB VRAM, but we only have 2KB, so treat as horizontal
                        let base = nt_addr & 0x03FF;
                        if nt_addr >= 0x2800 {
                            base + 0x0400
                        } else {
                            base
                        }
                    }
                    Mirroring::OneScreenLower | Mirroring::OneScreenUpper => {
                        // Single screen: all nametables map to same 1KB
                        nt_addr & 0x03FF
                    }
                };
                self.vram[nt_index as usize] = value;
            }
            0x3F00..=0x3FFF => {
                // Palette RAM
                let index = addr & 0x001F;
                let index = if (index & 0x03) == 0 { index & !0x10 } else { index };
                self.palette[index as usize] = value;
                // Mirror writes to 0x3F10, 0x3F14, 0x3F18, 0x3F1C
                if (index & 0x03) == 0 {
                    self.palette[(index ^ 0x10) as usize] = value;
                }
            }
            _ => {}
        }
    }

    pub fn step(&mut self, mut chr_read: impl FnMut(u16) -> u8) -> bool {
        let nmi_before = self.nmi_output;
        self.nmi_occurred = false;

        // Pre-render scanline (-1)
        if self.scanline == -1 {
            if self.cycle == 1 {
                // Clear flags at start of pre-render
                self.status &= 0x1F; // Clear VBlank, sprite overflow, sprite 0 hit
                self.nmi_output = false;
            }
            
            // Background tile fetching (cycles 1-256, 321-336)
            if self.cycle >= 1 && self.cycle <= 256 {
                // Background tile fetching happens at specific phases
                if (self.mask & 0x08) != 0 {
                    let phase = (self.cycle - 1) % 8;
                    match phase {
                        1 => {
                            // Cycle 2, 10, 18, ...: Fetch nametable byte
                            if self.cycle < 256 {
                                self.fetch_tile_data(&mut chr_read);
                            }
                        }
                        3 => {
                            // Cycle 4, 12, 20, ...: Fetch attribute byte
                            if self.cycle < 256 {
                                self.fetch_tile_data(&mut chr_read);
                            }
                        }
                        5 => {
                            // Cycle 6, 14, 22, ...: Fetch low pattern byte
                            if self.cycle < 256 {
                                self.fetch_tile_data(&mut chr_read);
                            }
                        }
                        7 => {
                            // Cycle 8, 16, 24, ...: Fetch high pattern byte and reload
                            if self.cycle < 256 {
                                self.fetch_tile_data(&mut chr_read);
                            }
                        }
                        _ => {}
                    }
                }
                
                // Shift registers AFTER fetching (for pre-render, just shift, don't render)
                if (self.mask & 0x08) != 0 {
                    self.shift_registers();
                }
                
                // Increment Y scroll at cycle 256 - only when rendering is enabled
                if self.cycle == 256 && (self.mask & 0x18) != 0 {
                    self.increment_y();
                }
            } else if self.cycle == 257 {
                // Only copy X scroll when rendering is enabled
                if (self.mask & 0x18) != 0 {
                    self.copy_x();
                }
            } else if self.cycle >= 280 && self.cycle <= 304 {
                // Only copy Y scroll when rendering is enabled
                if (self.mask & 0x18) != 0 {
                    self.copy_y();
                }
            } else if self.cycle >= 321 && self.cycle <= 336 {
                // Background tile fetching for next scanline
                if (self.mask & 0x08) != 0 {
                    let phase = (self.cycle - 1) % 8;
                    match phase {
                        1 => self.fetch_tile_data(&mut chr_read), // Nametable
                        3 => self.fetch_tile_data(&mut chr_read), // Attribute
                        5 => self.fetch_tile_data(&mut chr_read), // Low pattern
                        7 => self.fetch_tile_data(&mut chr_read), // High pattern + reload
                        _ => {}
                    }
                    // Shift after fetching
                    self.shift_registers();
                }
            }
        }
        // Visible scanlines (0-239)
        else if self.scanline >= 0 && self.scanline < 240 {
            // Clear secondary OAM at cycle 1
            if self.cycle == 1 {
                self.secondary_oam = [0xFF; 0x20]; // Clear secondary OAM
                self.sprite_count = 0;
            }
            
            // Sprite evaluation (cycles 65-256)
            if self.cycle >= 65 && self.cycle <= 256 {
                self.evaluate_sprites_cycle();
            }
            
            // Render visible pixels (cycles 1-256)
            if self.cycle >= 1 && self.cycle <= 256 {
                // Render pixel (matching C reference: reads tile data on-the-fly)
                let x = (self.cycle - 1) as u32;
                let y = self.scanline as u32;
                if x < 256 && y < 240 {
                    let idx = (y * 256 + x) as usize;
                    if idx < self.framebuffer.len() {
                        // Render pixel if rendering enabled, otherwise use background color
                        if (self.mask & 0x18) != 0 {
                            let pixel = self.render_pixel(&mut chr_read);
                            self.framebuffer[idx] = pixel;
                        } else {
                            // Rendering disabled: show background color (palette entry 0)
                            self.framebuffer[idx] = self.palette[0] & 0x3F;
                        }
                    }
                }
                
                // Increment vram address when fine_x wraps (matching C reference)
                // This happens after rendering the pixel
                if (self.mask & 0x08) != 0 && x < 256 {
                    let fine_x = ((self.fine_x as u32 + x) % 8) as u8;
                    if fine_x == 7 {
                        // Increment coarse X (matching C code: if(fine_x == 7))
                        if (self.vram_addr & 0x001F) == 0x001F {
                            // Wrap to next nametable horizontally
                            self.vram_addr &= !0x001F;
                            self.vram_addr ^= 0x0400;
                        } else {
                            self.vram_addr += 1;
                        }
                    }
                }
            }
            // Increment Y scroll (cycle 257 = VISIBLE_DOTS + 1) - matching C reference
            else if self.cycle == 257 {
                if (self.mask & 0x08) != 0 {
                    // Increment Y scroll (matching C code: if(ppu->dots == VISIBLE_DOTS + 1 && ppu->mask & SHOW_BG))
                    if (self.vram_addr & 0x7000) != 0x7000 {
                        // Increment fine Y
                        self.vram_addr += 0x1000;
                    } else {
                        // Wrap fine Y and increment coarse Y
                        self.vram_addr &= !0x7000;
                        let mut coarse_y = (self.vram_addr & 0x03E0) >> 5;
                        if coarse_y == 29 {
                            coarse_y = 0;
                            // Switch vertical nametable
                            self.vram_addr ^= 0x0800;
                        } else if coarse_y == 31 {
                            coarse_y = 0;
                        } else {
                            coarse_y += 1;
                        }
                        self.vram_addr = (self.vram_addr & !0x03E0) | (coarse_y << 5);
                    }
                }
            }
            // Copy X scroll (cycle 258 = VISIBLE_DOTS + 2) - matching C reference
            else if self.cycle == 258 {
                if (self.mask & 0x18) != 0 {
                    // Copy X scroll (matching C code: else if(ppu->dots == VISIBLE_DOTS + 2 && (ppu->mask & RENDER_ENABLED)))
                    self.copy_x();
                }
            }
            // Sprite tile fetching (cycles 257-320)
            else if self.cycle >= 257 && self.cycle <= 320 {
                // Sprite fetching: 8 cycles per sprite, max 8 sprites
                let sprite_cycle = self.cycle - 257;
                let sprite_idx = (sprite_cycle / 8) as usize;
                let sprite_phase = sprite_cycle % 8;
                
                if sprite_idx < self.sprite_count as usize {
                    if sprite_phase == 0 || sprite_phase == 4 {
                        self.fetch_sprite_data(&mut chr_read, sprite_idx, sprite_phase);
                    }
                }
            }
            // Background tile fetching for next scanline (cycles 321-336)
            else if self.cycle >= 321 && self.cycle <= 336 {
                if (self.mask & 0x08) != 0 {
                    let phase = (self.cycle - 1) % 8;
                    match phase {
                        1 => self.fetch_tile_data(&mut chr_read), // Nametable
                        3 => self.fetch_tile_data(&mut chr_read), // Attribute
                        5 => self.fetch_tile_data(&mut chr_read), // Low pattern
                        7 => self.fetch_tile_data(&mut chr_read), // High pattern + reload
                        _ => {}
                    }
                    // Shift after fetching
                    self.shift_registers();
                }
            }
        }
        // VBlank scanlines (241-260)
        else if self.scanline == 241 && self.cycle == 1 {
            // Enter VBlank
            self.status |= 0x80;
            if (self.ctrl & 0x80) != 0 {
                self.nmi_occurred = true;
            }
        }

        self.cycle += 1;
        
        // Odd/even frame timing: on odd frames, if rendering is enabled, skip cycle 340
        let rendering_enabled = (self.mask & 0x18) != 0;
        let is_odd_frame = (self.frame & 1) != 0;
        let skip_cycle = self.scanline == -1 && self.cycle == 340 && rendering_enabled && is_odd_frame;
        
        if skip_cycle {
            // Skip cycle 340 on odd frames during pre-render scanline
            self.cycle = 0;
            self.scanline += 1;
        } else if self.cycle > 340 {
            self.cycle = 0;
            self.scanline += 1;
        }
        
        if self.scanline > 260 {
            self.scanline = -1;
            self.frame += 1;
        }

        self.nmi_output = self.nmi_occurred && (self.ctrl & 0x80) != 0;
        self.nmi_output && !nmi_before
    }

    fn render_pixel(&mut self, chr_read: &mut impl FnMut(u16) -> u8) -> u8 {
        // Render background pixel - match C reference implementation
        // The C code reads tile data on-the-fly during rendering using read_vram
        let bg_pixel = if (self.mask & 0x08) != 0 {
            let cycle_x = (self.cycle - 1) as u32;
            let fine_x = ((self.fine_x as u32 + cycle_x) % 8) as u8;
            
            // Check if left 8 pixels are masked (PPUMASK bit 1)
            // C code: !(ppu->mask & SHOW_BG_8) && x < 8
            // SHOW_BG_8 is bit 1 (0x02), so we mask if bit 1 is 0 AND x < 8
            let left_masked = cycle_x < 8 && (self.mask & 0x02) == 0;
            
            if !left_masked {
                // Match C reference: render_background function
                // Calculate tile address: 0x2000 | (v & 0xFFF)
                let tile_addr = 0x2000 | (self.vram_addr & 0x0FFF);
                
                // Read tile ID from nametable (nametables are in VRAM, no CHR read needed)
                let mut no_chr = None;
                let tile_id = self.read_vram(tile_addr, &mut no_chr);
                
                // Debug logging for first few pixels
                if self.scanline >= 0 && self.scanline < 3 && cycle_x < 10 {
                    debug!("render_pixel: sl={}, cy={}, vram_addr=0x{:04X}, tile_addr=0x{:04X}, tile_id=0x{:02X}, mask=0x{:02X}", 
                        self.scanline, cycle_x, self.vram_addr, tile_addr, tile_id, self.mask);
                }
                
                // Calculate attribute address: 0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07)
                let attr_addr = 0x23C0 | (self.vram_addr & 0x0C00) 
                    | ((self.vram_addr >> 4) & 0x38) 
                    | ((self.vram_addr >> 2) & 0x07);
                
                // Calculate pattern address: (tile_id * 16 + fine_y) | bg_table_base
                let bg_pt_base = if (self.ctrl & 0x10) != 0 { 0x1000 } else { 0x0000 };
                let fine_y = (self.vram_addr >> 12) & 0x07;
                let pattern_addr = (tile_id as u16) * 16 + fine_y | bg_pt_base;
                
                // Read pattern bytes directly from CHR (pattern tables are 0x0000-0x1FFF)
                // Pattern tables are read from cartridge, not VRAM
                let pattern_low = chr_read(pattern_addr);
                let pattern_high = chr_read(pattern_addr + 8);
                
                // Extract pixel bits (matching C code exactly: >> (7 ^ fine_x))
                let bit_pos = 7 ^ fine_x;
                let palette_addr = ((pattern_low >> bit_pos) & 1) | (((pattern_high >> bit_pos) & 1) << 1);
                
                if palette_addr != 0 {
                    // Read attribute byte from VRAM (use read_vram, but pattern tables don't need chr_read)
                    let mut no_chr = None;
                    let attr = self.read_vram(attr_addr, &mut no_chr);
                    // Attribute shift: ((v >> 4) & 4 | v & 2) - matches C code
                    let shift = ((self.vram_addr >> 4) & 0x04) | (self.vram_addr & 0x02);
                    let attr_bits = ((attr >> shift) & 0x03) as u8;
                    Some((attr_bits << 2) | palette_addr)
                } else {
                    None
                }
            } else {
                // Left 8 pixels masked
                None
            }
        } else {
            None
        };

        // Render sprite pixel
        let sprite_pixel = if (self.mask & 0x10) != 0 {
            for i in 0..self.sprite_count as usize {
                let sprite_x = self.sprite_positions[i] as u32;
                let cycle_x = (self.cycle - 1) as u32;
                
                if cycle_x >= sprite_x && cycle_x < sprite_x + 8 {
                    let shift = 7 - (cycle_x - sprite_x);
                    let pattern_bit = ((self.sprite_patterns_high[i] >> shift) & 0x01) << 1
                        | ((self.sprite_patterns_low[i] >> shift) & 0x01);
                    
                    if pattern_bit != 0 {
                        let attr = self.sprite_attributes[i];
                        let priority = (attr & 0x20) == 0; // 0 = in front of bg
                        let palette_idx = ((attr & 0x03) << 2) | pattern_bit;
                        
                        // Sprite 0 hit detection
                        // Conditions:
                        // 1. Must be sprite 0 (i == 0)
                        // 2. Background pixel must be non-transparent
                        // 3. Sprite pixel must be non-transparent (already checked)
                        // 4. Must be during visible scanlines (0-239) - already true
                        // 5. Must be during cycles 1-256 (not 257-340)
                        // 6. Left 8 pixels can be masked by PPUMASK bit 2 (show left 8 pixels of bg/sprites)
                        if i == 0 && bg_pixel.is_some() && cycle_x < 256 {
                            // Check if left 8 pixels are masked (PPUMASK bits 1-2)
                            // Bit 1: show left 8 pixels of background
                            // Bit 2: show left 8 pixels of sprites
                            let left_masked = cycle_x < 8 && ((self.mask & 0x06) != 0x06);
                            if !left_masked && (self.mask & 0x18) == 0x18 {
                                // Both background and sprites enabled
                                self.status |= 0x40; // Set sprite 0 hit flag
                            }
                        }
                        
                        if bg_pixel.is_none() || priority {
                            // Look up sprite palette (0x10-0x1F -> palette RAM 0x10-0x1F)
                            let palette_addr = (0x10 | palette_idx) as usize;
                            return self.palette[palette_addr] & 0x3F;
                        }
                    }
                }
            }
            None
        } else {
            None
        };

        // Return final pixel color index (0-63) from palette RAM
        if let Some(bg) = bg_pixel {
            // Background palette: look up in palette RAM (indices 0x00-0x0F)
            let palette_addr = bg as usize;
            // Debug: log first few background pixels to see if they're being rendered
            if self.scanline >= 0 && self.scanline < 3 && (self.cycle - 1) < 50 {
                debug!("BG pixel: sl={}, cy={}, bg_idx={}, palette[{}]=0x{:02X}", 
                    self.scanline, self.cycle - 1, bg, palette_addr, self.palette[palette_addr]);
            }
            self.palette[palette_addr] & 0x3F
        } else if let Some(spr) = sprite_pixel {
            // Sprite pixel already looked up in palette RAM
            spr
        } else {
            // Universal background color (palette entry 0)
            // Debug: log when we fall back to backdrop
            if self.scanline >= 0 && self.scanline < 3 && (self.cycle - 1) < 50 {
                debug!("Backdrop: sl={}, cy={}, palette[0]=0x{:02X}, shift_low=0x{:04X}, shift_high=0x{:04X}", 
                    self.scanline, self.cycle - 1, self.palette[0], self.shift_pattern_low, self.shift_pattern_high);
            }
            self.palette[0] & 0x3F
        }
    }
    
    pub fn build_framebuffer(&mut self, framebuffer: &mut [u8], _chr_read: impl Fn(u16) -> u8) {
        // Re-render the visible frame
        for y in 0..240 {
            for x in 0..256 {
                let idx = (y * 256 + x) as usize;
                if idx >= framebuffer.len() {
                    continue;
                }
                
                // Simplified rendering - would need to simulate PPU state for this pixel
                // For now, use a simple approach: render from current PPU state
                if (self.mask & 0x08) == 0 && (self.mask & 0x10) == 0 {
                    framebuffer[idx] = self.palette[0] & 0x3F;
                    continue;
                }
                
                // This is a simplified version - full accuracy would require
                // tracking pixel state during rendering
                framebuffer[idx] = self.palette[0] & 0x3F;
            }
        }
    }

    fn shift_registers(&mut self) {
        // Shift pattern registers (shift in new bits from tile data every 8 cycles)
        // During visible rendering, shift happens every cycle
        self.shift_pattern_low <<= 1;
        self.shift_pattern_high <<= 1;
        // Attribute shifters repeat their values
        self.shift_attr_low = (self.shift_attr_low << 1) | (self.shift_attr_low & 0x01);
        self.shift_attr_high = (self.shift_attr_high << 1) | (self.shift_attr_high & 0x01);
    }

    fn fetch_tile_data(&mut self, chr_read: &mut impl FnMut(u16) -> u8) {
        // Fetch phases: 1, 3, 5, 7 (cycles 2, 4, 6, 8 of each 8-cycle group)
        // Use cycle % 8 to determine fetch phase (cycle 1 = phase 0, cycle 2 = phase 1, etc.)
        let phase = (self.cycle - 1) % 8;
        
        match phase {
            1 => {
                // Fetch nametable byte
                // vram_addr bits 10-11 select nametable: 00=$2000, 01=$2400, 10=$2800, 11=$2C00
                // Nametable base = 0x2000 | ((vram_addr & 0x0C00) >> 8) * 0x400
                // Simplified: 0x2000 | (vram_addr & 0x0C00) = correct base
                let nametable_base = 0x2000 | (self.vram_addr & 0x0C00);
                let addr = nametable_base | (self.vram_addr & 0x03FF);
                let mut no_chr_read = None;
                self.next_tile_id = self.read_vram(addr, &mut no_chr_read);
                
                // Debug: log nametable reads to see what tiles are being fetched
                if self.scanline >= 0 && self.scanline < 3 && self.cycle < 50 {
                    debug!("Nametable read: sl={}, cy={}, vram_addr=0x{:04X}, nametable_base=0x{:04X}, nametable_addr=0x{:04X}, tile_id=0x{:02X}",
                        self.scanline, self.cycle, self.vram_addr, nametable_base, addr, self.next_tile_id);
                }
                // Note: vram_addr is NOT incremented during tile fetching - only during rendering
            }
            3 => {
                // Fetch attribute byte
                // Attribute table is at nametable base + 0x03C0
                // Use same nametable base as nametable fetch (from vram_addr bits 10-11)
                let nametable_base = 0x2000 | (self.vram_addr & 0x0C00);
                let addr = nametable_base | 0x03C0
                    | ((self.vram_addr >> 4) & 0x38)
                    | ((self.vram_addr >> 2) & 0x07);
                let mut no_chr_read = None;
                let attr = self.read_vram(addr, &mut no_chr_read);
                // Attribute quadrant selection: matches C reference
                // C code: ((ppu->v >> 4) & 4 | ppu->v & 2)
                // This extracts: bit 1 from coarse_x (v bit 1) and bit 2 from coarse_y (v bit 6)
                // Result: bit 0 = coarse_x bit 0, bit 1 = coarse_y bit 0
                let shift = ((self.vram_addr >> 4) & 0x04) | (self.vram_addr & 0x02);
                self.next_tile_attr = ((attr >> shift) & 0x03) as u8;
                // Note: vram_addr is NOT incremented during tile fetching - only during rendering
            }
            5 => {
                // Fetch background pattern table low byte (plane 0)
                // PPUCTRL bit 4 (0x10): 0 = background from $0000, 1 = background from $1000
                // Address calculation: base + (tile_id * 16) + fine_y
                // Each tile is 16 bytes: 8 bytes for low plane (0-7), 8 bytes for high plane (8-15)
                let bg_pt_base = if (self.ctrl & 0x10) != 0 { 0x1000 } else { 0x0000 };
                let fine_y = (self.vram_addr >> 12) & 0x07; // Fine Y scroll (0-7)
                let low_plane_addr = bg_pt_base | ((self.next_tile_id as u16) << 4) | fine_y;
                self.next_tile_low = chr_read(low_plane_addr);
                
                // Debug: log first few fetches with bitplane info
                if self.scanline >= 0 && self.scanline < 3 && self.cycle < 50 {
                    log::info!("BG Pattern LOW: sl={}, cy={}, tile=0x{:02X}, fine_y={}, addr=0x{:04X}, value=0x{:02X} (bits: {:08b})",
                        self.scanline, self.cycle, self.next_tile_id, fine_y, low_plane_addr, self.next_tile_low, self.next_tile_low);
                }
            }
            7 => {
                // Fetch background pattern table high byte (plane 1)
                // High plane is 8 bytes after low plane: base + (tile_id * 16) + fine_y + 8
                let bg_pt_base = if (self.ctrl & 0x10) != 0 { 0x1000 } else { 0x0000 };
                let fine_y = (self.vram_addr >> 12) & 0x07; // Fine Y scroll (0-7)
                let high_plane_addr = bg_pt_base | ((self.next_tile_id as u16) << 4) | fine_y | 8;
                self.next_tile_high = chr_read(high_plane_addr);
                
                // Debug: log first few fetches with bitplane composition
                if self.scanline >= 0 && self.scanline < 3 && self.cycle < 50 {
                    // Show what pixels would be generated from these bytes
                    let pixel_samples = (0..8).map(|i| {
                        let low_bit = (self.next_tile_low >> (7-i)) & 0x01;
                        let high_bit = (self.next_tile_high >> (7-i)) & 0x01;
                        ((high_bit << 1) | low_bit) as u8
                    }).collect::<Vec<_>>();
                    log::info!("BG Pattern HIGH: sl={}, cy={}, tile=0x{:02X}, fine_y={}, addr=0x{:04X}, value=0x{:02X} (bits: {:08b}) | pixels: {:?}",
                        self.scanline, self.cycle, self.next_tile_id, fine_y, high_plane_addr, self.next_tile_high, self.next_tile_high, pixel_samples);
                }
                
                // Store fetched data and reload shift registers
                // This happens at the END of the 8-cycle fetch group (phase 7)
                self.tile_id = self.next_tile_id;
                self.tile_attr = self.next_tile_attr;
                self.tile_low = self.next_tile_low;
                self.tile_high = self.next_tile_high;
                
                // Reload shift registers: load into high byte (bits 8-15)
                // The shift registers are 16-bit: bits 8-15 hold the current tile, bits 0-7 hold the next tile
                // We shift LEFT each cycle, so we load into high byte and read from high byte
                // After 8 shifts, the tile moves to low byte, and we load next tile into high byte
                self.shift_pattern_low = (self.shift_pattern_low & 0x00FF) | ((self.tile_low as u16) << 8);
                self.shift_pattern_high = (self.shift_pattern_high & 0x00FF) | ((self.tile_high as u16) << 8);
                
                // Load attribute bits (repeat across 8 pixels)
                // Attribute is 2 bits, expanded to 8 bits (one per pixel)
                let attr_low = if (self.tile_attr & 0x01) != 0 { 0xFF } else { 0x00 };
                let attr_high = if (self.tile_attr & 0x02) != 0 { 0xFF } else { 0x00 };
                self.shift_attr_low = (self.shift_attr_low & 0x00FF) | (attr_low << 8);
                self.shift_attr_high = (self.shift_attr_high & 0x00FF) | (attr_high << 8);
                
                // Debug: log shift register loading
                if self.scanline >= 0 && self.scanline < 3 && self.cycle < 30 {
                    log::info!("Shift load: sl={}, cy={}, tile=0x{:02X}, tile_low=0x{:02X}, tile_high=0x{:02X}, shift_low=0x{:04X}, shift_high=0x{:04X}", 
                        self.scanline, self.cycle, self.tile_id, self.tile_low, self.tile_high,
                        self.shift_pattern_low, self.shift_pattern_high);
                }
            }
            _ => {}
        }
    }

    fn fetch_sprite_data(&mut self, chr_read: &mut impl FnMut(u16) -> u8, sprite_idx: usize, phase: u32) {
        if sprite_idx >= self.sprite_count as usize {
            return;
        }
        
        let sprite_y = (self.scanline as i16) - (self.secondary_oam[sprite_idx * 4] as i16);
        let sprite_tile = self.secondary_oam[sprite_idx * 4 + 1];
        let sprite_attr = self.secondary_oam[sprite_idx * 4 + 2];
        let flip_vertical = (sprite_attr & 0x80) != 0;
        let sprite_row = if flip_vertical {
            7 - (sprite_y % 8)
        } else {
            sprite_y % 8
        };
        
            match phase {
            0 => {
                // Fetch sprite pattern low (cycle 0 of sprite fetch)
                // PPUCTRL bit 3 (0x08): 0 = sprites from $0000, 1 = sprites from $1000
                let addr = if (self.ctrl & 0x20) != 0 {
                    // 8x16 sprites: pattern table determined by bit 0 of tile ID
                    ((sprite_tile as u16 & 0xFE) << 4) | ((sprite_tile as u16 & 0x01) << 12) | sprite_row as u16
                } else {
                    // 8x8 sprites: pattern table from PPUCTRL bit 3
                    let sprite_pt_base = if (self.ctrl & 0x08) != 0 { 0x1000 } else { 0x0000 };
                    sprite_pt_base | (sprite_tile as u16) << 4 | sprite_row as u16
                };
                let mut pattern = chr_read(addr);
                if (sprite_attr & 0x40) != 0 {
                    pattern = pattern.reverse_bits();
                }
                self.sprite_patterns_low[sprite_idx] = pattern;
            }
            4 => {
                // Fetch sprite pattern high (cycle 4 of sprite fetch)
                // PPUCTRL bit 3 (0x08): 0 = sprites from $0000, 1 = sprites from $1000
                let addr = if (self.ctrl & 0x20) != 0 {
                    // 8x16 sprites: pattern table determined by bit 0 of tile ID
                    ((sprite_tile as u16 & 0xFE) << 4) | ((sprite_tile as u16 & 0x01) << 12) | sprite_row as u16 | 8
                } else {
                    // 8x8 sprites: pattern table from PPUCTRL bit 3
                    let sprite_pt_base = if (self.ctrl & 0x08) != 0 { 0x1000 } else { 0x0000 };
                    sprite_pt_base | (sprite_tile as u16) << 4 | sprite_row as u16 | 8
                };
                let mut pattern = chr_read(addr);
                if (sprite_attr & 0x40) != 0 {
                    pattern = pattern.reverse_bits();
                }
                self.sprite_patterns_high[sprite_idx] = pattern;
                self.sprite_positions[sprite_idx] = self.secondary_oam[sprite_idx * 4 + 3];
                self.sprite_attributes[sprite_idx] = sprite_attr;
            }
            _ => {}
        }
    }

    fn evaluate_sprites(&mut self) {
        // Legacy method - kept for compatibility
        self.evaluate_sprites_cycle();
    }
    
    fn evaluate_sprites_cycle(&mut self) {
        // Sprite evaluation happens during cycles 65-256
        // This is called every cycle during that range
        // We need to simulate the cycle-by-cycle evaluation
        
        // Calculate which OAM entry we're evaluating
        // Evaluation starts at cycle 65, takes 2 cycles per sprite (64 sprites × 2 = 128 cycles)
        // But we simplify: evaluate all sprites that match the scanline
        
        // Only evaluate once per scanline (at cycle 65)
        if self.cycle == 65 {
            self.sprite_count = 0;
            let scanline = self.scanline as u16;
            let sprite_height = if (self.ctrl & 0x20) != 0 { 16 } else { 8 };
            
            for i in 0..64 {
                let y = self.oam[i * 4] as u16;
                // Check if sprite is on current scanline
                // Note: y=0xFF means sprite is off-screen (y >= 239 for 8x8, y >= 231 for 8x16)
                if y < 240 && scanline >= y && scanline < y + sprite_height {
                    if self.sprite_count < 8 {
                        let idx = (self.sprite_count * 4) as usize;
                        self.secondary_oam[idx] = self.oam[i * 4];
                        self.secondary_oam[idx + 1] = self.oam[i * 4 + 1];
                        self.secondary_oam[idx + 2] = self.oam[i * 4 + 2];
                        self.secondary_oam[idx + 3] = self.oam[i * 4 + 3];
                        self.sprite_indices[self.sprite_count as usize] = i as u8;
                        self.sprite_count += 1;
                    } else {
                        // Sprite overflow - set flag but continue checking
                        self.status |= 0x20;
                        // Don't break - need to check all sprites for overflow detection
                    }
                }
            }
        }
    }

    fn increment_x(&mut self) {
        if (self.vram_addr & 0x001F) == 0x001F {
            self.vram_addr &= !0x001F;
            self.vram_addr ^= 0x0400;
        } else {
            self.vram_addr += 1;
        }
    }

    fn increment_y(&mut self) {
        if (self.vram_addr & 0x7000) != 0x7000 {
            self.vram_addr += 0x1000;
        } else {
            self.vram_addr &= !0x7000;
            let mut y = (self.vram_addr & 0x03E0) >> 5;
            if y == 29 {
                y = 0;
                self.vram_addr ^= 0x0800;
            } else if y == 31 {
                y = 0;
            } else {
                y += 1;
            }
            self.vram_addr = (self.vram_addr & !0x03E0) | (y << 5);
        }
    }

    fn copy_x(&mut self) {
        self.vram_addr = (self.vram_addr & 0xFBE0) | (self.vram_addr_temp & 0x041F);
    }

    fn copy_y(&mut self) {
        self.vram_addr = (self.vram_addr & 0x841F) | (self.vram_addr_temp & 0x7BE0);
    }

    pub fn render_pixel_to_buffer(&mut self, framebuffer: &mut [u8], x: u32, y: u32) {
        if y >= 240 || x >= 256 {
            return;
        }
        
        let idx = (y * 256 + x) as usize;
        if idx >= framebuffer.len() {
            return;
        }

        if (self.mask & 0x08) == 0 && (self.mask & 0x10) == 0 {
            // Rendering disabled
            framebuffer[idx] = self.palette[0] & 0x3F;
            return;
        }

        let cycle = x as u32;
        let bg_pixel = if (self.mask & 0x08) != 0 && cycle < 256 && self.scanline >= 0 && self.scanline < 240 {
            let shift = 15 - self.fine_x as u32 - (cycle % 8);
            let pattern = ((self.shift_pattern_high >> shift) & 0x01) << 1
                | ((self.shift_pattern_low >> shift) & 0x01);
            let attr = ((self.shift_attr_high >> shift) & 0x01) << 1
                | ((self.shift_attr_low >> shift) & 0x01);
            if pattern != 0 {
                Some((attr << 2) | pattern)
            } else {
                None
            }
        } else {
            None
        };

        let _sprite_pixel: Option<u8> = if (self.mask & 0x10) != 0 && cycle < 256 && self.scanline >= 0 && self.scanline < 240 {
            for i in 0..self.sprite_count as usize {
                let sprite_x = self.sprite_positions[i] as u32;
                if cycle >= sprite_x && cycle < sprite_x + 8 {
                    let shift = 7 - (cycle - sprite_x);
                    let pattern = ((self.sprite_patterns_high[i] >> shift) & 0x01) << 1
                        | ((self.sprite_patterns_low[i] >> shift) & 0x01);
                    if pattern != 0 {
                        let attr = self.sprite_attributes[i];
                        let priority = (attr & 0x20) == 0;
                        let palette = (attr & 0x03) << 2 | pattern;
                        if i == 0 && bg_pixel.is_some() && cycle != 255 {
                            // Sprite 0 hit
                            self.status |= 0x40;
                        }
                        if bg_pixel.is_none() || priority {
                            // Look up sprite palette
                            let palette_addr = (0x10 | palette) as usize;
                            framebuffer[idx] = self.palette[palette_addr] & 0x3F;
                            return;
                        }
                    }
                }
            }
            None
        } else {
            None
        };

        if let Some(bg) = bg_pixel {
            // Look up background palette
            let palette_addr = bg as usize;
            framebuffer[idx] = self.palette[palette_addr] & 0x3F;
        } else {
            framebuffer[idx] = self.palette[0] & 0x3F; // Background color
        }
    }

}
