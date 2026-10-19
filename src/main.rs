mod qr_scanner;
#[cfg(target_os = "windows")]
mod desktop_cropper;
#[cfg(target_os = "windows")]
mod overlay;

use arboard::{Clipboard, ImageData};
use eframe::egui;
use image::ImageBuffer;
use std::sync::mpsc::{self, Receiver, Sender};

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

/// Un-premultiply RGBA bytes in place. tiny-skia produces premultiplied
/// pixels; the image crate expects straight (unpremultiplied) RGBA.
pub fn demultiply_rgba(pixels: &mut [u8]) {
    for chunk in pixels.chunks_exact_mut(4) {
        let a = chunk[3];
        if a != 0 && a != 255 {
            for c in chunk[..3].iter_mut() {
                let v = ((*c as u32) * 255 + a as u32 / 2) / a as u32;
                *c = v.min(255) as u8;
            }
        }
    }
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

    // tiny-skia produces premultiplied RGBA; un-premultiply for the image crate.
    let mut pixels = pixmap.data().to_vec();
    demultiply_rgba(&mut pixels);
    if let Some(buf) = image::ImageBuffer::from_raw(width, height, pixels) {
        return Some(image::DynamicImage::ImageRgba8(buf));
    }

    None
}

/// Decode HTML entities in a single left-to-right pass, so that decoded text
/// is never decoded again (e.g. `&amp;lt;` must yield `&lt;`, not `<`).
pub fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            // Entity must end with ';' within a reasonable window.
            let win_end = (i + 1 + 12).min(bytes.len());
            if let Some(rel) = bytes[i + 1..win_end].iter().position(|&b| b == b';') {
                let entity = match s.get(i + 1..i + 1 + rel) {
                    Some(e) => e,
                    None => "",
                };
                let decoded = match entity {
                    "amp" => Some("&"),
                    "lt" => Some("<"),
                    "gt" => Some(">"),
                    "quot" => Some("\""),
                    "apos" => Some("'"),
                    _ => None,
                };
                if let Some(d) = decoded {
                    out.push_str(d);
                    i += rel + 2;
                    continue;
                }
                if let Some(rest) = entity.strip_prefix('#') {
                    let num = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        rest.parse::<u32>().ok()
                    };
                    if let Some(c) = num.and_then(char::from_u32) {
                        out.push(c);
                        i += rel + 2;
                        continue;
                    }
                }
            }
        }
        out.push(s[i..].chars().next().unwrap());
        i += s[i..].chars().next().unwrap().len_utf8();
    }
    out
}

/// Skip whitespace starting at `from`; returns index of first non-space byte.
fn skip_ws(s: &str, mut from: usize) -> usize {
    let b = s.as_bytes();
    while from < b.len() && (b[from] as char).is_whitespace() {
        from += 1;
    }
    from
}

/// Parse the `src` attribute of an `<img>` tag body (text after `<img`, without `<`/`>`).
/// Tokenizes attribute names properly so look-alikes such as `data-src` do not match.
fn parse_src_attr(tag: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut i = 0;
    while i < b.len() {
        i = skip_ws(&lower, i);
        if i >= b.len() {
            return None;
        }
        if b[i] == b'/' {
            i += 1;
            continue;
        }
        let name_start = i;
        while i < b.len()
            && b[i] != b'='
            && b[i] != b'>'
            && b[i] != b'/'
            && !(b[i] as char).is_whitespace()
        {
            i += 1;
        }
        let name = &lower[name_start..i];
        i = skip_ws(&lower, i);
        let has_value = i < b.len() && b[i] == b'=';
        if has_value {
            i += 1; // consume '='
            i = skip_ws(&lower, i);
        }
        if name == "src" && has_value && i < b.len() {
            let quote = b[i];
            if quote == b'"' || quote == b'\'' {
                i += 1;
                let start = i;
                while i < b.len() && b[i] != quote {
                    i += 1;
                }
                return Some(decode_html_entities(&tag[start..i]));
            } else {
                // Unquoted value: runs until whitespace or '>'
                let start = i;
                while i < b.len() && b[i] != b'>' && !(b[i] as char).is_whitespace() {
                    i += 1;
                }
                let val = tag[start..i].trim_end_matches('/');
                return Some(decode_html_entities(val));
            }
        }
        // Skip the value (if any) and continue with the next attribute.
        if has_value && i < b.len() {
            let quote = b[i];
            if quote == b'"' || quote == b'\'' {
                i += 1;
                while i < b.len() && b[i] != quote {
                    i += 1;
                }
                i += 1;
            } else {
                while i < b.len() && b[i] != b'>' && !(b[i] as char).is_whitespace() {
                    i += 1;
                }
            }
        }
    }
    None
}

