mod qr_scanner;
#[cfg(target_os = "linux")]
mod overlay;

use arboard::{Clipboard, ImageData};
use eframe::egui;
use image::ImageBuffer;

struct QrScannerApp {
    result_text: String,
    history: Vec<String>,
    auto_copy: bool,
    debug_info: String,
}

impl QrScannerApp {
    fn new() -> Self {
        Self {
            result_text: String::new(),
            history: Vec::new(),
            auto_copy: true,
            debug_info: "Ready".to_string(),
        }
    }

    fn copy_to_clipboard(text: &str) {
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(text.to_string());
        }
    }

    fn capture_screen() -> Option<image::DynamicImage> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let temp_path = "/tmp/qr_scan_capture.png";

        // macOS: screencapture (interactive area selection)
        // Must use .status() not .output() - screencapture -i is interactive
        // and .output() captures stdout/stderr which deadlocks on macOS
        #[cfg(target_os = "macos")]
        {
            let _ = std::fs::remove_file(temp_path);
            if std::process::Command::new("screencapture")
                .arg("-i")
                .arg(temp_path)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
                && std::path::Path::new(temp_path).exists()
            {
                let img = image::open(temp_path).ok();
                let _ = std::fs::remove_file(temp_path);
                return img;
            }

            // macOS fallback: full screen capture
            let _ = std::fs::remove_file(temp_path);
            if std::process::Command::new("screencapture")
                .arg(temp_path)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
                && std::path::Path::new(temp_path).exists()
            {
                let img = image::open(temp_path).ok();
                let _ = std::fs::remove_file(temp_path);
                return img;
            }
        }

        // Linux: GNOME (area selection)
        #[cfg(target_os = "linux")]
        if std::process::Command::new("gnome-screenshot")
            .arg("-a")
            .arg("-f")
            .arg(temp_path)
            .output()
            .map(|o| o.status.success() && std::path::Path::new(temp_path).exists())
            .unwrap_or(false)
        {
            let img = image::open(temp_path).ok();
            let _ = std::fs::remove_file(temp_path);
            return img;
        }

        // Linux: KDE Plasma (area selection)
        #[cfg(target_os = "linux")]
        if std::process::Command::new("spectacle")
            .arg("-b")
            .arg("-o")
            .arg(temp_path)
            .output()
            .map(|o| o.status.success() && std::path::Path::new(temp_path).exists())
            .unwrap_or(false)
        {
            let img = image::open(temp_path).ok();
            let _ = std::fs::remove_file(temp_path);
            return img;
        }

        // Linux: Xfce
        #[cfg(target_os = "linux")]
        if std::process::Command::new("xfce4-screenshooter")
            .arg("-r")
            .arg("-s")
            .arg(temp_path)
            .output()
            .map(|o| o.status.success() && std::path::Path::new(temp_path).exists())
            .unwrap_or(false)
        {
            let img = image::open(temp_path).ok();
            let _ = std::fs::remove_file(temp_path);
            return img;
        }

        // Linux: Sway/Wayland with grim + slop
        #[cfg(target_os = "linux")]
        if let Ok(slop_out) = std::process::Command::new("slop").output() {
            if slop_out.status.success() {
                let geom = String::from_utf8_lossy(&slop_out.stdout);
                let parts: Vec<&str> = geom.split_whitespace().collect();
                if parts.len() >= 4 {
                    let x = parts[0];
                    let y = parts[1];
                    let w = parts[2];
                    let h = parts[3];

                    if std::process::Command::new("grim")
                        .arg("-g")
                        .arg(format!("{} {}x{}", format!("{},{}", x, y), w, h))
                        .arg(temp_path)
                        .output()
                        .map(|o| o.status.success() && std::path::Path::new(temp_path).exists())
                        .unwrap_or(false)
                    {
                        let img = image::open(temp_path).ok();
                        let _ = std::fs::remove_file(temp_path);
                        return img;
                    }
                }
            }
        }

        // Linux fallback: full screen gnome-screenshot
        #[cfg(target_os = "linux")]
        if std::process::Command::new("gnome-screenshot")
            .arg("-f")
            .arg(temp_path)
            .output()
            .map(|o| o.status.success() && std::path::Path::new(temp_path).exists())
            .unwrap_or(false)
        {
            let img = image::open(temp_path).ok();
            let _ = std::fs::remove_file(temp_path);
            return img;
        }

        // Linux fallback: scrot
        #[cfg(target_os = "linux")]
        if std::process::Command::new("scrot")
            .arg(temp_path)
            .output()
            .map(|o| o.status.success() && std::path::Path::new(temp_path).exists())
            .unwrap_or(false)
        {
            let img = image::open(temp_path).ok();
            let _ = std::fs::remove_file(temp_path);
            return img;
        }

        // Windows: use screenshots crate
        #[cfg(target_os = "windows")]
        {
            if let Ok(screens) = screenshots::Screen::all() {
                if let Some(screen) = screens.into_iter().next() {
                    if let Ok(captured) = screen.capture() {
                        // screenshots returns ImageBuffer<Rgba<u8>, Vec<u8>>
                        // Convert to project's image type
                        let width = captured.width();
                        let height = captured.height();
                        let raw_data: Vec<u8> = captured.into_raw();

                        if let Some(img_buf) =
                            image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, raw_data)
                        {
                            return Some(image::DynamicImage::ImageRgba8(img_buf));
                        }
                    }
                }
            }
        }

        None
    }

    fn start_scan(&mut self) {
        self.debug_info = "Taking screenshot...".to_string();

        match Self::capture_screen() {
            Some(img) => {
                match qr_scanner::scan_image(&img) {
                    Ok(result) => {
                        self.result_text = result.text.clone();
                        self.history.push(result.text);
                        if self.auto_copy {
                            Self::copy_to_clipboard(&self.result_text);
                        }
                        self.debug_info = "QR scanned from screenshot!".to_string();
                    }
                    Err(e) => {
                        self.debug_info = format!(
                            "No QR found in screenshot: {}. Try 'Open File'.",
                            e
                        );
                    }
                }
            }
            None => {
                self.debug_info =
                    "Screenshot not supported. Use 'Paste Image' or 'Open File'."
                        .to_string();
            }
        }
    }

    fn paste_from_clipboard(&mut self) {
        self.debug_info = "Checking clipboard...".to_string();

        #[cfg(target_os = "linux")]
        {
            // Try xclip for X11
            if let Ok(output) = std::process::Command::new("sh")
                .arg("-c")
                .arg("xclip -selection clipboard -t image/png -o 2>/dev/null | base64")
                .output()
            {
                if !output.stdout.is_empty() {
                    if let Ok(png_data) = base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        &output.stdout,
                    ) {
                        if let Ok(img) = image::load_from_memory(&png_data) {
                            self.debug_info = "Loaded image via xclip!".to_string();
                            match qr_scanner::scan_image(&img) {
                                Ok(result) => {
                                    self.result_text = result.text.clone();
                                    self.history.push(result.text);
                                    if self.auto_copy {
                                        Self::copy_to_clipboard(&self.result_text);
                                    }
                                    self.debug_info =
                                        "QR scanned from clipboard!".to_string();
                                    return;
                                }
                                Err(e) => {
                                    self.result_text = format!("No QR code: {}", e);
                                    self.debug_info = format!("No QR found: {}", e);
                                    return;
                                }
                            }
                        }
                    }
                }
            }

            // Try wl-paste for Wayland
            if let Ok(output) = std::process::Command::new("sh")
                .arg("-c")
                .arg("wl-paste --type image/png 2>/dev/null | base64")
                .output()
            {
                if !output.stdout.is_empty() {
                    if let Ok(png_data) = base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        &output.stdout,
                    ) {
                        if let Ok(img) = image::load_from_memory(&png_data) {
                            self.debug_info = "Loaded image via wl-paste!".to_string();
                            match qr_scanner::scan_image(&img) {
                                Ok(result) => {
                                    self.result_text = result.text.clone();
                                    self.history.push(result.text);
                                    if self.auto_copy {
                                        Self::copy_to_clipboard(&self.result_text);
                                    }
                                    self.debug_info =
                                        "QR scanned from clipboard!".to_string();
                                    return;
                                }
                                Err(e) => {
                                    self.result_text = format!("No QR code: {}", e);
                                    self.debug_info = format!("No QR found: {}", e);
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Cross-platform: arboard clipboard
        if let Ok(mut clipboard) = Clipboard::new() {
            match clipboard.get_image() {
                Ok(img_data) => {
                    eprintln!("arboard get_image succeeded: {}x{} ({} bytes)", 
                              img_data.width, img_data.height, img_data.bytes.len());
                    self.scan_image_data(img_data);
                    return;
                }
                Err(e) => {
                    eprintln!("arboard get_image error: {:?}", e);
                }
            }
        }

        // Windows: try PNG format from clipboard
        #[cfg(target_os = "windows")]
        {
            if let Some(img) = Self::read_png_from_clipboard_windows() {
                match qr_scanner::scan_image(&img) {
                    Ok(result) => {
                        self.result_text = result.text.clone();
                        self.history.push(result.text);
                        if self.auto_copy {
                            Self::copy_to_clipboard(&self.result_text);
                        }
                        self.debug_info = "QR scanned from clipboard!".to_string();
                        return;
                    }
                    Err(e) => {
                        self.result_text = format!("No QR code: {}", e);
                        self.debug_info = format!("No QR found: {}", e);
                        return;
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            self.debug_info =
                "No image in clipboard. Copy an image (Cmd+C in browser) then paste."
                    .to_string();
        }
        #[cfg(target_os = "linux")]
        {
            if std::env::var("WAYLAND_DISPLAY").is_ok() {
                self.debug_info = "Wayland: Right-click → 'Copy Image', then Ctrl+V \
                     or use 'Open File'."
                    .to_string();
            } else if std::env::var("DISPLAY").is_ok() {
                self.debug_info =
                    "X11: Make sure xclip is installed: sudo apt install xclip"
                        .to_string();
            } else {
                self.debug_info = "No image in clipboard. Use 'Open File'.".to_string();
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            self.debug_info = "No image in clipboard. Use 'Open File'.".to_string();
        }
    }

    fn scan_image_data(&mut self, img_data: ImageData) {
        let width = img_data.width;
        let height = img_data.height;
        
        // Debug: print first few pixels
        let sample = img_data.bytes.iter().take(16).collect::<Vec<_>>();
        eprintln!("Clipboard raw: {}x{}, first bytes: {:02X?}", width, height, sample);
        
        // Convert BGRA to RGBA for correct colors
        let mut bytes = Vec::with_capacity(img_data.bytes.len());
        let mut i = 0;
        while i + 3 < img_data.bytes.len() {
            let b = img_data.bytes[i];
            let g = img_data.bytes[i + 1];
            let r = img_data.bytes[i + 2];
            let a = img_data.bytes[i + 3];
            bytes.push(r);
            bytes.push(g);
            bytes.push(b);
            bytes.push(a);
            i += 4;
        }

        self.debug_info = format!("Got image: {}x{}", width, height);

        if let Some(img) = ImageBuffer::<image::Rgba<u8>, _>::from_raw(
            width as u32,
            height as u32,
            bytes,
        ) {
            // Debug: save image to temp file to verify
            let debug_path = std::env::temp_dir().join("qr_debug_clipboard.png");
            if img.save(&debug_path).is_ok() {
                eprintln!("Saved debug image to: {:?}", debug_path);
            }

            let dyn_img = image::DynamicImage::ImageRgba8(img);
            match qr_scanner::scan_image(&dyn_img) {
                Ok(result) => {
                    self.result_text = result.text.clone();
                    self.history.push(result.text);
                    if self.auto_copy {
                        Self::copy_to_clipboard(&self.result_text);
                    }
                    self.debug_info = "QR code scanned from clipboard!".to_string();
                }
                Err(e) => {
                    self.result_text = format!("No QR code in image: {}", e);
                    self.debug_info = format!("No QR found: {}", e);
                }
            }
        } else {
            self.result_text = "Failed to process image data".to_string();
            self.debug_info = "Image format not supported".to_string();
        }
    }

    #[cfg(target_os = "windows")]
    fn read_png_from_clipboard_windows() -> Option<image::DynamicImage> {
        eprintln!("Trying clipboard-win to read image...");
        use clipboard_win::raw::{get_vec, open, close, EnumFormats};

        let mut formats = Vec::new();
        if open().is_ok() {
            EnumFormats::new().for_each(|fmt| {
                formats.push(fmt);
            });
            let _ = close();
            eprintln!("Found {} clipboard formats", formats.len());

            for fmt in formats {
                if open().is_ok() {
                    let mut data = Vec::new();
                    let result = get_vec(fmt, &mut data);
                    let _ = close();
                    if result.is_ok() && !data.is_empty() {
                        eprintln!("Format {}: got {} bytes", fmt, data.len());

                        // Check for PNG
                        if data.len() > 8 && data[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
                            if let Ok(img) = image::load_from_memory(&data) {
                                return Some(img);
                            }
                        }
                        // Check for JPEG
                        if data.len() > 2 && data[0..2] == [0xFF, 0xD8] {
                            if let Ok(img) = image::load_from_memory(&data) {
                                return Some(img);
                            }
                        }
                        // Check for BMP
                        if data.len() > 2 && data[0..2] == [0x42, 0x4D] {
                            if let Ok(img) = image::load_from_memory(&data) {
                                return Some(img);
                            }
                        }
                        // Check for SVG text
                        if let Ok(text) = String::from_utf8(data.clone()) {
                            let t = text.trim();
                            let first_200 = text.chars().take(200).collect::<String>();
                            eprintln!("  Text content (first 200): {:?}", first_200);
                            
                            // Check if it's HTML with img src
                            if t.contains("<html") || t.contains("<img") {
                                eprintln!("  Detected HTML, trying to extract image URL...");
                                if let Some(img_url) = Self::extract_img_url_from_html(&text) {
                                    eprintln!("  Found image URL: {}", img_url);
                                    // Download the image
                                    if let Some(img) = Self::download_image_from_url(&img_url) {
                                        eprintln!("  Downloaded and loaded image!");
                                        return Some(img);
                                    }
                                }
                            }
                            
                            // Also check for plain SVG (only if HTML didn't work)
                            let is_svg = t.contains("<svg") || 
                                          t.contains("</svg>") ||
                                          t.contains("xmlns=") ||
                                          t.contains("viewBox=");
                            
                            eprintln!("  Is SVG detected: {}", is_svg);
                            
                            if is_svg {
                                eprintln!("  Detected SVG text ({} bytes), rasterizing...", data.len());
                                if let Some(img) = Self::rasterize_svg(&text) {
                                    return Some(img);
                                } else {
                                    eprintln!("  SVG rasterization failed!");
                                }
                            }
                        }
                        // Try generic image load
                        if let Ok(img) = image::load_from_memory(&data) {
                            return Some(img);
                        }
                    }
                } else {
                    let _ = close();
                }
            }
        } else {
            eprintln!("Failed to open clipboard");
        }

        eprintln!("No supported image format found on clipboard");
        None
    }

fn rasterize_svg(svg_text: &str) -> Option<image::DynamicImage> {
        eprintln!("Rasterizing SVG...");
        let opt = usvg::Options::default();
        let tree = match usvg::Tree::from_str(svg_text, &opt) {
            Ok(tree) => tree,
            Err(e) => {
                eprintln!("Failed to parse SVG: {:?}", e);
                return None;
            }
        };

        let size = tree.size();
        let width = size.width().ceil() as u32;
        let height = size.height().ceil() as u32;
        eprintln!("SVG size: {}x{}", width, height);

        if width == 0 || height == 0 {
            return None;
        }

        let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
        
        // Fill with white background first
        let white = tiny_skia::Color::from_rgba(1.0, 1.0, 1.0, 1.0).unwrap();
        pixmap.fill(white);
        
        // Render SVG on top
        resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());

        let pixels = pixmap.data().to_vec();
        if let Some(buf) = image::ImageBuffer::from_raw(width, height, pixels) {
            // Save debug image
            let debug_path = std::env::temp_dir().join("qr_rasterized.png");
            let _ = buf.save(&debug_path);
            eprintln!("Saved rasterized SVG to: {:?}", debug_path);
            
            return Some(image::DynamicImage::ImageRgba8(buf));
        }
        
        None
    }

    #[cfg(target_os = "windows")]
    fn extract_img_url_from_html(html: &str) -> Option<String> {
        // Simple extraction - find src="..."
        if let Some(start) = html.find("src=\"") {
            let start = start + 5;
            if let Some(end) = html[start..].find('"') {
                return Some(html[start..start + end].to_string());
            }
        }
        if let Some(start) = html.find("src='") {
            let start = start + 5;
            if let Some(end) = html[start..].find('\'') {
                return Some(html[start..start + end].to_string());
            }
        }
        None
    }

#[cfg(target_os = "windows")]
    fn download_image_from_url(url: &str) -> Option<image::DynamicImage> {
        eprintln!("Downloading image from URL: {}", url);
        
        let temp_path = std::env::temp_dir().join("qr_downloaded.png");
        
        // Try PowerShell Invoke-WebRequest
        let ps_script = format!(
            "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing",
            url,
            temp_path.display()
        );
        
        let output = std::process::Command::new("powershell")
            .args(&["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .output()
            .ok()?;
        
        if output.status.success() && temp_path.exists() {
            eprintln!("Downloaded to {:?}", temp_path);
            
            // Try to open the image
            match image::open(&temp_path) {
                Ok(img) => {
                    let _ = std::fs::remove_file(&temp_path);
                    eprintln!("Successfully loaded downloaded image!");
                    return Some(img);
                }
                Err(e) => {
                    eprintln!("Failed to open downloaded image: {:?}", e);
                    let _ = std::fs::remove_file(&temp_path);
                    return None;
                }
            }
        } else {
            eprintln!("Download failed");
            None
        }
    }

fn scan_file(&mut self, path: &str) {
        self.debug_info = format!("Loading: {}", path);

        let path_lower = path.to_lowercase();
        
        // Handle SVG files separately - image crate doesn't support SVG
        if path_lower.ends_with(".svg") {
            if let Some(rasterized) = Self::rasterize_svg_from_file(path) {
                match qr_scanner::scan_image(&rasterized) {
                    Ok(result) => {
                        self.result_text = result.text.clone();
                        self.history.push(result.text);
                        if self.auto_copy {
                            Self::copy_to_clipboard(&self.result_text);
                        }
                        self.debug_info = "QR code scanned from SVG file!".to_string();
                        return;
                    }
                    Err(e) => {
                        self.result_text = format!("No QR code found: {}", e);
                        self.debug_info = format!("Scan failed: {}", e);
                        return;
                    }
                }
            }
            self.result_text = "Failed to parse SVG".to_string();
            self.debug_info = "Error: could not parse SVG file".to_string();
            return;
        }
        
        // Try to open as regular image
        match image::open(path) {
            Ok(dyn_img) => {
                match qr_scanner::scan_image(&dyn_img) {
                    Ok(result) => {
                        self.result_text = result.text.clone();
                        self.history.push(result.text);
                        if self.auto_copy {
                            Self::copy_to_clipboard(&self.result_text);
                        }
                        self.debug_info = "QR code scanned from file!".to_string();
                    }
                    Err(e) => {
                        self.result_text = format!("No QR code found: {}", e);
                        self.debug_info = format!("Scan failed: {}", e);
                    }
                }
            }
            Err(e) => {
                self.result_text = format!("Failed to open image: {}", path);
                self.debug_info = format!("Error: {}", e);
            }
        }
    }

    fn rasterize_svg_from_file(path: &str) -> Option<image::DynamicImage> {
        eprintln!("Rasterizing SVG from file: {}", path);
        let svg_text = std::fs::read_to_string(path).ok()?;
        Self::rasterize_svg(&svg_text)
    }
}

impl eframe::App for QrScannerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut trigger_paste = false;

        let events = ctx.input(|i| i.events.clone());
        for event in &events {
            match event {
                egui::Event::Key { key, pressed: _, modifiers, .. } => {
                    // Trigger on V with Ctrl, regardless of pressed state (some systems report weirdly)
                    if *key == egui::Key::V && modifiers.ctrl {
                        eprintln!("Ctrl+V detected in events!");
                        trigger_paste = true;
                    }
                }
                egui::Event::Paste(_) => {
                    trigger_paste = true;
                }
                _ => {}
            }
        }

        if trigger_paste {
            self.paste_from_clipboard();
        }

        // Main UI
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("QR Scanner");
            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Scan QR (area selection)").clicked() {
                    self.start_scan();
                }
                ui.checkbox(&mut self.auto_copy, "Auto-copy");
            });
            ui.label("Click and drag to select QR code area on screen");

            ui.separator();
            ui.label("Result:");
            ui.text_edit_multiline(&mut self.result_text);

            ui.horizontal(|ui| {
                if ui.button("Copy").clicked() {
                    Self::copy_to_clipboard(&self.result_text);
                }
                if ui.button("Clear").clicked() {
                    self.result_text.clear();
                }
            });

            ui.separator();

            ui.label("Paste from clipboard:");
            ui.horizontal(|ui| {
                if ui.button("Paste Image (Ctrl+V)").clicked() {
                    self.paste_from_clipboard();
                }
            });
            ui.label("In browser: right-click image -> Copy Image");

            ui.separator();

            ui.collapsing("Or open file", |ui| {
                if ui.button("Open File...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "bmp", "webp", "svg"])
                        .pick_file()
                    {
                        self.scan_file(path.to_str().unwrap());
                    }
                }
            });

            ui.separator();

            ui.collapsing("History", |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, item) in self.history.iter().enumerate().rev().take(20) {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}:", self.history.len() - i));
                            let display = if item.len() > 40 {
                                format!("{}...", &item[..40])
                            } else {
                                item.clone()
                            };
                            ui.monospace(display);
                            if ui.button("Copy").clicked() {
                                Self::copy_to_clipboard(item);
                            }
                        });
                    }
                });
                if !self.history.is_empty() {
                    if ui.button("Clear History").clicked() {
                        self.history.clear();
                    }
                }
            });

            ui.separator();

            ui.label("Debug:");
            ui.monospace(&self.debug_info);
        });
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([450.0, 550.0])
            .with_title("QR Scanner"),
        ..Default::default()
    };

    eframe::run_native(
        "QR Scanner",
        native_options,
        Box::new(|_cc| Ok(Box::new(QrScannerApp::new()))),
    )?;

    Ok(())
}
