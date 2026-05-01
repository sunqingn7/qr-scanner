mod qr_scanner;
#[cfg(target_os = "windows")]
mod desktop_cropper;
#[cfg(target_os = "windows")]
mod overlay;

use arboard::{Clipboard, ImageData};
use eframe::egui;
use image::ImageBuffer;

pub fn convert_bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(bgra.len());
    let mut i = 0;
    while i + 3 < bgra.len() {
        let b = bgra[i];
        let g = bgra[i + 1];
        let r = bgra[i + 2];
        let a = bgra[i + 3];
        bytes.push(r);
        bytes.push(g);
        bytes.push(b);
        bytes.push(a);
        i += 4;
    }
    bytes
}

pub fn rasterize_svg(svg_text: &str) -> Option<image::DynamicImage> {
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

    let white = tiny_skia::Color::from_rgba(1.0, 1.0, 1.0, 1.0).unwrap();
    pixmap.fill(white);

    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());

    let pixels = pixmap.data().to_vec();
    if let Some(buf) = image::ImageBuffer::from_raw(width, height, pixels) {
        let debug_path = std::env::temp_dir().join("qr_rasterized.png");
        let _ = buf.save(&debug_path);
        eprintln!("Saved rasterized SVG to: {:?}", debug_path);

        return Some(image::DynamicImage::ImageRgba8(buf));
    }

    None
}

pub fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

pub fn extract_img_url_from_html(html: &str) -> Option<String> {
    if let Some(start) = html.find("src=\"") {
        let start = start + 5;
        if let Some(end) = html[start..].find('"') {
            let raw = html[start..start + end].to_string();
            return Some(decode_html_entities(&raw));
        }
    }
    if let Some(start) = html.find("src='") {
        let start = start + 5;
        if let Some(end) = html[start..].find('\'') {
            let raw = html[start..start + end].to_string();
            return Some(decode_html_entities(&raw));
        }
    }
    None
}