/// Extract the `src` of the first `<img>` tag in `html`.
/// Only actual `<img>` tags count: `<script src=...>` and `data-src` attributes
/// are ignored, so lazy-loaded images no longer return their placeholder.
pub fn extract_img_url_from_html(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(pos) = lower[search_from..].find("<img") {
        let tag_start = search_from + pos + "<img".len();
        let tag_end = lower[tag_start..].find('>').map(|e| tag_start + e)?;
        if let Some(src) = parse_src_attr(&html[tag_start..tag_end]) {
            return Some(src);
        }
        search_from = tag_end;
    }
    None
}

/// Only http(s) URLs are safe to hand to curl/PowerShell: this blocks `file://`
/// reads, `javascript:` junk, and arguments/whitespace being injected as URL.
pub fn is_safe_http_url(url: &str) -> bool {
    let u = url.trim();
    !u.is_empty() && !u.chars().any(char::is_whitespace) && {
        let lower = u.to_ascii_lowercase();
        lower.starts_with("http://") || lower.starts_with("https://")
    }
}

/// Truncate to at most `max_chars` chars (char-boundary safe) with an ellipsis.
pub fn truncate_for_display(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('\u{2026}');
    out
}

/// Parse the selection geometry printed by area-selection tools. Supported shapes:
/// - `WxH+X+Y` (slop)
/// - `X,Y WxH` (slurp / grim style)
/// - `X Y W H` (four whitespace-separated tokens)
pub fn parse_selection_geometry(s: &str) -> Option<(i32, i32, u32, u32)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // slop style: 100x200+10+20
    {
        let mut it = s.split('+');
        let (wxy, xs, ys) = (it.next(), it.next(), it.next());
        if let (Some(wxy), Some(xs), Some(ys)) = (wxy, xs, ys) {
            if let Some((ws, hs)) = wxy.split_once('x') {
                let parsed = (
                    ws.parse::<u32>(),
                    hs.parse::<u32>(),
                    xs.parse::<i32>(),
                    ys.parse::<i32>(),
                );
                if let (Ok(w), Ok(h), Ok(x), Ok(y)) = parsed {
                    if w > 0 && h > 0 {
                        return Some((x, y, w, h));
                    }
                }
            }
        }
    }
    let parts: Vec<&str> = s.split_whitespace().collect();
    // slurp style: 10,20 100x200
    if parts.len() == 2 {
        if let (Some((xs, ys)), Some((ws, hs))) =
            (parts[0].split_once(','), parts[1].split_once('x'))
        {
            let parsed = (
                xs.parse::<i32>(),
                ys.parse::<i32>(),
                ws.parse::<u32>(),
                hs.parse::<u32>(),
            );
            if let (Ok(x), Ok(y), Ok(w), Ok(h)) = parsed {
                if w > 0 && h > 0 {
                    return Some((x, y, w, h));
                }
            }
        }
    }
    // four separate numbers
    if parts.len() == 4 {
        let parsed = (
            parts[0].parse::<i32>(),
            parts[1].parse::<i32>(),
            parts[2].parse::<u32>(),
            parts[3].parse::<u32>(),
        );
        if let (Ok(x), Ok(y), Ok(w), Ok(h)) = parsed {
            if w > 0 && h > 0 {
                return Some((x, y, w, h));
            }
        }
    }
    None
}

/// Run an external capture tool writing to `out_path`; return its image output
/// only when the tool exits successfully and produced a readable file.
/// A corrupt or unreadable output file yields None so the caller can try the
/// next tool instead of aborting the whole fallback chain.
#[cfg(target_os = "linux")]
fn run_capture_tool(
    bin: &str,
    args: &[&str],
    out_path: &std::path::Path,
) -> Option<image::DynamicImage> {
    let ok = std::process::Command::new(bin)
        .args(args)
        .output()
        .map(|o| o.status.success() && out_path.exists())
        .unwrap_or(false);
    if !ok {
        let _ = std::fs::remove_file(out_path);
        return None;
    }
    let img = image::open(out_path).ok();
    let _ = std::fs::remove_file(out_path);
    img
}



const MAX_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024; // 20 MiB

pub fn download_image_from_url(url: &str) -> Option<image::DynamicImage> {
    if !is_safe_http_url(url) {
        eprintln!("Refusing to download non-http(s) or empty URL: {:?}", url);
        return None;
    }
    eprintln!("Downloading image from URL: {}", url);

    // Unique temp dir: no predictable paths, no cross-instance collisions.
    let dir = tempfile::tempdir().ok()?;
    let temp_path = dir.path().join("download.bin");

    #[cfg(target_os = "windows")]
    {
        // The URL is passed via environment variables, never interpolated into
        // the PowerShell script, so it cannot break out of quoting (injection).
        let output = std::process::Command::new("powershell")
            .args(&[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Invoke-WebRequest -Uri $env:QR_URL -OutFile $env:QR_OUT -UseBasicParsing",
            ])
            .env("QR_URL", url)
            .env("QR_OUT", &temp_path)
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
            .args(&[
                "-fsSL",
                "--max-filesize",
                &MAX_DOWNLOAD_BYTES.to_string(),
                "-H",
                "User-Agent: Mozilla/5.0",
                "-o",
            ])
            .arg(&temp_path)
            .arg(url)
            .status()
            .ok()?;
        if !output.success() || !temp_path.exists() {
            eprintln!("curl download failed");
            return None;
        }
    }

    // Read downloaded bytes to detect actual content type
    let data = match std::fs::read(&temp_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to read downloaded file: {:?}", e);
            return None;
        }
    };
    if data.is_empty() || data.len() as u64 > MAX_DOWNLOAD_BYTES {
        eprintln!("Downloaded file rejected ({} bytes)", data.len());
        return None;
    }

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

