//! Намалено копие на снимките за публичната страница.
//!
//! Снимка от телефон е 3–8 MB и 4000+ px, а екранът на телефона е ~1200 px.
//! Оригиналът се пази непроменен, а на страницата се показва JPEG до 1600 px.

use std::{io::Cursor, path::Path};

use image::{
    codecs::jpeg::JpegEncoder, imageops::FilterType, metadata::Orientation, DynamicImage,
    GenericImageView, ImageDecoder, ImageReader, Limits, RgbImage,
};

/// Най-дългата страна на копието. Достатъчно за телефон и таблет, и за увеличаване.
pub const MAX_SIDE: u32 = 1600;
const JPEG_QUALITY: u8 = 82;
/// Малка снимка, която вече е в размер, се показва директно.
const KEEP_ORIGINAL_BELOW: u64 = 400 * 1024;
/// Защита от „бомби“ (малък файл, който декомпресиран заема гигабайти) и таван на паметта
/// при лимита на контейнера (512 MB): 256 MB стигат за ~85 MP снимка (48 MP = 144 MB).
const MAX_PIXELS_SIDE: u32 = 16_000;
const MAX_ALLOC: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
pub enum CopyError {
    /// Снимката е над тавана на паметта (резолюция над ~85 MP).
    TooLarge,
    /// Не е снимка, която може да се прочете.
    Unreadable(anyhow::Error),
}

impl From<image::ImageError> for CopyError {
    fn from(err: image::ImageError) -> Self {
        match err {
            image::ImageError::Limits(_) => CopyError::TooLarge,
            other => CopyError::Unreadable(other.into()),
        }
    }
}

impl From<std::io::Error> for CopyError {
    fn from(err: std::io::Error) -> Self {
        CopyError::Unreadable(err.into())
    }
}

/// Прави копие за показване. `Ok(None)` значи, че оригиналът е подходящ както е.
///
/// Паметта се пази внимателно, защото снимка от 48 MP заема 144 MB декодирана:
/// освен нея има само малки буфери, без копия на пълния размер.
pub fn display_copy(path: &Path, original_size: u64) -> Result<Option<Vec<u8>>, CopyError> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_PIXELS_SIDE);
    limits.max_image_height = Some(MAX_PIXELS_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);

    let mut decoder = reader.into_decoder()?;
    // `max_alloc` по-горе не се спазва от всички декодери (JPEG го пропуска), затова
    // размерът се проверява тук, от заглавието на файла, преди да се заеме паметта.
    if decoder.total_bytes() > MAX_ALLOC {
        return Err(CopyError::TooLarge);
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let image = DynamicImage::from_decoder(decoder)?;

    let rotated = orientation != Orientation::NoTransforms;
    let (width, height) = image.dimensions();
    let too_big = width.max(height) > MAX_SIDE;
    if !too_big && !rotated && original_size <= KEEP_ORIGINAL_BELOW {
        return Ok(None);
    }

    // Първо намаляване, после завъртане: завъртането на пълната снимка би я копирало цялата.
    let mut image = if too_big { shrink(image) } else { image };
    // Телефоните пазят снимката „легнала“ и записват завъртането в EXIF.
    // Новият JPEG е без EXIF, затова завъртането се прилага върху пикселите.
    image.apply_orientation(orientation);

    let bytes = encode_jpeg(&flatten_on_white(image)).map_err(CopyError::Unreadable)?;
    // Например малък PNG с няколко цвята може да е по-лек от JPEG.
    if !rotated && bytes.len() as u64 >= original_size {
        return Ok(None);
    }
    Ok(Some(bytes))
}

/// До MAX_SIDE. Голямата снимка първо се смалява до 2×MAX_SIDE с `thumbnail`, което
/// усреднява пикселите без междинен буфер с размера на оригинала. Качественото
/// намаляване (CatmullRom) е върху вече малкото копие.
fn shrink(image: DynamicImage) -> DynamicImage {
    let image = if image.width().max(image.height()) > 2 * MAX_SIDE {
        image.thumbnail(2 * MAX_SIDE, 2 * MAX_SIDE)
    } else {
        image
    };
    image.resize(MAX_SIDE, MAX_SIDE, FilterType::CatmullRom)
}

/// JPEG няма прозрачност: прозрачните части стават бели, както на хартия.
fn flatten_on_white(image: DynamicImage) -> RgbImage {
    if !image.color().has_alpha() {
        // Без копие, ако снимката вече е RGB8 (обичайният случай при JPEG).
        return image.into_rgb8();
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
    fn oversized_jpeg_is_rejected_before_decoding() {
        // Истински малък JPEG, на който в заглавието (SOF0) са записани размери 12000×12000:
        // 432 MB декодиран, над тавана. Трябва да се откаже само по заглавието.
        let mut jpeg = encode(&noisy_rgb(16, 16), ImageFormat::Jpeg);
        let sof = jpeg.windows(2).position(|w| w == [0xFF, 0xC0]).expect("SOF0");
        jpeg[sof + 5..sof + 7].copy_from_slice(&12_000u16.to_be_bytes());
        jpeg[sof + 7..sof + 9].copy_from_slice(&12_000u16.to_be_bytes());
        let path = write_temp("oversized.jpg", &jpeg);
        let result = display_copy(&path, jpeg.len() as u64);
        assert!(matches!(result, Err(CopyError::TooLarge)), "{result:?}");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn broken_image_is_an_error() {
        let path = write_temp("broken.png", b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDRjunk");
        assert!(display_copy(&path, 100).is_err());
        std::fs::remove_file(path).ok();
    }
}