pub fn download_image_from_url(url: &str) -> Option<image::DynamicImage> {
    eprintln!("Downloading image from URL: {}", url);

    let temp_path = std::env::temp_dir().join("qr_downloaded");

    #[cfg(target_os = "windows")]
    {
        let ps_script = format!(
            "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing",
            url,
            temp_path.display()
        );

        let output = std::process::Command::new("powershell")
            .args(&["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .output()
            .ok()?;
        if !output.status.success() || !temp_path.exists() {
            eprintln!("PowerShell download failed");
            return None;
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let output = std::process::Command::new("curl")
            .args(&["-sL", "-H", "User-Agent: Mozilla/5.0", "-o"])
            .arg(&temp_path)
            .arg(url)
            .status()
            .ok()?;
        if !output.success() || !temp_path.exists() {
            eprintln!("curl download failed");
            return None;
        }
    }

    eprintln!("Downloaded to {:?}", temp_path);

    // Read downloaded bytes to detect actual content type
    let data = match std::fs::read(&temp_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to read downloaded file: {:?}", e);
            let _ = std::fs::remove_file(&temp_path);
            return None;
        }
    };

    let _ = std::fs::remove_file(&temp_path);

    // Check if it's actually SVG (regardless of URL extension)
    if let Ok(text) = String::from_utf8(data.clone()) {
        let trimmed = text.trim();
        if trimmed.starts_with("<?xml") || trimmed.starts_with("<svg") || trimmed.contains("<svg") {
            eprintln!("Downloaded content is SVG, rasterizing...");
            return rasterize_svg(&text);
        }
    }

    // Try loading as raster image
    match image::load_from_memory(&data) {
        Ok(img) => {
            eprintln!("Successfully loaded downloaded image!");
            Some(img)
        }
        Err(e) => {
            eprintln!("Failed to load downloaded image: {:?}", e);
            None
        }
    }
}

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
        // macOS: screencapture (interactive area selection)
        #[cfg(target_os = "macos")]
        {
            use std::process::Command;

            // Try interactive capture to file
            eprintln!("Trying screencapture -i to file...");
            let temp_path = "/tmp/qr_scan_capture.png";
            let _ = std::fs::remove_file(temp_path);
            
            let status = Command::new("screencapture")
                .arg("-i")
                .arg(temp_path)
                .status();
            let success = status.map(|s| s.success()).unwrap_or(false);
            let file_exists = std::path::Path::new(temp_path).exists();
            eprintln!("screencapture -i: success={}, file_exists={}", success, file_exists);
            
            if success && file_exists {
                match image::open(temp_path) {
                    Ok(img) => {
                        let _ = std::fs::remove_file(temp_path);
                        eprintln!("Captured image: {}x{}", img.width(), img.height());
                        return Some(img);
                    }
                    Err(e) => {
                        eprintln!("Failed to open image: {:?}", e);
                        let _ = std::fs::remove_file(temp_path);
                    }
                }
            }

            // Fallback: full screen capture
            eprintln!("Trying full screen capture...");
            let _ = std::fs::remove_file(temp_path);
            if Command::new("screencapture")
                .arg(temp_path)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
                && std::path::Path::new(temp_path).exists()
            {
                match image::open(temp_path) {
                    Ok(img) => {
                        let _ = std::fs::remove_file(temp_path);
                        return Some(img);
                    }
                    Err(e) => {
                        eprintln!("Failed to open full screen: {:?}", e);
                        let _ = std::fs::remove_file(temp_path);
                    }
                }
            }
            
            eprintln!("All capture attempts failed");
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

        // Windows: use desktop cropper for region selection
        #[cfg(target_os = "windows")]
        {
            return desktop_cropper::start_snapshot_cropper();
        }

        // All screenshot tools failed
        None
    }

    fn start_scan(&mut self) {
        self.debug_info = "Select area to capture...".to_string();

        match Self::capture_screen() {
            Some(img) => {
                eprintln!("capture_screen returned image: {}x{}", img.width(), img.height());
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
                eprintln!("capture_screen returned None");
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

        // Cross-platform: arboard clipboard (bitmap image)
        if let Ok(mut clipboard) = Clipboard::new() {
            match clipboard.get_image() {
                Ok(img_data) => {
                    eprintln!("arboard get_image succeeded: {}x{} ({} bytes)", img_data.width, img_data.height, img_data.bytes.len());
                    self.scan_image_data(img_data);
                    return;
                }
                Err(e) => {
                    eprintln!("arboard get_image error: {:?}", e);
                }
            }

            // arboard bitmap failed — try reading clipboard as text (SVG/HTML)
            match clipboard.get_text() {
                Ok(text) => {
                    let trimmed = text.trim();
                    eprintln!("arboard get_text succeeded ({} bytes), first 500 chars: {:?}", text.len(), trimmed.chars().take(500).collect::<String>());

                    // Check if it's HTML with an img src
                    let lower = trimmed.to_lowercase();
                    if lower.contains("<html") || lower.contains("<img") {
                        eprintln!("Detected HTML in clipboard, extracting image URL...");
                        if let Some(img_url) = extract_img_url_from_html(&text) {
                            eprintln!("Found image URL: {}", img_url);
                            if let Some(img) = download_image_from_url(&img_url) {
                                eprintln!("Downloaded and loaded image!");
                                match qr_scanner::scan_image(&img) {
                                    Ok(result) => {
                                        self.result_text = result.text.clone();
                                        self.history.push(result.text);
                                        if self.auto_copy {
                                            Self::copy_to_clipboard(&self.result_text);
                                        }
                                        self.debug_info =
                                            "QR scanned from HTML clipboard!".to_string();
                                        return;
                                    }
                                    Err(e) => {
                                        self.result_text = format!("No QR code: {}", e);
                                        self.debug_info = format!("No QR found: {}", e);
                                        return;
                                    }
                                }
                            } else {
                                self.debug_info = format!(
                                    "Found image URL but download failed: {}",
                                    img_url
                                );
                            }
                        } else {
                            self.debug_info = "HTML in clipboard but no img src found.".to_string();
                        }
                    }

                    // Check if it's SVG content
                    let is_svg = lower.contains("<svg")
                        || lower.contains("</svg>")
                        || lower.contains("<?xml")
                            && (lower.contains("xmlns") && lower.contains("svg"))
                        || (lower.contains("xmlns=") && lower.contains("viewbox="));

                    eprintln!("Is SVG detected: {}", is_svg);

                    if is_svg {
                        eprintln!("Detected SVG in clipboard ({} bytes), rasterizing...", text.len());
                        if let Some(img) = rasterize_svg(&text) {
                            eprintln!("SVG rasterized successfully!");
                            match qr_scanner::scan_image(&img) {
                                Ok(result) => {
                                    self.result_text = result.text.clone();
                                    self.history.push(result.text);
                                    if self.auto_copy {
                                        Self::copy_to_clipboard(&self.result_text);
                                    }
                                    self.debug_info =
                                        "QR scanned from SVG clipboard!".to_string();
                                    return;
                                }
                                Err(e) => {
                                    self.result_text = format!("No QR code in SVG: {}", e);
                                    self.debug_info = format!("No QR found in SVG: {}", e);
                                    return;
                                }
                            }
                        } else {
                            self.debug_info = "SVG detected in clipboard but rasterization failed.".to_string();
                            return;
                        }
                    }

                    eprintln!("Clipboard text is not SVG or HTML with image.");
                }
                Err(e) => {
                    eprintln!("arboard get_text error: {:?}", e);
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

        // macOS: try reading HTML/SVG from clipboard via osascript/AppKit
        #[cfg(target_os = "macos")]
        {
            if let Some(img) = Self::read_html_or_svg_from_clipboard_macos() {
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

        let bytes = convert_bgra_to_rgba(&img_data.bytes);

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
                            eprintln!(" Text content (first 200): {:?}", first_200);

                            // Check if it's HTML with img src
                            if t.contains("<html") || t.contains("<img") {
                                eprintln!(" Detected HTML, trying to extract image URL...");
                            if let Some(img_url) = extract_img_url_from_html(&text) {
                                eprintln!(" Found image URL: {}", img_url);
                                if let Some(img) = download_image_from_url(&img_url) {
                                        eprintln!(" Downloaded and loaded image!");
                                        return Some(img);
                                    }
                                }
                            }

                            // Also check for plain SVG (only if HTML didn't work)
                            let is_svg = t.contains("<svg") ||
                                t.contains("</svg>") ||
                                t.contains("xmlns=") ||
                                t.contains("viewBox=");

                            eprintln!(" Is SVG detected: {}", is_svg);

                            if is_svg {
                                eprintln!(" Detected SVG text ({} bytes), rasterizing...", data.len());
                                if let Some(img) = rasterize_svg(&text) {
                                    return Some(img);
                                } else {
                                    eprintln!(" SVG rasterization failed!");
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

    #[cfg(target_os = "macos")]
    fn read_html_or_svg_from_clipboard_macos() -> Option<image::DynamicImage> {
        use std::process::Command;

        eprintln!("Trying macOS pasteboard for HTML/SVG...");

        let types_script = r#"
use framework "AppKit"
set pb to current application's NSPasteboard's generalPasteboard()
set types to pb's types() as list
set output to ""
repeat with t in types
    set output to output & (t as text) & ","
end repeat
return output
"#;

        let types_output = Command::new("osascript")
            .args(&["-e", types_script])
            .output()
            .ok()?;

        let types_str = String::from_utf8_lossy(&types_output.stdout);
        let types: Vec<&str> = types_str.trim().split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        eprintln!("macOS pasteboard types: {:?}", types);

        let has_html = types.iter().any(|t| *t == "public.html" || *t == "Apple HTML pasteboard type");
        let has_svg = types.iter().any(|t| *t == "public.svg-image" || *t == "public.svg");
        let has_tiff = types.iter().any(|t| *t == "public.tiff" || *t == "public.png");

        if has_html {
            let html_script = r#"
use framework "AppKit"
set pb to current application's NSPasteboard's generalPasteboard()
set htmlData to pb's dataForType:"public.html"
if htmlData is missing value then
    set htmlData to pb's dataForType:"Apple HTML pasteboard type"
end if
if htmlData is not missing value then
    set htmlString to (current application's NSString's alloc()'s initWithData:htmlData encoding:4) as text
    return htmlString
end if
return ""
"#;
            if let Ok(output) = Command::new("osascript").args(&["-e", html_script]).output() {
                let html = String::from_utf8_lossy(&output.stdout).trim().to_string();
                eprintln!("macOS HTML clipboard ({} bytes), first 500: {:?}", html.len(), html.chars().take(500).collect::<String>());

                if !html.is_empty() {
                    let lower = html.to_lowercase();

                    if lower.contains("<svg") {
                        eprintln!("Found SVG in HTML clipboard!");
                        if let Some(img) = rasterize_svg(&html) {
                            return Some(img);
                        }
                    }

                    if lower.contains("<img") {
                        eprintln!("Found <img> in HTML clipboard, extracting src...");
                        if let Some(img_url) = extract_img_url_from_html(&html) {
                            eprintln!("Extracted image URL: {}", img_url);
                            if let Some(img) = download_image_from_url(&img_url) {
                                eprintln!("Downloaded image from clipboard URL!");
                                return Some(img);
                            } else {
                                eprintln!("Failed to download image from URL: {}", img_url);
                            }
                        } else {
                            eprintln!("<img> found but no src attribute");
                        }
                    }
                }
            }
        }

        if has_svg {
            let svg_script = r#"
use framework "AppKit"
set pb to current application's NSPasteboard's generalPasteboard()
set svgData to pb's dataForType:"public.svg-image"
if svgData is missing value then
    set svgData to pb's dataForType:"public.svg"
end if
if svgData is not missing value then
    set svgString to (current application's NSString's alloc()'s initWithData:svgData encoding:4) as text
    return svgString
end if
return ""
"#;
            if let Ok(output) = Command::new("osascript").args(&["-e", svg_script]).output() {
                let svg = String::from_utf8_lossy(&output.stdout).trim().to_string();
                eprintln!("macOS SVG clipboard ({} bytes)", svg.len());
                if !svg.is_empty() && svg.contains("<svg") {
                    eprintln!("Found SVG in pasteboard!");
                    if let Some(img) = rasterize_svg(&svg) {
                        return Some(img);
                    }
                }
            }
        }

        if has_tiff {
            let tmp_out = std::env::temp_dir().join("qr_clipboard_macos.png");
            let tmp_out_str = tmp_out.display().to_string();
            let img_script = format!(r#"
use framework "AppKit"
set pb to current application's NSPasteboard's generalPasteboard()
set imgData to pb's dataForType:"public.tiff"
if imgData is missing value then
    set imgData to pb's dataForType:"public.png"
end if
if imgData is not missing value then
    set nsImage to current application's NSImage's alloc()'s initWithData:imgData
    if nsImage is not missing value then
        set tiffData to nsImage's TIFFRepresentation()
        set nsBitmap to current application's NSBitmapImageRep's alloc()'s initWithData:tiffData
        set pngData to nsBitmap's representationUsingType:(current application's NSPNGFileType) |properties|:(missing value)
        pngData's writeToFile:"{}" atomically:true
        return "{}"
    end if
end if
return ""
"#, tmp_out_str, tmp_out_str);
            if let Ok(output) = Command::new("osascript").args(&["-e", &img_script]).output() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() && tmp_out.exists() {
                    eprintln!("macOS: saved clipboard image to {}", path);
                    if let Ok(img) = image::open(&tmp_out) {
                        let _ = std::fs::remove_file(&tmp_out);
                        return Some(img);
                    }
                    let _ = std::fs::remove_file(&tmp_out);
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    eprintln!("macOS TIFF osascript failed: stderr={}", err);
                }
            }
        }

        eprintln!("No image found in macOS clipboard");
        None
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
        rasterize_svg(&svg_text)
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
            ui.label("Windows: uses OS Snipping Tool. macOS/Linux: click and drag to select.");

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

#[cfg(test)]
mod tests {
    use super::*;
    use qr_scanner::scan_image;

    fn generate_qr_image(data: &str, size: usize) -> image::DynamicImage {
        use qrcode::QrCode;
        let code = QrCode::new(data).unwrap();
        let size = size as u32;
        let svg_str = code.render::<qrcode::render::svg::Color>().min_dimensions(size, size).dark_color(qrcode::render::svg::Color("#000000")).light_color(qrcode::render::svg::Color("#ffffff")).build();
        rasterize_svg(&svg_str).expect("Failed to rasterize QR SVG")
    }

    #[test]
    fn test_convert_bgra_to_rgba_red_pixel() {
        let bgra: &[u8] = &[0, 0, 255, 255];
        let rgba = convert_bgra_to_rgba(bgra);
        assert_eq!(rgba, vec![255, 0, 0, 255]);
    }

    #[test]
    fn test_convert_bgra_to_rgba_green_pixel() {
        let bgra: &[u8] = &[0, 255, 0, 255];
        let rgba = convert_bgra_to_rgba(bgra);
        assert_eq!(rgba, vec![0, 255, 0, 255]);
    }

    #[test]
    fn test_convert_bgra_to_rgba_blue_pixel() {
        let bgra: &[u8] = &[255, 0, 0, 255];
        let rgba = convert_bgra_to_rgba(bgra);
        assert_eq!(rgba, vec![0, 0, 255, 255]);
    }

    #[test]
    fn test_convert_bgra_to_rgba_multiple_pixels() {
        let bgra: &[u8] = &[
            0, 0, 255, 255,  // red
            0, 255, 0, 255,  // green
            255, 0, 0, 255,  // blue
        ];
        let rgba = convert_bgra_to_rgba(bgra);
        assert_eq!(rgba, vec![
            255, 0, 0, 255,
            0, 255, 0, 255,
            0, 0, 255, 255,
        ]);
    }

    #[test]
    fn test_convert_bgra_to_rgba_empty() {
        let bgra: &[u8] = &[];
        let rgba = convert_bgra_to_rgba(bgra);
        assert!(rgba.is_empty());
    }

    #[test]
    fn test_convert_bgra_to_rgba_incomplete_pixel_ignored() {
        let bgra: &[u8] = &[0, 0, 255, 255, 42];
        let rgba = convert_bgra_to_rgba(bgra);
        assert_eq!(rgba, vec![255, 0, 0, 255]);
    }

    #[test]
    fn test_scan_qr_from_generated_image() {
        let img = generate_qr_image("https://example.com", 256);
        let result = scan_image(&img).expect("Should decode QR");
        assert_eq!(result.text, "https://example.com");
    }

    #[test]
    fn test_scan_qr_with_longer_text() {
        let img = generate_qr_image("Hello, QR Scanner Test!", 256);
        let result = scan_image(&img).expect("Should decode QR");
        assert_eq!(result.text, "Hello, QR Scanner Test!");
    }

    #[test]
    fn test_scan_qr_no_qr_in_image() {
        let img = image::DynamicImage::ImageRgba8(
            image::ImageBuffer::from_pixel(100, 100, image::Rgba([128, 128, 128, 255]))
        );
        let result = scan_image(&img);
        assert!(result.is_err());
    }

    #[test]
    fn test_rasterize_svg_valid() {
        let svg = r#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
  <rect width="100" height="100" fill="white"/>
  <rect x="10" y="10" width="20" height="20" fill="black"/>
</svg>"#;
        let img = rasterize_svg(svg);
        assert!(img.is_some());
        let img = img.unwrap();
        assert_eq!(img.width(), 100);
        assert_eq!(img.height(), 100);
    }

    #[test]
    fn test_rasterize_svg_invalid() {
        let svg = "not an svg at all";
        let img = rasterize_svg(svg);
        assert!(img.is_none());
    }

    #[test]
    fn test_rasterize_svg_empty() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"></svg>"#;
        let img = rasterize_svg(svg);
        assert!(img.is_none());
    }

    #[test]
    fn test_rasterize_svg_qr_code() {
        let img = generate_qr_image("test-svg-qr", 128);
        let result = scan_image(&img);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().text, "test-svg-qr");
    }

    #[test]
    fn test_extract_img_url_double_quotes() {
        let html = r#"<html><body><img src="https://example.com/qr.png"></body></html>"#;
        let url = extract_img_url_from_html(html);
        assert_eq!(url, Some("https://example.com/qr.png".to_string()));
    }

    #[test]
    fn test_extract_img_url_single_quotes() {
        let html = r#"<html><body><img src='https://example.com/qr.png'></body></html>"#;
        let url = extract_img_url_from_html(html);
        assert_eq!(url, Some("https://example.com/qr.png".to_string()));
    }

    #[test]
    fn test_extract_img_url_no_src() {
        let html = "<html><body><p>No image here</p></body></html>";
        let url = extract_img_url_from_html(html);
        assert_eq!(url, None);
    }

    #[test]
    fn test_extract_img_url_empty_src() {
        let html = r#"<img src="">"#;
        let url = extract_img_url_from_html(html);
        assert_eq!(url, Some("".to_string()));
    }

    #[test]
    fn test_extract_img_url_with_surrounding_attributes() {
        let html = r#"<img alt="QR Code" src="https://cdn.example.com/qr/abc123.png" width="200">"#;
        let url = extract_img_url_from_html(html);
        assert_eq!(url, Some("https://cdn.example.com/qr/abc123.png".to_string()));
    }

    #[test]
    fn test_extract_img_url_html_entities() {
        let html = r#"<img src="https://api.qr.com/v1?text=hello&amp;width=300&amp;format=PNG">"#;
        let url = extract_img_url_from_html(html);
        assert_eq!(url, Some("https://api.qr.com/v1?text=hello&width=300&format=PNG".to_string()));
    }

    #[test]
    fn test_decode_html_entities() {
        assert_eq!(decode_html_entities("&amp;"), "&");
        assert_eq!(decode_html_entities("&lt;"), "<");
        assert_eq!(decode_html_entities("&gt;"), ">");
        assert_eq!(decode_html_entities("&quot;"), "\"");
        assert_eq!(decode_html_entities("a&amp;b&amp;c"), "a&b&c");
    }

    #[test]
    fn test_full_paste_flow_bgra_to_qr() {
        let img = generate_qr_image("https://paste-test.com", 256);
        let rgba_img = img.to_rgba8();
        let (w, h) = rgba_img.dimensions();

        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for pixel in rgba_img.pixels() {
            bgra.push(pixel[2]);
            bgra.push(pixel[1]);
            bgra.push(pixel[0]);
            bgra.push(pixel[3]);
        }

        let rgba_converted = convert_bgra_to_rgba(&bgra);
        let restored = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, rgba_converted)
            .expect("Should create image buffer");
        let dyn_img = image::DynamicImage::ImageRgba8(restored);

        let result = scan_image(&dyn_img).expect("Should decode QR from round-tripped BGRA data");
        assert_eq!(result.text, "https://paste-test.com");
    }

    #[test]
    fn test_full_paste_flow_svg_to_qr() {
        use qrcode::QrCode;
        let code = QrCode::new("https://svg-paste-test.com").unwrap();
        let svg_str = code.render::<qrcode::render::svg::Color>()
            .min_dimensions(200, 200)
            .dark_color(qrcode::render::svg::Color("#000000"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .build();

        let img = rasterize_svg(&svg_str).expect("Should rasterize SVG");
        let result = scan_image(&img).expect("Should decode QR from SVG");
        assert_eq!(result.text, "https://svg-paste-test.com");
    }
}
