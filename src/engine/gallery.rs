use std::path::{Path, PathBuf};
use std::sync::Arc;
use moka::future::Cache;
use std::time::Duration;
use anyhow::Context;
use tokio::sync::Semaphore;
use tracing::{error, info, warn};

use crate::engine::supported::is_supported_image;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FileInfo {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub dimensions: Option<(u32, u32)>,
    pub modified: std::time::SystemTime,
    pub is_dir: bool,
    pub phash: Option<String>,
}

pub struct GalleryScanner;

impl GalleryScanner {
    /// Scans a directory and returns a sorted list of folders and supported image files.
    /// All filesystem I/O is offloaded to a blocking thread via `spawn_blocking` (B6 fix).
    pub async fn scan_directory(path: &Path) -> Vec<FileInfo> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || Self::scan_directory_blocking(&path))
            .await
            .unwrap_or_else(|e| {
                error!("scan_directory task panicked: {}", e);
                Vec::new()
            })
    }

    fn scan_directory_blocking(path: &Path) -> Vec<FileInfo> {
        let mut items = Vec::new();

        info!("Scanning directory: {:?}", path);

        match std::fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries.filter_map(Result::ok) {
                    let entry_path = entry.path();
                    let metadata = match entry.metadata() {
                        Ok(m) => m,
                        Err(e) => {
                            warn!("Failed to read metadata for {:?}: {}", entry_path, e);
                            continue;
                        }
                    };

                    let name = entry_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let modified = metadata
                        .modified()
                        .unwrap_or(std::time::SystemTime::now());

                    if metadata.is_dir() {
                        items.push(FileInfo {
                            path: entry_path,
                            name,
                            size: 0,
                            dimensions: None,
                            modified,
                            is_dir: true,
                            phash: None,
                        });
                    } else if metadata.is_file() && is_supported_image(&entry_path) {
                        items.push(FileInfo {
                            path: entry_path,
                            name,
                            size: metadata.len(),
                            dimensions: None,
                            modified,
                            is_dir: false,
                            phash: None,
                        });
                    }
                }
            }
            Err(e) => error!("Failed to read directory {:?}: {}", path, e),
        }

        // Sort: Folders first, then files (A-Z, case-insensitive)
        items.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                b.is_dir.cmp(&a.is_dir)
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
        });

        // Prepend ".." if parent exists
        if let Some(parent) = path.parent() {
            items.insert(
                0,
                FileInfo {
                    path: parent.to_path_buf(),
                    name: "..".to_string(),
                    size: 0,
                    dimensions: None,
                    modified: std::time::SystemTime::now(),
                    is_dir: true,
                    phash: None,
                },
            );
        }

        items
    }

    /// Counts supported image files in a directory (non-recursive).
    /// Offloaded to a blocking thread (B6 fix).
    pub async fn count_images(path: &Path) -> usize {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || Self::count_images_blocking(&path))
            .await
            .unwrap_or(0)
    }

    fn count_images_blocking(path: &Path) -> usize {
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let count = entries
                    .filter_map(Result::ok)
                    .filter(|e| {
                        e.metadata().map(|m| m.is_file()).unwrap_or(false)
                            && is_supported_image(&e.path())
                    })
                    .count();
                info!("Counted {} images in directory: {:?}", count, path);
                count
            }
            Err(_) => 0,
        }
    }
}

#[derive(Clone)]
pub struct ThumbnailManager {
    inner: Arc<ThumbnailManagerInner>,
}

struct ThumbnailManagerInner {
    memory_cache: Cache<PathBuf, Arc<egui::ColorImage>>,
    disk_cache_path: PathBuf,
    semaphore: Arc<Semaphore>,
    audit_tx: Option<tokio::sync::mpsc::Sender<crate::messages::AuditMsg>>,
}

