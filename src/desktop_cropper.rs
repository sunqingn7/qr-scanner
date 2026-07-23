use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use arboard::Clipboard;
#[cfg(target_os = "windows")]
use image::{DynamicImage, ImageBuffer, Rgba};

/// Take a full desktop snapshot and return a cropped image via OS cropping
/// The OS cropping is done by ms-screenclip; the result is placed on the clipboard
/// and read back here.
pub fn start_snapshot_cropper() -> Option<DynamicImage> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;

        // Snapshot whatever is currently on the clipboard so we can detect when
        // a *new* image appears (the snipping tool writes its result to the
        // clipboard after the user makes a selection). Without this, on the
        // first run we'd often pick up a stale image or no image at all.
        let prev_image_signature = capture_clipboard_image_signature();

        // Launch the OS screen clip tool
        let _ = Command::new("powershell")
            .args(&["-NoProfile", "-Command", "Start-Process -WindowStyle Hidden 'ms-screenclip:'"])
            .status();

        // The snipping tool is interactive — we cannot use a fixed sleep. We
        // need to wait until the user has actually made a selection. Poll the
        // clipboard for up to 60 seconds, returning as soon as a new image
        // appears.
        let poll_interval = Duration::from_millis(250);
        let overall_timeout = Duration::from_secs(60);
        let start = Instant::now();

        // Give the snipping tool a moment to launch before we start polling,
        // but keep this short so the user can start selecting immediately.
        thread::sleep(Duration::from_millis(300));

        loop {
            if start.elapsed() > overall_timeout {
                eprintln!("desktop_cropper: timed out waiting for snip after 60s");
                return None;
            }

            if let Some(img) = read_new_clipboard_image(&prev_image_signature) {
                return Some(img);
            }

            thread::sleep(poll_interval);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
fn capture_clipboard_image_signature() -> Option<(usize, usize, Vec<u8>)> {
    // Capture a small prefix of the current clipboard image bytes (if any) so
    // we can tell when a *new* image arrives. Returns (width, height, prefix).
    let mut clipboard = Clipboard::new().ok()?;
    match clipboard.get_image() {
        Ok(data) => {
            let prefix = data.bytes.iter().take(64).copied().collect::<Vec<_>>();
            Some((data.width, data.height, prefix))
        }
        Err(_) => None,
    }
}

#[cfg(target_os = "windows")]
fn read_new_clipboard_image(prev: &Option<(usize, usize, Vec<u8>)>) -> Option<DynamicImage> {
    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(_) => return None,
    };

    let data = match clipboard.get_image() {
        Ok(d) => d,
        Err(_) => return None,
    };

    if data.width == 0 || data.height == 0 {
        return None;
    }

    // If the clipboard content is unchanged from before we launched the snip
    // tool, ignore it — the user hasn't made a selection yet.
    if let Some((pw, ph, pprefix)) = prev {
        if data.width == *pw && data.height == *ph {
            let current_prefix: Vec<u8> =
                data.bytes.iter().take(64).copied().collect();
            if current_prefix == *pprefix {
                return None;
            }
        }
    }

    eprintln!(
        "desktop_cropper: got image {}x{} ({} bytes)",
        data.width,
        data.height,
        data.bytes.len()
    );

    // Convert BGRA to RGBA
    let mut rgba_data: Vec<u8> = Vec::with_capacity(data.bytes.len());
    let bytes = data.bytes.as_ref();
    for chunk in bytes.chunks(4) {
        if chunk.len() >= 4 {
            rgba_data.push(chunk[2]); // R
            rgba_data.push(chunk[1]); // G
            rgba_data.push(chunk[0]); // B
            rgba_data.push(chunk[3]); // A
        }
    }

    let buf = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
        data.width as u32,
        data.height as u32,
        rgba_data,
    )?;

    let debug_path = std::env::temp_dir().join("qr_clipped_raw.png");
    let dyn_img = DynamicImage::ImageRgba8(buf);
    if dyn_img.save(&debug_path).is_ok() {
        eprintln!("desktop_cropper: saved debug to {:?}", debug_path);
    }

    Some(dyn_img)
}
