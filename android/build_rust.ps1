# Build script for Nesium Android native library
# Requires: cargo-ndk, Android NDK
#
# Install prerequisites:
#   cargo install cargo-ndk
#   rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android

$ErrorActionPreference = "Stop"

Write-Host "Building Nesium Android native library..." -ForegroundColor Green

# Navigate to project root
Push-Location "$PSScriptRoot\.."

try {
    # Build for all Android architectures
    cargo ndk `
        -t arm64-v8a `
        -t armeabi-v7a `
        -t x86_64 `
        -t x86 `
        -o android/app/src/main/jniLibs `
        build --release -p nesium-android

    Write-Host "Build successful!" -ForegroundColor Green
    Write-Host "Native libraries are in android/app/src/main/jniLibs/" -ForegroundColor Cyan
    
    # List built libraries
    Get-ChildItem -Recurse "android/app/src/main/jniLibs" -Filter "*.so" | ForEach-Object {
        Write-Host "  $($_.FullName) ($([math]::Round($_.Length / 1KB, 1)) KB)" -ForegroundColor Gray
    }
} catch {
    Write-Host "Build failed: $_" -ForegroundColor Red
    exit 1
} finally {
    Pop-Location
}