/// Messages sent from background worker threads back to the UI thread.
/// `Status` is a progress update (job still running); the other variants are
/// terminal and clear the busy flag.
enum WorkerMsg {
    Status(String),
    Scanned { source: &'static str, text: String },
    ScanFailed { source: &'static str, err: String, hint: String },
    Error { result: String, debug: String },
    Done { debug: String },
}

/// Send a message to the UI thread and wake it up for a repaint.
fn post(tx: &Sender<WorkerMsg>, ctx: &egui::Context, msg: WorkerMsg) {
    let _ = tx.send(msg);
    ctx.request_repaint();
}

struct QrScannerApp {
    result_text: String,
    history: Vec<String>,
    auto_copy: bool,
    debug_info: String,
    /// Tracks the physical state of the V key so Ctrl/Cmd+V triggers once per
    /// key press instead of also firing on release or key-repeat.
    v_key_down: bool,
    /// Background job plumbing: workers never touch UI state directly, they
    /// only send `WorkerMsg`s through this channel.
    events_tx: Sender<WorkerMsg>,
    events_rx: Receiver<WorkerMsg>,
    /// True while a background capture/scan job is running; UI triggers are
    /// disabled and ignored until it finishes.
    busy: bool,
}

impl QrScannerApp {
    fn new() -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            result_text: String::new(),
            history: Vec::new(),
            auto_copy: true,
            debug_info: "Ready".to_string(),
            v_key_down: false,
            events_tx,
            events_rx,
            busy: false,
        }
    }

