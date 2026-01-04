mod cartridge;
mod cpu;
mod ppu;
mod apu;
mod memory;
mod input;
mod emulator;
mod renderer;
mod trace;

use clap::Parser;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::fs::{File, create_dir_all};
use std::path::PathBuf;
use std::io::{self, Write};

#[derive(Parser)]
#[command(name = "nesium")]
#[command(about = "A NES emulator written in Rust")]
struct Args {
    /// Path to the NES ROM file
    rom_path: String,
    
    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
    
    /// Enable CPU instruction tracing (nestest format)
    #[arg(long)]
    trace: bool,
}

/// A writer that writes to both stdout and a file
struct DualWriter {
    file: File,
}

impl DualWriter {
    fn new(file: File) -> Self {
        Self { file }
    }
}

impl Write for DualWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Write to stdout
        io::stdout().write_all(buf)?;
        // Write to file
        self.file.write_all(buf)?;
        Ok(buf.len())
    }
    
    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()?;
        self.file.flush()
    }
}

fn main() {
    let args = Args::parse();

    // Set log level based on debug flag
    let log_level = if args.debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };
    
    // Set RUST_LOG if not already set
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", 
            if args.debug { "debug" } else { "info" });
    }
    
    // Create logs directory if it doesn't exist
    let logs_dir = PathBuf::from("logs");
    if let Err(e) = create_dir_all(&logs_dir) {
        eprintln!("Warning: Could not create logs directory: {}", e);
    }
    
    // Generate log filename with timestamp
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let rom_name = std::path::Path::new(&args.rom_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .replace(" ", "_")
        .replace("\\", "_")
        .replace("/", "_");
    let log_filename = format!("{}_{}.log", rom_name, timestamp);
    let log_path = logs_dir.join(&log_filename);
    
    // Create a dual writer that writes to both stdout and file
    let log_file = match File::create(&log_path) {
        Ok(file) => {
            println!("Log file: {}", log_path.display());
            Some(file)
        }
        Err(e) => {
            eprintln!("Warning: Could not create log file: {}", e);
            None
        }
    };
    
    // Initialize logger with dual output
    let mut builder = env_logger::Builder::from_default_env();
    builder.filter_level(log_level);
    
    if let Some(file) = log_file {
        // Create a writer that writes to both stdout and file
        let dual_writer = DualWriter::new(file);
        builder.target(env_logger::Target::Pipe(Box::new(dual_writer)));
    }
    
    builder.init();

    println!("Loading ROM: {}", args.rom_path);
    let cartridge = match cartridge::Cartridge::load(&args.rom_path) {
        Ok(cart) => cart,
        Err(e) => {
            eprintln!("Error loading ROM: {}", e);
            return;
        }
    };

    println!("Mapper: {}", cartridge.mapper_id);
    println!("PRG ROM: {} KB", cartridge.prg_rom.len() / 1024);
    println!("CHR ROM: {} KB", cartridge.chr_rom.len() / 1024);

    let mut emulator = match emulator::Emulator::new(cartridge, args.trace) {
        Ok(emu) => emu,
        Err(e) => {
            eprintln!("Error initializing emulator: {}", e);
            return;
        }
    };

    println!("Starting emulation...");
    println!("Controls:");
    println!("  Arrow Keys - D-pad");
    println!("  A - B button");
    println!("  S - A button");
    println!("  Enter - Start");
    println!("  Right Shift - Select");

    // Get SDL context from renderer (already initialized)
    // Get event pump from renderer's SDL context
    let sdl_context = emulator.get_renderer().get_sdl_context();
    let mut event_pump = sdl_context.event_pump().unwrap();

    let mut frame_time = Instant::now();
    let target_frame_time = Duration::from_secs_f64(1.0 / 60.0988); // NTSC frame rate

    'running: loop {
        // Handle events
        for event in event_pump.poll_iter() {
            use sdl2::event::Event;
            match event {
                Event::Quit { .. } => break 'running,
                Event::KeyDown { keycode: Some(keycode), .. } => {
                    let scancode = get_scancode(keycode);
                    emulator.handle_input(scancode, true);
                }
                Event::KeyUp { keycode: Some(keycode), .. } => {
                    let scancode = get_scancode(keycode);
                    emulator.handle_input(scancode, false);
                }
                _ => {}
            }
        }

        // Emulate one frame
        emulator.step_frame();
        
        // Log FPS periodically (every 30 seconds at 60 FPS)
        if emulator.frame_count() % 1800 == 0 && emulator.frame_count() > 0 {
            println!("FPS: {:.2} | Frames: {}", emulator.get_fps(), emulator.frame_count());
        }

        // Frame rate limiting
        let elapsed = frame_time.elapsed();
        if elapsed < target_frame_time {
            std::thread::sleep(target_frame_time - elapsed);
        }
        frame_time = Instant::now();
    }

    println!("Emulation stopped.");
}

fn get_scancode(keycode: sdl2::keyboard::Keycode) -> u32 {
    use sdl2::keyboard::Keycode;
    match keycode {
        Keycode::A => 1073742048,
        Keycode::S => 1073742050,
        Keycode::Return => 1073742052,
        Keycode::RShift => 1073742053,
        Keycode::Up => 1073741904,
        Keycode::Down => 1073741905,
        Keycode::Left => 1073741903,
        Keycode::Right => 1073741906,
        _ => 0,
    }
}
