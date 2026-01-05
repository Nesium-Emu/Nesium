# Setup script for SDL2 using CPM/CMake on Windows

Write-Host "Setting up SDL2 using CMake and CPM..." -ForegroundColor Green

# Check if CMake is installed
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
    Write-Host "Error: CMake is not installed. Please install from https://cmake.org/" -ForegroundColor Red
    exit 1
}

# Create build directory
New-Item -ItemType Directory -Force -Path build | Out-Null

# Configure and build SDL2
Write-Host "Configuring SDL2 build..." -ForegroundColor Yellow
cmake -B build -S . -DCMAKE_BUILD_TYPE=Release

if ($LASTEXITCODE -eq 0) {
    Write-Host "Building SDL2..." -ForegroundColor Yellow
    cmake --build build --config Release
    
    if ($LASTEXITCODE -eq 0) {
        $sdl2Dir = (Resolve-Path "build\_deps\sdl2-src").Path
        $sdl2LibDir = Join-Path (Resolve-Path "build\_deps\sdl2-build").Path "Release"
        
        # Rename SDL2-static.lib to SDL2.lib for sdl2-sys compatibility
        $staticLib = Join-Path $sdl2LibDir "SDL2-static.lib"
        $renamedLib = Join-Path $sdl2LibDir "SDL2.lib"
        if (Test-Path $staticLib) {
            Copy-Item $staticLib $renamedLib -Force
            Write-Host "Created SDL2.lib symlink for Rust compatibility" -ForegroundColor Yellow
        }
        
        Write-Host ""
        Write-Host "SDL2 built successfully using CPM!" -ForegroundColor Green
        Write-Host ""
        Write-Host "Environment variables are configured in .cargo/config.toml" -ForegroundColor Cyan
        Write-Host "You can now build the Rust project with:" -ForegroundColor Cyan
        Write-Host "  cargo build --release" -ForegroundColor White
        Write-Host ""
        Write-Host "Note: SDL2_DIR is set in .cargo/config.toml" -ForegroundColor Yellow
    }
} else {
    Write-Host "Failed to configure SDL2 build" -ForegroundColor Red
    exit 1
}

