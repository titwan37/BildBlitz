use std::path::{Path, PathBuf};
use std::time::SystemTime;
use image::ImageReader;

#[derive(Debug, Clone)]
pub struct ImageMetadata {
    pub name: String,
    pub path: PathBuf,
    pub dimensions: Option<(u32, u32)>,
    pub size_bytes: u64,
    pub modified: SystemTime,
    pub format: String,
}

pub struct MetadataParser;

impl MetadataParser {
    pub fn extract_metadata(path: &Path) -> anyhow::Result<ImageMetadata> {
        let metadata = std::fs::metadata(path)?;
        let name = path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        
        let extension = path.extension()
            .map(|e| e.to_string_lossy().to_string().to_uppercase())
            .unwrap_or_default();

        let dimensions = match ImageReader::open(path) {
            Ok(reader) => reader.into_dimensions().ok(),
            Err(_) => None,
        };

        Ok(ImageMetadata {
            name,
            path: path.to_path_buf(),
            dimensions,
            size_bytes: metadata.len(),
            modified: metadata.modified().unwrap_or(SystemTime::now()),
            format: extension,
        })
    }
}
