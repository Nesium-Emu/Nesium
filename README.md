# NESium - NES Emulator in Rust

A cycle-accurate NES emulator written in Rust.

## Building

### Prerequisites

1. **Rust**: Install from [rustup.rs](https://rustup.rs/)
2. **SDL2**: The emulator requires SDL2 development libraries

### Windows Setup

You have several options for SDL2 on Windows:

#### Option 1: Using vcpkg (Recommended)
```powershell
# Install vcpkg (if not already installed)
git clone https://github.com/Microsoft/vcpkg.git
cd vcpkg
.\bootstrap-vcpkg.bat

# Install SDL2
.\vcpkg install sdl2:x64-windows

# Set environment variable
$env:VCPKG_ROOT = "C:\path\to\vcpkg"
```

Then set the `SDL2_DIR` environment variable:
```powershell
$env:SDL2_DIR = "$env:VCPKG_ROOT\installed\x64-windows"
```

#### Option 2: Manual SDL2 Installation
1. Download SDL2 development libraries from [libsdl.org](https://github.com/libsdl-org/SDL/releases)
2. Extract to a location like `C:\SDL2`
3. Set environment variables:
```powershell
$env:SDL2_DIR = "C:\SDL2"
$env:LIB = "$env:LIB;C:\SDL2\lib\x64"
$env:INCLUDE = "$env:INCLUDE;C:\SDL2\include"
```

#### Option 3: Using CPM (CMake Package Manager) - RECOMMENDED
This project includes CPM setup for automatic SDL2 dependency management:

```powershell
# Run the setup script (handles everything automatically)
.\setup-sdl2.ps1

# Then build Rust
cargo build --release
```

The setup script will:
1. Download CPM automatically
2. Use CPM to download and build SDL2
3. Configure `.cargo/config.toml` with proper paths
4. Rename libraries for Rust compatibility

No manual environment variable setup needed!

### Build

```bash
cargo build --release
```

### Run

```bash
cargo run --release -- path/to/game.nes
```

## Controls

- Arrow Keys: D-pad
- A: B button
- S: A button  
- Enter: Start
- Right Shift: Select

## Features

- Cycle-accurate 6502 CPU emulation
- Pixel-perfect PPU rendering with scanline-based rendering
- Full APU emulation (5 channels)
- NROM (Mapper 0) support
- Accurate NTSC timing (60.0988 FPS)

## Testing

Test ROMs:
- `nestest.nes` - CPU accuracy test
- Super Mario Bros. - Popular test game
- Tetris - Another popular test game

