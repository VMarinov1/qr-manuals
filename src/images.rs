//! Намалено копие на снимките за публичната страница.
//!
//! Снимка от телефон е 3–8 MB и 4000+ px, а екранът на телефона е ~1200 px.
//! Оригиналът се пази непроменен, а на страницата се показва JPEG до 1600 px.

use std::{io::Cursor, path::Path};

use anyhow::Context;
use image::{
    codecs::jpeg::JpegEncoder, imageops::FilterType, metadata::Orientation, DynamicImage,
    GenericImageView, ImageDecoder, ImageReader, Limits, RgbImage,
};

/// Най-дългата страна на копието. Достатъчно за телефон и таблет, и за увеличаване.
pub const MAX_SIDE: u32 = 1600;
const JPEG_QUALITY: u8 = 82;
/// Малка снимка, която вече е в размер, се показва директно.
const KEEP_ORIGINAL_BELOW: u64 = 400 * 1024;
/// Защита от „бомби“: малък файл, който декомпресиран заема гигабайти.
const MAX_PIXELS_SIDE: u32 = 16_000;
const MAX_ALLOC: u64 = 768 * 1024 * 1024;

/// Прави копие за показване. `Ok(None)` значи, че оригиналът е подходящ както е.
/// Грешка значи, че файлът не е снимка, която може да се прочете.
pub fn display_copy(path: &Path, original_size: u64) -> anyhow::Result<Option<Vec<u8>>> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_PIXELS_SIDE);
    limits.max_image_height = Some(MAX_PIXELS_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);

    let mut decoder = reader.into_decoder().context("непознат формат")?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).context("повредена снимка")?;

    // Телефоните пазят снимката „легнала“ и записват завъртането в EXIF.
    // Новият JPEG е без EXIF, затова завъртането се прилага върху пикселите.
    let rotated = orientation != Orientation::NoTransforms;
    image.apply_orientation(orientation);

    let (width, height) = image.dimensions();
    let too_big = width.max(height) > MAX_SIDE;
    if !too_big && !rotated && original_size <= KEEP_ORIGINAL_BELOW {
        return Ok(None);
    }
    if too_big {
        image = image.resize(MAX_SIDE, MAX_SIDE, FilterType::CatmullRom);
    }

    let bytes = encode_jpeg(&flatten_on_white(&image))?;
    // Например малък PNG с няколко цвята може да е по-лек от JPEG.
    if !rotated && bytes.len() as u64 >= original_size {
        return Ok(None);
    }
    Ok(Some(bytes))
}

/// JPEG няма прозрачност: прозрачните части стават бели, както на хартия.
fn flatten_on_white(image: &DynamicImage) -> RgbImage {
    if !image.color().has_alpha() {
        return image.to_rgb8();
    }
    let rgba = image.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let blend = |c: u8| ((c as u16 * a as u16 + 255 * (255 - a as u16)) / 255) as u8;
        image::Rgb([blend(r), blend(g), blend(b)])
    })
}

fn encode_jpeg(image: &RgbImage) -> anyhow::Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).encode_image(image)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("qrm-test-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Шум, за да не се компресира до нищо, както истинска снимка.
    fn noisy_rgb(width: u32, height: u32) -> DynamicImage {
        let mut seed = 12345u32;
        DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |_, _| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let v = (seed >> 16) as u8;
            image::Rgb([v, v.wrapping_mul(3), v.wrapping_mul(7)])
        }))
    }

    fn encode(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    #[test]
    fn large_photo_is_scaled_down_keeping_aspect() {
        let bytes = encode(&noisy_rgb(4000, 3000), ImageFormat::Png);
        let path = write_temp("large.png", &bytes);
        let copy = display_copy(&path, bytes.len() as u64).unwrap().unwrap();
        let decoded = image::load_from_memory(&copy).unwrap();
        assert_eq!(decoded.dimensions(), (1600, 1200));
        assert!(copy.len() < bytes.len() / 4, "{} vs {}", copy.len(), bytes.len());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn small_image_is_kept_as_is() {
        let bytes = encode(&noisy_rgb(300, 200), ImageFormat::Png);
        let path = write_temp("small.png", &bytes);
        assert!(display_copy(&path, bytes.len() as u64).unwrap().is_none());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn transparent_png_becomes_white() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2000, 100, Rgba([0, 0, 0, 0])));
        let bytes = encode(&image, ImageFormat::Png);
        let path = write_temp("alpha.png", &bytes);
        // Принудително намаляване: 2000 px > MAX_SIDE.
        let copy = display_copy(&path, u64::MAX).unwrap().unwrap();
        let pixel = image::load_from_memory(&copy).unwrap().to_rgb8().get_pixel(10, 10).0;
        assert!(pixel.iter().all(|&c| c > 245), "{pixel:?}");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn exif_rotation_is_applied() {
        // Широка снимка 40×20 с EXIF orientation=6 (завърти на 90°) трябва да стане 20×40.
        let jpeg = encode(&noisy_rgb(40, 20), ImageFormat::Jpeg);
        let exif: &[u8] = b"Exif\0\0MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0";
        let mut with_exif = jpeg[..2].to_vec(); // SOI
        with_exif.extend_from_slice(&[0xFF, 0xE1]);
        with_exif.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        with_exif.extend_from_slice(exif);
        with_exif.extend_from_slice(&jpeg[2..]);

        let path = write_temp("rotated.jpg", &with_exif);
        let copy = display_copy(&path, with_exif.len() as u64).unwrap().unwrap();
        assert_eq!(image::load_from_memory(&copy).unwrap().dimensions(), (20, 40));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn broken_image_is_an_error() {
        let path = write_temp("broken.png", b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDRjunk");
        assert!(display_copy(&path, 100).is_err());
        std::fs::remove_file(path).ok();
    }
}
