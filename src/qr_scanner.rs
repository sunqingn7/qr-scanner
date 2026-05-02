use image::{DynamicImage, GrayImage, ImageBuffer, Luma};
use rqrr::PreparedImage;
use std::error::Error;

pub struct ScanResult {
    pub text: String,
}

fn try_scan(prepared: &mut PreparedImage<GrayImage>) -> Option<String> {
    for grid in prepared.detect_grids() {
        if let Ok((_meta, content)) = grid.decode() {
            return Some(content);
        }
    }
    None
}

pub fn scan_image(img: &DynamicImage) -> Result<ScanResult, Box<dyn Error>> {
    eprintln!("QR scan: processing image");
    
    let gray = img.to_luma8();
    let (w, h) = (gray.width(), gray.height());
    eprintln!("QR scan: grayscale {}x{}", w, h);
    
    let mut white = 0u32;
    let mut black = 0u32;
    for p in gray.pixels() {
        if p[0] > 128 { white += 1; } else { black += 1; }
    }
    eprintln!("QR scan: {} white, {} black pixels", white, black);
    
    // Try original first
    let mut prepared = PreparedImage::prepare(gray.clone());
    if let Some(text) = try_scan(&mut prepared) {
        return Ok(ScanResult { text });
    }
    
    // Try inverted image (white/black swapped)
    let mut inv = ImageBuffer::new(w, h);
    for (x, y, p) in gray.enumerate_pixels() {
        inv.put_pixel(x, y, Luma([if p[0] > 128 { 0 } else { 255 }]));
    }
    let mut prepared_inv = PreparedImage::prepare(inv);
    if let Some(text) = try_scan(&mut prepared_inv) {
        return Ok(ScanResult { text });
    }
    
    // Try binary thresholds with normal and inverted
    let thresholds = [60, 80, 100, 128, 150, 180, 200];
    for thresh in thresholds {
        let (w, h) = (gray.width(), gray.height());
        
        // Normal
        let mut bin = ImageBuffer::new(w, h);
        for (x, y, p) in gray.enumerate_pixels() {
            bin.put_pixel(x, y, Luma([if p[0] < thresh { 0 } else { 255 }]));
        }
        let mut prepared = PreparedImage::prepare(bin);
        if let Some(text) = try_scan(&mut prepared) {
            return Ok(ScanResult { text });
        }
        
        // Inverted
        let mut bin = ImageBuffer::new(w, h);
        for (x, y, p) in gray.enumerate_pixels() {
            bin.put_pixel(x, y, Luma([if p[0] >= thresh { 0 } else { 255 }]));
        }
        let mut prepared = PreparedImage::prepare(bin);
        if let Some(text) = try_scan(&mut prepared) {
            return Ok(ScanResult { text });
        }
    }

    Err("No QR code found".into())
}