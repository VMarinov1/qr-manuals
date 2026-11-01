use std::io::Cursor;

use qrcode::{render::svg, EcLevel, QrCode};

/// Ниво Q възстановява до ~25% повреден код (надраскан или намачкан етикет),
/// а линкът е къс, така че кодът остава малък.
const EC_LEVEL: EcLevel = EcLevel::Q;

/// Векторен QR, подходящ за печат във всякакъв размер.
pub fn svg(data: &str) -> anyhow::Result<String> {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EC_LEVEL)?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .build())
}

/// PNG с поне `size` пиксела страна.
pub fn png(data: &str, size: u32) -> anyhow::Result<Vec<u8>> {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EC_LEVEL)?;
    let image = code
        .render::<image::Luma<u8>>()
        .min_dimensions(size, size)
        .quiet_zone(true)
        .build();
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_svg_and_png() {
        let url = "https://example.com/m/rPKrtd7k";
        assert!(svg(url).unwrap().starts_with("<?xml"));
        let png = png(url, 1200).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        let decoded = image::load_from_memory(&png).unwrap();
        assert!(decoded.width() >= 1200);
    }
}
