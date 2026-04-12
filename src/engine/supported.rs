/// Centralized list of supported image file extensions.
/// Used by the gallery scanner, navigation tree counts, and file filtering.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "bmp", "tiff", "tif", "avif",
];

/// Checks whether a file path has a supported image extension.
pub fn is_supported_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}
