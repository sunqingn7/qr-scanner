# QR Scanner

A cross-platform QR code scanner built with Rust. Features:

- **Screen Area Selection**: Click "Scan QR" to gray out the screen and drag a rectangle over a QR code
- **Image Paste**: Copy an image (right-click "Copy Image" on web) and paste it for QR extraction
- **Clipboard Integration**: Scan results are automatically copied to clipboard
- **History**: Keeps track of recent scans
- **System Tray** (optional): Run from system tray with double-click to scan

## Requirements

### For Basic Build (without system tray):
- Rust 1.85+ (install via `rustup`)

### For System Tray Support (Linux):
```bash
sudo apt-get install libgtk-3-dev libgdk-pixbuf2.0-dev libatk1.0-dev libappindicator3-dev pkg-config libxdo-dev
```

### For System Tray Support (Windows/Mac):
No additional dependencies needed - builds out of the box.

## Build Instructions

### Basic Build (no system tray):
```bash
cargo build --release
```

### With System Tray:
```bash
cargo build --release --features system-tray
```

## Usage

1. Run the application: `./target/release/qr-scanner`
2. Click "Scan QR" button
3. Drag a rectangle over the QR code on your screen
4. The decoded text will appear in the result box and be copied to clipboard

### Paste Image:
1. Right-click an image on a webpage and select "Copy Image"
2. In QR Scanner, expand "Paste Image" section
3. Click "Paste from Clipboard"

## Platform Support

- ✅ Linux (tested)
- ✅ Windows (should work)
- ✅ macOS (should work)

## Dependencies

- `eframe` - GUI framework (egui)
- `screenshots` - Screen capture
- `rqrr` - QR code decoding
- `arboard` - Clipboard access
- `image` - Image processing
- `tray-icon` - System tray (optional feature)
