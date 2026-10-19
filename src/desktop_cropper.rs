use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use image::{DynamicImage, ImageBuffer, Rgba};

/// How long to wait for the user to finish cropping in the OS Snipping Tool.
const CROP_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(300);

/// Take a full desktop snapshot and return a cropped image via OS cropping.
///
/// The OS cropper (`ms-screenclip:`) places the crop on the clipboard. We clear
/// the clipboard first and then poll until new image data appears, so a stale
/// image left over from a previous copy can never be scanned by mistake (the
/// previous fixed `sleep(1.5s)` always read the *old* clipboard contents).
pub fn start_snapshot_cropper() -> Option<DynamicImage> {
    use std::process::Command;

    let mut clipboard = Clipboard::new().ok()?;

    // Remove any pre-existing clipboard contents so we only ever accept a
    // genuinely new crop from the snipping tool.
    let _ = clipboard.clear();

    // Launch the OS screen clip tool
    let _ = Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-Command",
            "Start-Process -WindowStyle Hidden 'ms-screenclip:'",
        ])
        .status();

    // Poll the clipboard until the user completes a crop (or times out).
    let start = Instant::now();
    while start.elapsed() < CROP_TIMEOUT {
        thread::sleep(POLL_INTERVAL);

        if let Ok(data) = clipboard.get_image() {
            eprintln!(
                "desktop_cropper: got image {}x{} ({} bytes)",
                data.width,
                data.height,
                data.bytes.len()
            );
            if data.width > 0 && data.height > 0 {
                let rgba_data = crate::convert_bgra_to_rgba(data.bytes.as_ref());

                if let Some(buf) = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
                    data.width as u32,
                    data.height as u32,
                    rgba_data,
                ) {
                    return Some(DynamicImage::ImageRgba8(buf));
                }
                // Malformed/partial bitmap - keep polling in case of a retry.
            }
        }
    }

    eprintln!("desktop_cropper: timed out waiting for a crop");
    None
}
