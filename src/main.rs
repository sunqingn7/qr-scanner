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
                    self.scan_image_data(img_data);
                    return;
                }
                Err(e) => {
                    eprintln!("arboard get_image error: {:?}", e);
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
        let bytes = if cfg!(target_os = "macos") {
            // macOS arboard returns BGRA, convert to RGBA for correct colors
            let mut rgba_bytes = Vec::with_capacity(img_data.bytes.len());
            let mut i = 0;
            while i + 3 < img_data.bytes.len() {
                let b = img_data.bytes[i];
                let g = img_data.bytes[i + 1];
                let r = img_data.bytes[i + 2];
                let a = img_data.bytes[i + 3];
                rgba_bytes.push(r);
                rgba_bytes.push(g);
                rgba_bytes.push(b);
                rgba_bytes.push(a);
                i += 4;
            }
            rgba_bytes
        } else {
            img_data.bytes.to_vec()
        };

        self.debug_info = format!("Got image: {}x{}", width, height);

        if let Some(img) = ImageBuffer::<image::Rgba<u8>, _>::from_raw(
            width as u32,
            height as u32,
            bytes,
        ) {
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

    fn scan_file(&mut self, path: &str) {
        self.debug_info = format!("Loading: {}", path);

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
                self.result_text = format!("Failed to open image: {}", e);
                self.debug_info = format!("Error: {}", e);
            }
        }
    }
}

impl eframe::App for QrScannerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut trigger_paste = false;

        let events = ctx.input(|i| i.events.clone());
        for event in &events {
            match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if *key == egui::Key::V && modifiers.ctrl {
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
                        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "bmp", "webp"])
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