impl ThumbnailManager {
    pub fn new() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::env::temp_dir())
            .join("BildBlitz")
            .join("thumbnails");

        if !cache_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                error!(
                    "Failed to create thumbnail cache directory {:?}: {}",
                    cache_dir, e
                );
            } else {
                info!("Created thumbnail cache directory: {:?}", cache_dir);
            }
        }

        Self {
            inner: Arc::new(ThumbnailManagerInner {
                memory_cache: Cache::builder()
                    .max_capacity(512)
                    .time_to_idle(Duration::from_secs(600))
                    .build(),
                disk_cache_path: cache_dir,
                semaphore: Arc::new(Semaphore::new(6)),
                audit_tx: None,
            }),
        }
    }

    pub fn set_audit_tx(&mut self, tx: tokio::sync::mpsc::Sender<crate::messages::AuditMsg>) {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.audit_tx = Some(tx);
        }
    }

    pub async fn get_thumbnail(
        &self,
        path: &Path,
        size: u32,
    ) -> Option<Arc<egui::ColorImage>> {
        if let Some(img) = self.inner.memory_cache.get(&path.to_path_buf()).await {
            return Some(img);
        }

        let cache_key = self.get_cache_key(path, size);
        let cache_path = self.inner.disk_cache_path.join(&cache_key);

        if cache_path.exists() {
            if let Ok(img) = Self::load_from_disk(&cache_path).await {
                let arc_img: Arc<egui::ColorImage> = Arc::new(img);
                self.inner
                    .memory_cache
                    .insert(path.to_path_buf(), arc_img.clone())
                    .await;
                return Some(arc_img);
            }
        }

        match self.generate_thumbnail(path, size).await {
            Ok(img) => {
                let arc_img: Arc<egui::ColorImage> = Arc::new(img);
                
                // Audit is now handled inside generate_thumbnail to capture format (B13 fix)

                let cache_path_clone = cache_path.clone();
                let arc_img_clone = arc_img.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        Self::save_to_disk(&cache_path_clone, &arc_img_clone).await
                    {
                        warn!("Failed to cache thumbnail to disk: {}", e);
                    }
                });

                self.inner
                    .memory_cache
                    .insert(path.to_path_buf(), arc_img.clone())
                    .await;
                Some(arc_img)
            }
            Err(e) => {
                error!("Failed to generate thumbnail for {:?}: {}", path, e);
                if let Some(tx) = &self.inner.audit_tx {
                    let _ = tx.try_send(crate::messages::AuditMsg {
                        name: format!("Thumbnail ERR: {:?}", path.file_name().unwrap_or_default()),
                        success: false,
                        message: Some(e.to_string()),
                    });
                }
                None
            }
        }
    }

    pub async fn invalidate(&self, path: &Path) {
        // Remove from memory cache
        self.inner.memory_cache.remove(&path.to_path_buf()).await;

        // Remove from disk cache (for all likely sizes)
        // Note: size=160 is the default in GridView
        let sizes = [160, 256, 512]; 
        for &size in &sizes {
            let cache_key = self.get_cache_key(path, size);
            let cache_path = self.inner.disk_cache_path.join(&cache_key);
            if cache_path.exists() {
                let _ = tokio::fs::remove_file(cache_path).await;
            }
        }
    }

    fn get_cache_key(&self, path: &Path, size: u32) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        size.hash(&mut hasher);
        format!("{:x}.png", hasher.finish())
    }

    async fn generate_thumbnail(
        &self,
        path: &Path,
        size: u32,
    ) -> anyhow::Result<egui::ColorImage> {
        let _permit = self.inner.semaphore.acquire().await?;
        let path = path.to_owned();
        let audit_tx = self.inner.audit_tx.clone();
        tokio::task::spawn_blocking(move || {
            // Use ImageReader for more robust format detection (especially for GIF/UNC paths)
            let file = std::fs::File::open(&path)
                .with_context(|| format!("IO error opening {:?}", path))?;
            
            let reader = image::ImageReader::new(std::io::BufReader::new(file))
                .with_guessed_format()
                .with_context(|| format!("Failed to detect format for {:?}", path))?;
            
            let format = reader.format();
            
            if let Some(tx) = &audit_tx {
                let _ = tx.try_send(crate::messages::AuditMsg {
                    name: format!("Thumbnail OK: {:?} ({:?})", path.file_name().unwrap_or_default(), format),
                    success: true,
                    message: None,
                });
            }

            let img = reader.decode()
                .with_context(|| format!("Failed to decode image {:?}", path))?;
            
            let thumbnail = img.thumbnail(size, size);
            let dims = [thumbnail.width() as usize, thumbnail.height() as usize];
            let pixels = thumbnail.to_rgba8().into_raw();
            Ok(egui::ColorImage::from_rgba_unmultiplied(dims, &pixels))
        })
        .await?
    }

    async fn load_from_disk(path: &Path) -> anyhow::Result<egui::ColorImage> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&path)
                .with_context(|| format!("IO error opening cache {:?}", path))?;
            let reader = image::ImageReader::new(std::io::BufReader::new(file))
                .with_guessed_format()
                .with_context(|| format!("Failed to detect cache format for {:?}", path))?;
            let img = reader.decode()
                .with_context(|| format!("Failed to decode cache image {:?}", path))?;
            let dims = [img.width() as usize, img.height() as usize];
            let pixels = img.to_rgba8().into_raw();
            Ok(egui::ColorImage::from_rgba_unmultiplied(dims, &pixels))
        })
        .await?
    }

    async fn save_to_disk(path: &Path, img: &egui::ColorImage) -> anyhow::Result<()> {
        let path = path.to_owned();
        let size = img.size;
        let pixels = img
            .pixels
            .iter()
            .flat_map(|p| [p.r(), p.g(), p.b(), p.a()])
            .collect::<Vec<u8>>();
        tokio::task::spawn_blocking(move || {
            let buffer = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
                size[0] as u32,
                size[1] as u32,
                pixels,
            )
            .context("Failed to create image buffer")?;
            buffer
                .save(&path)
                .with_context(|| format!("Failed to save thumbnail to {:?}", path))
        })
        .await?
    }
}

