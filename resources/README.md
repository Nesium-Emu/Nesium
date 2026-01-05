# Nesium Resources

This directory contains logo and branding assets for Nesium.

## Logo Files

- `nesium-logo.svg` - Full logo with controller and "NESIUM" text (512x512)
- `nesium-logo-simple.svg` - Simplified icon version with controller and "N" (256x256)

## Color Palette

- Primary Blue: `#64b4ff` (NES controller blue)
- Background Dark: `#1a1a2e` to `#16213e` (gradient)
- A Button: `#ff6464` (red)
- B Button: `#ffc864` (orange/yellow)
- Text: `#ffffff` with blue glow

## Usage

The SVG files can be used for:
- Application icons (convert to PNG/ICO as needed)
- Documentation
- Website/branding
- Social media

## Creating PNG Icon for Application

The application will automatically load `resources/nesium-icon.png` if it exists. To create it:

### Using Inkscape:
```bash
inkscape --export-png=nesium-icon.png --export-width=64 --export-height=64 nesium-logo-simple.svg
```

### Using ImageMagick:
```bash
convert -background none -density 300 -resize 64x64 nesium-logo-simple.svg nesium-icon.png
```

### Recommended Sizes:
- **64x64** - Standard application icon (recommended for `nesium-icon.png`)
- **32x32** - Small icon
- **128x128** - High DPI
- **256x256** - Very high DPI

The application will fall back to a programmatically generated icon if the PNG file is not found.

