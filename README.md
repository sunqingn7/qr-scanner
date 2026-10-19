# QR Scanner

A cross-platform QR code scanner and generator built with Rust (egui/eframe). Features:

- **Screen Area Selection**: Click "Scan QR" and drag a rectangle over a QR code
  (macOS/Linux: built-in overlay; Windows: launches the native Snipping Tool)
- **Image Paste**: Copy an image (right-click "Copy Image" on web), then press
  **Cmd+V** (macOS) / **Ctrl+V** (Windows/Linux) or click "Paste Image"
- **SVG Support**: Paste SVG from the clipboard or open `.svg` files — rasterized
  internally for scanning
- **Clipboard Integration**: Scan results are automatically copied to clipboard (toggleable)
- **History**: Keeps track of recent scans, each re-copyable with one click
- **QR Generation**: Type any text or URL, click "Generate", preview the QR code,
  then save it as **PNG** or **SVG**, or copy the image to the clipboard
- **Non-blocking UI**: screen capture, clipboard reads, downloads, and decoding all
  run on background worker threads with live progress — the window never freezes
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

### Runtime Dependencies (Windows):
- None (uses the built-in Snipping Tool via `ms-screenclip:` and PowerShell for downloads)

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

### Tests:
```bash
cargo test   # 41 unit tests: decoding, paste pipeline, URL/HTML safety,
             # worker-thread protocol, QR generation round-trips
```

## Usage

1. Run the application: `./target/release/qr-scanner`
2. **Scan an area**: click "Scan QR (area selection)", then drag a rectangle over
   the QR code. The decoded text appears in the result box and is copied to the
   clipboard (if Auto-copy is on).
3. **Paste an image**: right-click an image on a webpage → "Copy Image", then
   press Cmd+V / Ctrl+V in the app (or click "Paste Image"). Works with bitmaps,
   SVG, and HTML clipboard data (image URLs are extracted and downloaded).
4. **Open a file**: "Open File..." accepts `png`, `jpg`, `jpeg`, `gif`, `bmp`,
   `webp`, `svg`.
5. **Generate a QR code**: type text or a URL into the Generate field and click
   "Generate" (or press Enter). The QR is previewed above the Debug panel.
   - **Save PNG** / **Save SVG**: export via the native save dialog
   - **Copy Image**: put the QR bitmap on the clipboard
   - **Clear**: dismiss the preview

While a capture/scan job is running, action buttons are disabled and a spinner
shows next to the Debug line — the UI stays responsive and the same job can be
re-triggered as soon as it finishes.

## Platform Notes

- **Windows**: "Scan QR" opens the OS Snipping Tool; the app clears the clipboard
  first and polls (up to 60 s) until the new crop arrives, so stale clipboard
  images can't be scanned by mistake.
- **Linux**: tries each available capture tool in turn (gnome-screenshot,
  spectacle, xfce4-screenshooter, grim+slop, scrot) and falls through on failure;
  paste reads PNG bytes directly from `xclip` (X11) or `wl-paste` (Wayland).
- **macOS**: `screencapture -i` for area selection; Cmd+V paste.

## Security Notes

- Downloaded image URLs are restricted to `http(s)` (no `file://`, UNC paths, or
  `javascript:` junk) with a 20 MiB size cap.
- URLs are passed to PowerShell through environment variables (no command-string
  interpolation); downloads use `curl --max-filesize`.
- All temporary artifacts (screen captures, downloads, TIFF conversions) are
  written to private `tempfile::tempdir()` directories, not predictable `/tmp` paths.

## Dependencies

- `eframe` (0.31) - GUI framework (egui)
- `rqrr` (0.10) - QR code decoding
- `qrcode` (0.14, svg feature) - QR code generation
- `arboard` (3.3) - Clipboard access
- `clipboard-win` (5.3, Windows) - Windows clipboard PNG path
- `image` (0.25) - Image processing
- `usvg` / `resvg` / `tiny-skia` (0.44/0.44/0.11) - SVG rasterization
- `tempfile` (3) - Private temp dirs for screen capture / downloads
- `rfd` (0.15) - Native file dialogs
- `tray-icon` (0.14) - Optional feature flag only (tray not implemented yet)

External tools used at runtime (see *Runtime Dependencies* above): `screencapture`,
`gnome-screenshot`, `spectacle`, `xfce4-screenshooter`, `slop` + `grim`, `scrot`,
`xclip` / `wl-paste`, `curl` (or PowerShell on Windows), `osascript` (macOS pasteboard).