#[derive(Clone)]
pub struct FullImageManager {
    memory_cache: Cache<PathBuf, Arc<egui::ColorImage>>,
    semaphore: Arc<Semaphore>,
}

impl FullImageManager {
    pub fn new() -> Self {
        Self {
            memory_cache: Cache::builder()
                .max_capacity(32)
                .time_to_idle(Duration::from_secs(300))
                .build(),
            semaphore: Arc::new(Semaphore::new(2)),
        }
    }

    pub async fn invalidate(&self, path: &Path) {
        self.memory_cache.remove(&path.to_path_buf()).await;
    }

    pub async fn get_image(&self, path: &Path) -> Option<Arc<egui::ColorImage>> {
        if let Some(img) = self.memory_cache.get(&path.to_path_buf()).await {
            return Some(img);
        }

        match self.load_image(path).await {
            Ok(img) => {
                let arc_img = Arc::new(img);
                self.memory_cache
                    .insert(path.to_path_buf(), arc_img.clone())
                    .await;
                Some(arc_img)
            }
            Err(e) => {
                error!("Failed to load full image for {:?}: {}", path, e);
                None
            }
        }
    }

    async fn load_image(&self, path: &Path) -> anyhow::Result<egui::ColorImage> {
        let _permit = self.semaphore.acquire().await?;
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&path)
                .with_context(|| format!("IO error opening {:?}", path))?;
            let reader = image::ImageReader::new(std::io::BufReader::new(file))
                .with_guessed_format()
                .with_context(|| format!("Failed to detect format for {:?}", path))?;
                
            let img = reader.decode()
                .with_context(|| format!("Failed to decode image {:?}", path))?;
                
            let dims = [img.width() as usize, img.height() as usize];
            let pixels = img.to_rgba8().into_raw();
            Ok(egui::ColorImage::from_rgba_unmultiplied(dims, &pixels))
        })
        .await?
    }
}
