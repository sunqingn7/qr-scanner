use image::DynamicImage;
use rqrr::PreparedImage;
use std::error::Error;

pub struct ScanResult {
    pub text: String,
}

pub fn scan_image(img: &DynamicImage) -> Result<ScanResult, Box<dyn Error>> {
    eprintln!("QR scan: processing image");
    
    // Convert to grayscale
    let gray = img.to_luma8();
    let (w, h) = (gray.width(), gray.height());
    eprintln!("QR scan: grayscale {}x{}", w, h);
    
    // Check pixel distribution
    let mut white = 0u32;
    let mut black = 0u32;
    for p in gray.pixels() {
        if p[0] > 128 { white += 1; } else { black += 1; }
    }
    eprintln!("QR scan: {} white, {} black pixels", white, black);
    
    // Try original
    let mut prepared = PreparedImage::prepare(gray.clone());
    for grid in prepared.detect_grids() {
        if let Ok((_meta, content)) = grid.decode() {
            return Ok(ScanResult { text: content });
        }
    }
    
    // Try binary with different thresholds
    for thresh in [80, 100, 128, 150, 180] {
        let mut bin = image::ImageBuffer::new(w, h);
        for (x, y, p) in gray.enumerate_pixels() {
            bin.put_pixel(x, y, image::Luma([if p[0] < thresh { 0 } else { 255 }]));
        }
        
        let mut prepared = PreparedImage::prepare(bin);
        for grid in prepared.detect_grids() {
            if let Ok((_meta, content)) = grid.decode() {
                return Ok(ScanResult { text: content });
            }
        }
    }

    Err("No QR code found".into())
}