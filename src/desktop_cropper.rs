use std::thread;
use std::time::Duration;

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
        // Launch the OS screen clip tool
        let _ = Command::new("powershell")
            .args(&["-NoProfile", "-Command", "Start-Process -WindowStyle Hidden 'ms-screenclip:'"])
            .status();
        // Wait for user to crop and copy to clipboard
        thread::sleep(Duration::from_millis(1500));

        // Read image from clipboard
        let mut clipboard = Clipboard::new().ok()?;
        if let Ok(data) = clipboard.get_image() {
            eprintln!("desktop_cropper: got image {}x{} ({} bytes)", data.width, data.height, data.bytes.len());
            if data.width > 0 && data.height > 0 {
                // Debug: save image before conversion
                let debug_path = std::env::temp_dir().join("qr_clipped_raw.png");
                
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
                
                // Save debug image
                let dyn_img = DynamicImage::ImageRgba8(buf);
                if dyn_img.save(&debug_path).is_ok() {
                    eprintln!("desktop_cropper: saved debug to {:?}", debug_path);
                }
                
                return Some(dyn_img);
            }
        } else {
            eprintln!("desktop_cropper: get_image failed");
        }
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}
