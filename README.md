# QR Scanner

A cross-platform QR code scanner built with Rust. Features:

- **Screen Area Selection**: Click "Scan QR" to gray out the screen and drag a rectangle over a QR code
- **Image Paste**: Copy an image (right-click "Copy Image" on web) and paste it for QR extraction
- **Clipboard Integration**: Scan results are automatically copied to clipboard
- **History**: Keeps track of recent scans
- **QR Generation**: Type any text or URL, click "Generate", preview the QR code, then save it as PNG/SVG or copy it to the clipboard
- **System Tray**: *not yet implemented* (the optional `system-tray` feature is a placeholder)

## Requirements

### Build Requirements:
- Rust 1.85+ (install via `rustup`)

### Runtime Dependencies (Linux):

**For screen capture:**
- **GNOME**: `gnome-screenshot` (usually pre-installed)
- **KDE Plasma**: `spectacle` 
  ```bash
  sudo apt-get install kde-spectacle
  ```
- **Xfce**: `xfce4-screenshooter`
  ```bash
  sudo apt-get install xfce4-screenshooter
  ```
- **Sway/Wayland**: `slop` and `grim`
  ```bash
  sudo apt-get install slop grim
  ```

**For clipboard paste:**
- **X11**: `xclip`
  ```bash
  sudo apt-get install xclip
  ```
- **Wayland**: `wl-clipboard`
  ```bash
  sudo apt-get install wl-clipboard
  ```

### Runtime Dependencies (macOS):
- `screencapture` (built-in, no installation needed)

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

### With System Tray (experimental placeholder):
```bash
cargo build --release --features system-tray
```
> Note: tray support is not implemented yet; the feature flag currently pulls in
> `tray-icon` but the app behaves identically without it.

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

- `eframe` (0.31) - GUI framework (egui)
- `rqrr` (0.10) - QR code decoding
- `arboard` (3.3) - Clipboard access
- `image` (0.25) - Image processing
- `tempfile` (3) - Private temp dirs for screen capture / downloads
- `rfd` (0.15) - Native file dialogs
- `tray-icon` (0.14) - Optional feature flag only (tray not implemented yet)

External tools used at runtime (see *Runtime Dependencies* above): `screencapture`,
`gnome-screenshot`, `spectacle`, `xfce4-screenshooter`, `slop` + `grim`, `scrot`,
`xclip` / `wl-paste`, `curl` (or PowerShell on Windows).
