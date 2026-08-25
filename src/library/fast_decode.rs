// src/library/fast_decode.rs

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use image::DynamicImage;

pub enum DecodedSource {
    ExifThumbnail,
    FastBufferedStream,
}

pub struct FastDecodedImage {
    pub image: DynamicImage,
    pub source: DecodedSource,
}

pub struct FastImageDecoder;

impl FastImageDecoder {
    /// Multi-tier accelerated image decoding:
    /// 1. Instant EXIF Embedded Thumbnail Extraction (<0.5ms)
    /// 2. 128KB Large Buffered Stream Decoding (optimal for SSD & SMB network shares)
    pub fn decode_fast<P: AsRef<Path>>(path: P) -> anyhow::Result<FastDecodedImage> {
        let path_ref = path.as_ref();
        let file = File::open(path_ref)?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);

        // ── Tier 1: Try EXIF Embedded Thumbnail Extraction ────────────────
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) {
            let orientation = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0));
            
            // Check if thumbnail slice is embedded in EXIF buffer
            let thumb_data: Option<&[u8]> = {
                let offset = exif.get_field(exif::Tag::JPEGInterchangeFormat, exif::In::THUMBNAIL)
                    .and_then(|f| f.value.get_uint(0))
                    .map(|o| o as usize);
                let len = exif.get_field(exif::Tag::JPEGInterchangeFormatLength, exif::In::THUMBNAIL)
                    .and_then(|f| f.value.get_uint(0))
                    .map(|l| l as usize);
                if let (Some(off), Some(l)) = (offset, len) {
                    let buf = exif.buf();
                    if off + l <= buf.len() {
                        Some(&buf[off..off + l])
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if let Some(thumb_bytes) = thumb_data {
                if let Ok(mut thumb_img) = image::load_from_memory(thumb_bytes) {
                    if let Some(o) = orientation {
                        thumb_img = apply_exif_orientation(thumb_img, o);
                    }
                    return Ok(FastDecodedImage {
                        image: thumb_img,
                        source: DecodedSource::ExifThumbnail,
                    });
                }
            }
        }

        // ── Tier 2: Large 128KB Buffered Fallback ──────────────────────────
        let file = File::open(path_ref)?;
        let reader = BufReader::with_capacity(128 * 1024, file);
        let dyn_img = image::ImageReader::new(reader)
            .with_guessed_format()?
            .decode()?;

        Ok(FastDecodedImage {
            image: dyn_img,
            source: DecodedSource::FastBufferedStream,
        })
    }
}

fn apply_exif_orientation(img: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}
