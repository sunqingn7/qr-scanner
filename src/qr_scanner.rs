use image::DynamicImage;
use rqrr::PreparedImage;
use std::error::Error;

pub struct ScanResult {
    pub text: String,
}

pub fn scan_image(img: &DynamicImage) -> Result<ScanResult, Box<dyn Error>> {
    let gray = img.to_luma8();
    let mut prepared = PreparedImage::prepare(gray);
    let mut results = Vec::new();

    for grid in prepared.detect_grids() {
        match grid.decode() {
            Ok((_meta, content)) => {
                results.push(content);
            }
            Err(e) => {
                eprintln!("Failed to decode QR: {}", e);
            }
        }
    }

    if results.is_empty() {
        Err("No QR code found".into())
    } else {
        Ok(ScanResult {
            text: results.join("\n"),
        })
    }
}
