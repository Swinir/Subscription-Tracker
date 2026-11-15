//! The app's icon: a ring with a dot, drawn here so it needs no image file when running.

/// The icon as `size` x `size` RGBA pixels.
pub fn rgba(size: u32) -> Vec<u8> {
    let [r, g, b, _] = crate::theme::ACCENT.to_array();
    // The shapes are measured for 32 pixels and scaled to the size asked for.
    let scale = size as f32 / 32.0;
    let centre = (size as f32 - 1.0) / 2.0;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 - centre).powi(2) + (y as f32 - centre).powi(2)).sqrt() / scale;
            let ring = (d - 9.0).min(14.5 - d);
            let dot = 4.0 - d;
            let cover = (ring.max(dot) * scale + 0.5).clamp(0.0, 1.0);
            out.extend_from_slice(&[r, g, b, (cover * 255.0) as u8]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_with_a_dot() {
        for size in [16u32, 32, 64] {
            let px = rgba(size);
            assert_eq!(px.len(), (size * size * 4) as usize);
            let alpha = |x: u32, y: u32| px[((y * size + x) * 4 + 3) as usize];
            let mid = size / 2;
            assert!(alpha(mid, mid) > 200, "the dot, at {size}");
            assert_eq!(alpha(0, 0), 0, "the corner, at {size}");
            assert_eq!(alpha(mid, mid + size / 6), 0, "between the dot and the ring, at {size}");
            assert!(alpha(mid, size * 3 / 32 + 1) > 200, "the ring, at {size}");
        }
    }

    /// Writes assets/icon.ico, the file the Windows build puts in the program:
    /// `cargo test write_icon_file -- --ignored`
    #[test]
    #[ignore]
    fn write_icon_file() {
        use image::codecs::ico::{IcoEncoder, IcoFrame};
        use image::ExtendedColorType;
        let frames: Vec<IcoFrame> = [16u32, 32, 48, 64, 128, 256]
            .iter()
            .map(|&s| IcoFrame::as_png(&rgba(s), s, s, ExtendedColorType::Rgba8).unwrap())
            .collect();
        std::fs::create_dir_all("assets").unwrap();
        IcoEncoder::new(std::fs::File::create("assets/icon.ico").unwrap()).encode_images(&frames).unwrap();
    }
}
