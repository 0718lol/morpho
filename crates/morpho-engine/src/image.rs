//! Native image pipeline (pure Rust, `image` crate): fast, no sidecar.

use std::io::BufWriter;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat as ImFmt, ImageReader};

use crate::error::{Error, Result};
use crate::format::Format;

fn im_format(f: Format) -> Option<ImFmt> {
    Some(match f {
        Format::Png => ImFmt::Png,
        Format::Jpg => ImFmt::Jpeg,
        Format::Gif => ImFmt::Gif,
        Format::Bmp => ImFmt::Bmp,
        Format::Tiff => ImFmt::Tiff,
        Format::Ico => ImFmt::Ico,
        _ => return None,
    })
}

/// Convert an image whose source and target are both native-image formats.
pub fn convert(src: &Path, dst: &Path, target: Format, quality: Option<u8>) -> Result<()> {
    let fmt = im_format(target)
        .ok_or_else(|| Error::Other(format!("native image pipeline cannot encode {target}")))?;
    let img = ImageReader::open(src)?.decode()?;

    match target {
        Format::Jpg => {
            // JPEG has no alpha: composite onto white first.
            let img = flatten_on_white(img);
            let file = std::fs::File::create(dst)?;
            let w = BufWriter::new(file);
            let q = quality.unwrap_or(88).clamp(1, 100);
            let enc = JpegEncoder::new_with_quality(w, q);
            img.write_with_encoder(enc)?;
        }
        _ => {
            img.save_with_format(dst, fmt)?;
        }
    }
    Ok(())
}

fn flatten_on_white(img: DynamicImage) -> DynamicImage {
    if let image::ColorType::Rgba8 = img.color() {
        let mut rgba = img.into_rgba8();
        for px in rgba.pixels_mut() {
            let a = px[3] as u32;
            if a < 255 {
                for c in 0..3 {
                    px[c] = ((px[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
                }
            }
            px[3] = 255;
        }
        return DynamicImage::ImageRgba8(rgba);
    }
    img
}

/// Decode an image and re-encode it as PNG bytes, for UI thumbnails.
pub fn thumbnail_png(src: &Path, max_edge: u32) -> Result<Vec<u8>> {
    let img = ImageReader::open(src)?.decode()?;
    let thumb = if img.width().max(img.height()) > max_edge {
        img.resize(max_edge, max_edge, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut buf = std::io::Cursor::new(Vec::new());
    thumb.write_to(&mut buf, ImFmt::Png)?;
    Ok(buf.into_inner())
}
