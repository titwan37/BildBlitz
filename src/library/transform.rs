// src/library/transform.rs

use anyhow::Result;
use std::path::Path;
use image::GenericImageView;

/// Rotate an image by `degrees` (must be 90, 180, or 270).
/// Note: This currently performs a decode-encode cycle.
pub fn rotate(path: &Path, degrees: u16) -> Result<()> {
    let img = image::open(path)?;
    let rotated = match degrees {
        90 => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        _ => return Err(anyhow::anyhow!("Unsupported rotation angle {}", degrees)),
    };
    rotated.save(path)?;
    Ok(())
}

/// Flip image horizontally (mirror).
pub fn flip_horizontal(path: &Path) -> Result<()> {
    let img = image::open(path)?;
    let flipped = img.fliph();
    flipped.save(path)?;
    Ok(())
}

/// Flip image vertically.
pub fn flip_vertical(path: &Path) -> Result<()> {
    let img = image::open(path)?;
    let flipped = img.flipv();
    flipped.save(path)?;
    Ok(())
}