    fn copy_to_clipboard(text: &str) {
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(text.to_string());
        }
    }

    /// Apply one message delivered by a background worker. Kept separate from
    /// `update` so the protocol is unit-testable.
    fn handle_worker_msg(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::Status(s) => self.debug_info = s,
            WorkerMsg::Scanned { source, text } => {
                self.push_result(text);
                self.debug_info = format!("QR scanned from {}!", source);
                self.busy = false;
            }
            WorkerMsg::ScanFailed { source, err, hint } => {
                self.result_text = format!("No QR code in {}: {}", source, err);
                self.debug_info =
                    format!("No QR found: {} {}", err, hint).trim_end().to_string();
                self.busy = false;
            }
            WorkerMsg::Error { result, debug } => {
                self.result_text = result;
                self.debug_info = debug;
                self.busy = false;
            }
            WorkerMsg::Done { debug } => {
                self.debug_info = debug;
                self.busy = false;
            }
        }
    }

    fn push_result(&mut self, text: String) {
        self.result_text = text.clone();
        self.history.push(text);
        if self.auto_copy {
            Self::copy_to_clipboard(&self.result_text);
        }
    }

    /// Spawn a background worker for a long-running job (screen capture,
    /// clipboard reads, downloads, QR decoding) so the UI never freezes.
    /// The job must finish by posting exactly one terminal `WorkerMsg`
    /// (anything except `Status`).
    fn begin_job(
        &mut self,
        ctx: &egui::Context,
        status: &str,
        job: impl FnOnce(&Sender<WorkerMsg>, &egui::Context) + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        let tx = self.events_tx.clone();
        let ctx2 = ctx.clone();
        self.busy = true;
        self.debug_info = status.to_string();
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                job(&tx2, &ctx2);
            }));
            if result.is_err() {
                // Never leave the UI stuck in "busy" if a worker panicked.
                let _ = tx.send(WorkerMsg::Done {
                    debug: "Background task failed unexpectedly (panic)".to_string(),
                });
                ctx2.request_repaint();
            }
        });
    }

    fn capture_screen() -> Option<image::DynamicImage> {
        // macOS: screencapture (interactive area selection)
        #[cfg(target_os = "macos")]
        {
            use std::process::Command;

            let dir = tempfile::tempdir().ok()?;
            let temp_path = dir.path().join("capture.png");

            // Try interactive capture to file
            eprintln!("Trying screencapture -i to file...");
            let status = Command::new("screencapture")
                .arg("-i")
                .arg(&temp_path)
                .status();
            let success = status.map(|s| s.success()).unwrap_or(false);
            let file_exists = temp_path.exists();
            eprintln!(
                "screencapture -i: success={}, file_exists={}",
                success, file_exists
            );

            if success && file_exists {
                match image::open(&temp_path) {
                    Ok(img) => {
                        eprintln!("Captured image: {}x{}", img.width(), img.height());
                        return Some(img);
                    }
                    Err(e) => {
                        eprintln!("Failed to open image: {:?}", e);
                    }
                }
            }

            // Fallback: full screen capture
            eprintln!("Trying full screen capture...");
            let full_ok = Command::new("screencapture")
                .arg(&temp_path)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if full_ok && temp_path.exists() {
                if let Ok(img) = image::open(&temp_path) {
                    return Some(img);
                }
                eprintln!("Failed to open full screen capture");
            }

            eprintln!("All capture attempts failed");
        }

        // Linux: each tool goes through run_capture_tool so that a tool which
        // "succeeds" but writes an unreadable file falls through to the next
        // tool instead of aborting the whole fallback chain.
        #[cfg(target_os = "linux")]
        {
            let dir = tempfile::tempdir().ok()?;
            let temp_path = dir.path().join("capture.png");
            let path_str = temp_path.display().to_string();

            // GNOME (area selection)
            if let Some(img) =
                run_capture_tool("gnome-screenshot", &["-a", "-f", &path_str], &temp_path)
            {
                return Some(img);
            }

            // KDE Plasma (area selection; -r opens the region selector,
            // -b would silently capture the whole screen)
            if let Some(img) =
                run_capture_tool("spectacle", &["-r", "-o", &path_str], &temp_path)
            {
                return Some(img);
            }

            // Xfce
            if let Some(img) = run_capture_tool(
                "xfce4-screenshooter",
                &["-r", "-s", &path_str],
                &temp_path,
            ) {
                return Some(img);
            }

            // Sway/Wayland with grim + slop
            if let Ok(slop_out) = std::process::Command::new("slop").output() {
                if slop_out.status.success() {
                    let geom = String::from_utf8_lossy(&slop_out.stdout);
                    if let Some((x, y, w, h)) = parse_selection_geometry(&geom) {
                        let geometry = format!("{},{} {}x{}", x, y, w, h);
                        if let Some(img) =
                            run_capture_tool("grim", &["-g", &geometry], &temp_path)
                        {
                            return Some(img);
                        }
                    }
                }
            }

            // Fallbacks: full-screen captures
            if let Some(img) =
                run_capture_tool("gnome-screenshot", &["-f", &path_str], &temp_path)
            {
                return Some(img);
            }
            if let Some(img) = run_capture_tool("scrot", &[&path_str], &temp_path) {
                return Some(img);
            }
        }

        // Windows: use desktop cropper for region selection
        #[cfg(target_os = "windows")]
        {
            return desktop_cropper::start_snapshot_cropper();
        }

        // All screenshot tools failed
        #[cfg_attr(target_os = "windows", allow(unreachable_code))]
        {
            None
        }
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        self.begin_job(ctx, "Select area to capture...", |tx, ctx| {
            match Self::capture_screen() {
                Some(img) => {
                    eprintln!(
                        "capture_screen returned image: {}x{}",
                        img.width(),
                        img.height()
                    );
                    Self::scan_and_report(tx, ctx, &img, "screenshot", "Try 'Open File'.");
                }
                None => {
                    eprintln!("capture_screen returned None");
                    post(
                        tx,
                        ctx,
                        WorkerMsg::Done {
                            debug: "Screenshot not supported. Use 'Paste Image' or 'Open File'."
                                .to_string(),
                        },
                    );
                }
            }
        });
    }

    /// Decode a QR from `img` and report the outcome through the channel.
    /// `hint` is appended to the debug line when no QR is found.
    fn scan_and_report(
        tx: &Sender<WorkerMsg>,
        ctx: &egui::Context,
        img: &image::DynamicImage,
        source: &'static str,
        hint: &str,
    ) -> bool {
        match qr_scanner::scan_image(img) {
            Ok(result) => {
                post(tx, ctx, WorkerMsg::Scanned { source, text: result.text });
                true
            }
            Err(e) => {
                post(
                    tx,
                    ctx,
                    WorkerMsg::ScanFailed {
                        source,
                        err: e.to_string(),
                        hint: hint.to_string(),
                    },
                );
                false
            }
        }
    }

    fn paste_from_clipboard(&mut self, ctx: &egui::Context) {
        self.begin_job(ctx, "Checking clipboard...", Self::run_paste_job);
    }

    /// Paste pipeline, executed on a worker thread.
    fn run_paste_job(tx: &Sender<WorkerMsg>, ctx: &egui::Context) {
        #[cfg(target_os = "linux")]
        {
            // X11: read the PNG bytes straight from xclip's stdout.
            if let Ok(output) = std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "image/png", "-o"])
                .output()
            {
                if !output.stdout.is_empty() {
                    if let Ok(img) = image::load_from_memory(&output.stdout) {
                        post(
                            tx,
                            ctx,
                            WorkerMsg::Status("Loaded image via xclip!".to_string()),
                        );
                        Self::scan_and_report(tx, ctx, &img, "clipboard", "");
                        return;
                    }
                }
            }

            // Wayland: same approach with wl-paste.
            if let Ok(output) = std::process::Command::new("wl-paste")
                .args(["--type", "image/png"])
                .output()
            {
                if !output.stdout.is_empty() {
                    if let Ok(img) = image::load_from_memory(&output.stdout) {
                        post(
                            tx,
                            ctx,
                            WorkerMsg::Status("Loaded image via wl-paste!".to_string()),
                        );
                        Self::scan_and_report(tx, ctx, &img, "clipboard", "");
                        return;
                    }
                }
            }
        }

        // A more specific failure reason than the generic "no image" hint below.
        let mut hint_debug: Option<String> = None;

        // Cross-platform: arboard clipboard (bitmap image)
        if let Ok(mut clipboard) = Clipboard::new() {
            match clipboard.get_image() {
                Ok(img_data) => {
                    eprintln!(
                        "arboard get_image succeeded: {}x{} ({} bytes)",
                        img_data.width,
                        img_data.height,
                        img_data.bytes.len()
                    );
                    Self::scan_image_data_job(tx, ctx, img_data);
                    return;
                }
                Err(e) => {
                    eprintln!("arboard get_image error: {:?}", e);
                }
            }

            // arboard bitmap failed - try reading clipboard as text (SVG/HTML)
            if let Ok(text) = clipboard.get_text() {
                let trimmed = text.trim();
                let lower = trimmed.to_lowercase();

                // HTML clipboard: find the first <img src> and fetch it.
                if lower.contains("<html") || lower.contains("<img") {
                    post(
                        tx,
                        ctx,
                        WorkerMsg::Status(
                            "Detected HTML in clipboard, extracting image URL...".to_string(),
                        ),
                    );
                    if let Some(img_url) = extract_img_url_from_html(&text) {
                        eprintln!("Found image URL: {}", img_url);
                        post(
                            tx,
                            ctx,
                            WorkerMsg::Status(format!("Downloading {}", img_url)),
                        );
                        if let Some(img) = download_image_from_url(&img_url) {
                            Self::scan_and_report(tx, ctx, &img, "HTML clipboard", "");
                            return;
                        }
                        hint_debug = Some(format!(
                            "Found image URL but download failed: {}",
                            img_url
                        ));
                    } else {
                        hint_debug =
                            Some("HTML in clipboard but no img src found.".to_string());
                    }
                }

                // SVG clipboard text: rasterize directly.
                let is_svg = lower.contains("<svg")
                    || lower.contains("</svg>")
                    || (lower.contains("<?xml")
                        && lower.contains("xmlns")
                        && lower.contains("svg"))
                    || (lower.contains("xmlns=") && lower.contains("viewbox="));

                if is_svg {
                    post(
                        tx,
                        ctx,
                        WorkerMsg::Status(format!(
                            "Detected SVG in clipboard ({} bytes), rasterizing...",
                            text.len()
                        )),
                    );
                    if let Some(img) = rasterize_svg(&text) {
                        Self::scan_and_report(tx, ctx, &img, "SVG clipboard", "");
                        return;
                    }
                    post(
                        tx,
                        ctx,
                        WorkerMsg::Done {
                            debug: "SVG detected in clipboard but rasterization failed."
                                .to_string(),
                        },
                    );
                    return;
                }

                eprintln!("Clipboard text is not SVG or HTML with image.");
            }
        }

        // Windows: try PNG format from clipboard
        #[cfg(target_os = "windows")]
        {
            if let Some(img) = Self::read_png_from_clipboard_windows() {
                Self::scan_and_report(tx, ctx, &img, "clipboard", "");
                return;
            }
        }

        // macOS: try reading HTML/SVG/TIFF from the pasteboard via osascript/AppKit
        #[cfg(target_os = "macos")]
        {
            if let Some(img) = Self::read_html_or_svg_from_clipboard_macos() {
                Self::scan_and_report(tx, ctx, &img, "clipboard", "");
                return;
            }
        }

        #[cfg(target_os = "macos")]
        {
            post(
                tx,
                ctx,
                WorkerMsg::Done {
                    debug: hint_debug.unwrap_or_else(|| {
                        "No image in clipboard. Copy an image (Cmd+C in browser) then paste."
                            .to_string()
                    }),
                },
            );
        }
        #[cfg(target_os = "linux")]
        {
            let hint = if std::env::var("WAYLAND_DISPLAY").is_ok() {
                "Wayland: Right-click \u{2192} 'Copy Image', then Ctrl+V or use 'Open File'."
                    .to_string()
            } else if std::env::var("DISPLAY").is_ok() {
                "X11: Make sure xclip is installed: sudo apt install xclip".to_string()
            } else {
                "No image in clipboard. Use 'Open File'.".to_string()
            };
            post(tx, ctx, WorkerMsg::Done { debug: hint_debug.unwrap_or(hint) });
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            post(
                tx,
                ctx,
                WorkerMsg::Done {
                    debug: hint_debug.unwrap_or_else(|| {
                        "No image in clipboard. Use 'Open File'.".to_string()
                    }),
                },
            );
        }
    }

    fn scan_image_data_job(tx: &Sender<WorkerMsg>, ctx: &egui::Context, img_data: ImageData) {
        let width = img_data.width;
        let height = img_data.height;

        let sample = img_data.bytes.iter().take(16).collect::<Vec<_>>();
        eprintln!("Clipboard raw: {}x{}, first bytes: {:02X?}", width, height, sample);

        let bytes = convert_bgra_to_rgba(&img_data.bytes);
        post(tx, ctx, WorkerMsg::Status(format!("Got image: {}x{}", width, height)));

        if let Some(img) =
            ImageBuffer::<image::Rgba<u8>, _>::from_raw(width as u32, height as u32, bytes)
        {
            let dyn_img = image::DynamicImage::ImageRgba8(img);
            Self::scan_and_report(tx, ctx, &dyn_img, "clipboard image", "");
        } else {
            post(
                tx,
                ctx,
                WorkerMsg::Error {
                    result: "Failed to process image data".to_string(),
                    debug: "Image format not supported".to_string(),
                },
            );
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
            let dir = tempfile::tempdir().ok()?;
            let tmp_out = dir.path().join("clipboard.png");
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

    fn scan_file(&mut self, ctx: &egui::Context, path: String) {
        self.begin_job(ctx, &format!("Loading: {}", path), move |tx, ctx| {
            let path_lower = path.to_lowercase();

            // Handle SVG files separately - image crate doesn't support SVG
            if path_lower.ends_with(".svg") {
                match Self::rasterize_svg_from_file(&path) {
                    Some(rasterized) => {
                        Self::scan_and_report(tx, ctx, &rasterized, "SVG file", "");
                    }
                    None => {
                        post(
                            tx,
                            ctx,
                            WorkerMsg::Error {
                                result: "Failed to parse SVG".to_string(),
                                debug: "Error: could not parse SVG file".to_string(),
                            },
                        );
                    }
                }
                return;
            }

            // Try to open as regular image
            match image::open(&path) {
                Ok(dyn_img) => {
                    Self::scan_and_report(tx, ctx, &dyn_img, "file", "");
                }
                Err(e) => {
                    post(
                        tx,
                        ctx,
                        WorkerMsg::Error {
                            result: format!("Failed to open image: {}", path),
                            debug: format!("Error: {}", e),
                        },
                    );
                }
            }
        });
    }

    fn rasterize_svg_from_file(path: &str) -> Option<image::DynamicImage> {
        eprintln!("Rasterizing SVG from file: {}", path);
        let svg_text = std::fs::read_to_string(path).ok()?;
        rasterize_svg(&svg_text)
    }
}

impl eframe::App for QrScannerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Apply progress/results delivered by background workers.
        while let Ok(msg) = self.events_rx.try_recv() {
            self.handle_worker_msg(msg);
        }

        // 2. Keyboard shortcut: Cmd+V on macOS, Ctrl+V elsewhere.
        let mut trigger_paste = false;
        let events = ctx.input(|i| i.events.clone());
        for event in &events {
            match event {
                egui::Event::Key { key, pressed, modifiers, .. } => {
                    if *key == egui::Key::V {
                        let cmd_held = if cfg!(target_os = "macos") {
                            modifiers.mac_cmd
                        } else {
                            modifiers.ctrl
                        };
                        // Edge-trigger: fire once per press, ignoring key
                        // releases and auto-repeat.
                        if cmd_held && *pressed && !self.v_key_down {
                            trigger_paste = true;
                        }
                        self.v_key_down = *pressed;
                    }
                }
                egui::Event::Paste(_) => {
                    trigger_paste = true;
                }
                _ => {}
            }
        }
        if trigger_paste && !self.busy {
            self.paste_from_clipboard(ctx);
        }

        // 3. Main UI (all triggers disabled while a job is running).
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("QR Scanner");
            ui.separator();

            ui.add_enabled_ui(!self.busy, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Scan QR (area selection)").clicked() {
                        self.start_scan(ctx);
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
                    let paste_label = if cfg!(target_os = "macos") {
                        "Paste Image (Cmd+V)"
                    } else {
                        "Paste Image (Ctrl+V)"
                    };
                    if ui.button(paste_label).clicked() {
                        self.paste_from_clipboard(ctx);
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
                            match path.to_str() {
                                Some(p) => self.scan_file(ctx, p.to_string()),
                                None => {
                                    self.result_text = "Failed to open image".to_string();
                                    self.debug_info =
                                        "Error: file path is not valid UTF-8".to_string();
                                }
                            }
                        }
                    }
                });

                ui.separator();

                ui.collapsing("History", |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (i, item) in self.history.iter().enumerate().rev().take(20) {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}:", self.history.len() - i));
                                let display = truncate_for_display(item, 40);
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
            });

            ui.separator();

            ui.label("Debug:");
            ui.horizontal(|ui| {
                if self.busy {
                    ui.spinner();
                }
                ui.monospace(&self.debug_info);
            });
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

    // ---- Regression tests for review fixes ----

    #[test]
    fn test_history_truncation_multibyte_no_panic() {
        // Used to be `&item[..40]`, which panicked when byte 40 fell inside a
        // multibyte character (common for Chinese payment URLs etc).
        let item: String = std::iter::repeat('\u{652f}').take(50).collect();
        let display = truncate_for_display(&item, 40);
        assert_eq!(display.chars().count(), 41); // 40 + ellipsis
        assert!(display.ends_with('\u{2026}'));
        // multibyte string under the limit passes through untouched
        let short = "https://example.com/\u{652f}\u{4ed8}\u{4e8c}\u{7ef4}\u{7801}";
        assert_eq!(truncate_for_display(short, 40), short);
        assert_eq!(truncate_for_display("short", 40), "short");
    }

    #[test]
    fn test_decode_html_entities_no_double_decode() {
        assert_eq!(decode_html_entities("&amp;lt;"), "&lt;");
        assert_eq!(decode_html_entities("&amp;amp;"), "&amp;");
        assert_eq!(decode_html_entities("&#39;"), "'");
        assert_eq!(decode_html_entities("&#x27;"), "'");
        assert_eq!(decode_html_entities("&#60;"), "<");
        assert_eq!(decode_html_entities("&unknown;"), "&unknown;");
        assert_eq!(decode_html_entities("a&b"), "a&b");
    }

    #[test]
    fn test_extract_img_url_ignores_data_src() {
        let html = r#"<img data-src="placeholder.png" src="https://real.example/qr.png">"#;
        assert_eq!(
            extract_img_url_from_html(html),
            Some("https://real.example/qr.png".to_string())
        );
    }

    #[test]
    fn test_extract_img_url_ignores_script_src() {
        let html =
            r#"<html><head><script src="/app.js"></script><body><img src="qr.png"></body></html>"#;
        assert_eq!(extract_img_url_from_html(html), Some("qr.png".to_string()));
    }

    #[test]
    fn test_extract_img_url_src_inside_img_tag_only() {
        // src appearing outside an <img> tag must not match
        let html = r#"<div data-src="x.png"></div>"#;
        assert_eq!(extract_img_url_from_html(html), None);
    }

    #[test]
    fn test_extract_img_url_case_insensitive_and_unquoted() {
        let html = "<IMG SRC=\"https://ex.com/qr.png\">";
        assert_eq!(
            extract_img_url_from_html(html),
            Some("https://ex.com/qr.png".to_string())
        );
        let html = "<img width=10 src=https://ex.com/a.png >";
        assert_eq!(
            extract_img_url_from_html(html),
            Some("https://ex.com/a.png".to_string())
        );
    }

    #[test]
    fn test_extract_img_url_src_with_spaces_around_equals() {
        let html = r#"<img src = "https://ex.com/qr.png" >"#;
        assert_eq!(
            extract_img_url_from_html(html),
            Some("https://ex.com/qr.png".to_string())
        );
    }

    #[test]
    fn test_extract_img_url_skips_img_without_src() {
        let html = r#"<img alt="no src here"><img src="https://ex.com/qr.png">"#;
        assert_eq!(
            extract_img_url_from_html(html),
            Some("https://ex.com/qr.png".to_string())
        );
    }

    #[test]
    fn test_is_safe_http_url() {
        assert!(is_safe_http_url("http://example.com/a.png"));
        assert!(is_safe_http_url("HTTPS://example.com"));
        assert!(!is_safe_http_url(""));
        assert!(!is_safe_http_url("   "));
        assert!(!is_safe_http_url("file:///etc/passwd"));
        assert!(!is_safe_http_url("\\\\attacker\\share\\x.png"));
        assert!(!is_safe_http_url("javascript:alert(1)"));
        assert!(!is_safe_http_url("https://ex.com/a.png -o evil")); // whitespace
    }

    #[test]
    fn test_download_rejects_unsafe_url() {
        // No network access must be attempted for non-http(s) URLs.
        assert!(download_image_from_url("file:///etc/passwd").is_none());
        assert!(download_image_from_url("").is_none());
    }

    #[test]
    fn test_parse_selection_geometry_all_formats() {
        // slop format: WxH+X+Y (previously never parsed, so the sway path was dead)
        assert_eq!(parse_selection_geometry("100x200+10+20\n"), Some((10, 20, 100, 200)));
        // slurp / grim style
        assert_eq!(parse_selection_geometry("10,20 100x200"), Some((10, 20, 100, 200)));
        // four separate numbers
        assert_eq!(parse_selection_geometry("10 20 100 200"), Some((10, 20, 100, 200)));
        // invalid
        assert_eq!(parse_selection_geometry("garbage"), None);
        assert_eq!(parse_selection_geometry(""), None);
        assert_eq!(parse_selection_geometry("100x0+10+20"), None);
    }

    #[test]
    fn test_demultiply_rgba() {
        // 50%-transparent gray: premultiplied (64,64,64,128) must come back as
        // roughly (127,127,127,128), not stay at (64,64,64,128).
        let mut px = vec![64u8, 64, 64, 128, 0, 0, 0, 0, 10, 20, 30, 255];
        demultiply_rgba(&mut px);
        assert!(px[0] > 115 && px[0] < 140, "got {:?}", &px[0..4]);
        assert_eq!(px[3], 128);
        assert_eq!(&px[4..8], &[0, 0, 0, 0]); // fully transparent untouched
        assert_eq!(&px[8..12], &[10, 20, 30, 255]); // opaque untouched
    }

    // ---- Background worker / message-protocol tests ----

    #[test]
    fn test_worker_msg_protocol() {
        let mut app = QrScannerApp::new();
        app.auto_copy = false; // avoid touching the real clipboard in tests
        app.busy = true;

        // Status is non-terminal: must not clear the busy flag.
        app.handle_worker_msg(WorkerMsg::Status("working...".to_string()));
        assert_eq!(app.debug_info, "working...");
        assert!(app.busy);

        // Scanned is terminal and fills result + history.
        app.handle_worker_msg(WorkerMsg::Scanned {
            source: "clipboard",
            text: "https://ok.\u{4e8c}\u{7ef4}\u{7801}".to_string(),
        });
        assert!(!app.busy);
        assert_eq!(app.result_text, "https://ok.\u{4e8c}\u{7ef4}\u{7801}");
        assert_eq!(app.history.len(), 1);
        assert_eq!(app.debug_info, "QR scanned from clipboard!");

        // ScanFailed carries the hint through to the debug line.
        app.busy = true;
        app.handle_worker_msg(WorkerMsg::ScanFailed {
            source: "screenshot",
            err: "No QR code found".to_string(),
            hint: "Try 'Open File'.".to_string(),
        });
        assert!(!app.busy);
        assert!(app.result_text.contains("No QR code in screenshot"));
        assert!(app.debug_info.ends_with("Try 'Open File'."));

        // Error and Done are terminal too.
        app.busy = true;
        app.handle_worker_msg(WorkerMsg::Error {
            result: "r".to_string(),
            debug: "d".to_string(),
        });
        assert!(!app.busy);
        assert_eq!(app.result_text, "r");
        app.busy = true;
        app.handle_worker_msg(WorkerMsg::Done { debug: "bye".to_string() });
        assert!(!app.busy);
        assert_eq!(app.debug_info, "bye");
    }

    #[test]
    fn test_begin_job_runs_off_ui_thread_and_reports() {
        let mut app = QrScannerApp::new();
        let ctx = egui::Context::default();
        app.begin_job(&ctx, "starting", |tx, c| {
            post(tx, c, WorkerMsg::Status("mid".to_string()));
            post(
                tx,
                c,
                WorkerMsg::Done {
                    debug: "from worker".to_string(),
                },
            );
        });
        assert!(app.busy);
        // begin_job must ignore re-entrant starts while busy.
        app.begin_job(&ctx, "ignored", |tx, c| {
            post(tx, c, WorkerMsg::Done { debug: "SHOULD NOT APPEAR".to_string() });
        });
        assert_eq!(app.debug_info, "starting");

        // Drain with a timeout: messages are produced on a real thread.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut got_status = false;
        while std::time::Instant::now() < deadline {
            match app.events_rx.try_recv() {
                Ok(msg) => {
                    let terminal =
                        !matches!(msg, WorkerMsg::Status(_));
                    if matches!(msg, WorkerMsg::Status(_)) {
                        got_status = true;
                    }
                    app.handle_worker_msg(msg);
                    if terminal {
                        break;
                    }
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
        assert!(got_status, "worker never sent a Status message");
        assert!(!app.busy);
        assert_eq!(app.debug_info, "from worker");
    }

    #[test]
    fn test_begin_job_recovers_from_worker_panic() {
        let mut app = QrScannerApp::new();
        let ctx = egui::Context::default();
        app.begin_job(&ctx, "starting", |_tx, _c| {
            panic!("intentional worker panic");
        });
        assert!(app.busy);
        let msg = app
            .events_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("worker panic must still deliver a terminal message");
        app.handle_worker_msg(msg);
        assert!(!app.busy, "UI must never stay busy after a worker panic");
        assert!(app.debug_info.contains("panic"));
    }
}
