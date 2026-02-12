#!/bin/bash
# Build script for Nesium Android native library
# Requires: cargo-ndk, Android NDK
#
# Install prerequisites:
#   cargo install cargo-ndk
#   rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android

set -e

echo "Building Nesium Android native library..."

# Navigate to project root
cd "$(dirname "$0")/.."

# Build for all Android architectures
cargo ndk \
    -t arm64-v8a \
    -t armeabi-v7a \
    -t x86_64 \
    -t x86 \
    -o android/app/src/main/jniLibs \
    build --release -p nesium-android

echo "Build successful!"
echo "Native libraries are in android/app/src/main/jniLibs/"

# List built libraries
find android/app/src/main/jniLibs -name "*.so" -exec ls -lh {} \;
