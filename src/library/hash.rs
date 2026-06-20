// src/library/hash.rs

use anyhow::Result;
use std::path::Path;
use img_hash::{HasherConfig, ImageHash};

/// Compute a perceptual hash for the image at `path`.
/// Returns a base64 string representation of the 64‑bit hash.
pub fn compute_hash(path: &Path) -> Result<String> {
    // Use img_hash's re-exported image crate to ensure compatibility
    let img = img_hash::image::open(path)?;
    
    // Configure a hasher – 8x8 hash yields 64 bits.
    let hasher = HasherConfig::new().hash_size(8, 8).to_hasher();
    let hash: ImageHash = hasher.hash_image(&img);
    
    // Convert hash bits to a base64 string
    Ok(hash.to_base64())
}
