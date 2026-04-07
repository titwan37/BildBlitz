use std::path::{Path, PathBuf};
use std::sync::Arc;
use moka::future::Cache;
use std::time::Duration;
use anyhow::Context;
use tokio::sync::Semaphore;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub dimensions: Option<(u32, u32)>,
    pub modified: std::time::SystemTime,
    pub is_dir: bool,
}

pub struct GalleryScanner;

impl GalleryScanner {
    pub async fn scan_directory(path: &Path) -> Vec<FileInfo> {
        let mut items = Vec::new();
        let supported_extensions = ["jpg", "jpeg", "png", "webp", "gif", "bmp"];

        info!("Scanning directory: {:?}", path);

        match std::fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    let metadata = match entry.metadata() {
                        Ok(m) => m,
                        Err(e) => {
                            warn!("Failed to read metadata for {:?}: {}", path, e);
                            continue;
                        }
                    };

                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let modified = metadata.modified().unwrap_or(std::time::SystemTime::now());

                    if metadata.is_dir() {
                        items.push(FileInfo {
                            path: path.clone(),
                            name,
                            size: 0,
                            dimensions: None,
                            modified,
                            is_dir: true,
                        });
                    } else if metadata.is_file() {
                        let ext = path.extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default()
                            .to_lowercase();
                        
                        if supported_extensions.contains(&ext.as_str()) {
                            items.push(FileInfo {
                                path: path.clone(),
                                name,
                                size: metadata.len(),
                                dimensions: None,
                                modified,
                                is_dir: false,
                            });
                        }
                    }
                }
            }
            Err(e) => error!("Failed to read directory {:?}: {}", path, e),
        }
        // Sort: Folders first, then files (A-Z)
        items.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                b.is_dir.cmp(&a.is_dir)
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
        });

        // Prepend ".." if parent exists
        if let Some(parent) = path.parent() {
            items.insert(0, FileInfo {
                path: parent.to_path_buf(),
                name: "..".to_string(),
                size: 0,
                dimensions: None,
                modified: std::time::SystemTime::now(),
                is_dir: true,
            });
        }
        
        items
    }

    pub async fn count_images(path: &Path) -> usize {
        let supported_extensions = ["jpg", "jpeg", "png", "webp", "gif", "bmp"];
        let mut count = 0;

        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.filter_map(Result::ok) {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() {
                        let path = entry.path();
                        let ext = path.extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default()
                            .to_lowercase();
                        
                        if supported_extensions.contains(&ext.as_str()) {
                            count += 1;
                        }
                    }
                }
            }
        }
        count
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
}

impl ThumbnailManager {
    pub fn new() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::env::temp_dir())
            .join("BildBlitz")
            .join("thumbnails");
        
        if !cache_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                error!("Failed to create thumbnail cache directory {:?}: {}", cache_dir, e);
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
            }),
        }
    }

    pub async fn get_thumbnail(&self, path: &PathBuf, size: u32) -> Option<Arc<egui::ColorImage>> {
        if let Some(img) = self.inner.memory_cache.get(path).await {
            return Some(img);
        }

        let cache_key = self.get_cache_key(path, size);
        let cache_path = self.inner.disk_cache_path.join(&cache_key);

        if cache_path.exists() {
            if let Ok(img) = Self::load_from_disk(&cache_path).await {
                let arc_img: Arc<egui::ColorImage> = Arc::new(img);
                self.inner.memory_cache.insert(path.clone(), arc_img.clone()).await;
                return Some(arc_img);
            }
        }

        match self.generate_thumbnail(path, size).await {
            Ok(img) => {
                let arc_img: Arc<egui::ColorImage> = Arc::new(img);
                let cache_path_clone = cache_path.clone();
                let arc_img_clone = arc_img.clone();
                tokio::spawn(async move {
                    if let Err(e) = Self::save_to_disk(&cache_path_clone, &arc_img_clone).await {
                        warn!("Failed to cache thumbnail to disk: {}", e);
                    }
                });

                self.inner.memory_cache.insert(path.clone(), arc_img.clone()).await;
                Some(arc_img)
            }
            Err(e) => {
                error!("Failed to generate thumbnail for {:?}: {}", path, e);
                None
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

    async fn generate_thumbnail(&self, path: &Path, size: u32) -> anyhow::Result<egui::ColorImage> {
        let _permit = self.inner.semaphore.acquire().await?;
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let img = image::open(&path).with_context(|| format!("Failed to open image {:?}", path))?;
            let thumbnail = img.thumbnail(size, size);
            let size = [thumbnail.width() as usize, thumbnail.height() as usize];
            let pixels = thumbnail.to_rgba8().into_raw();
            Ok(egui::ColorImage::from_rgba_unmultiplied(size, &pixels))
        }).await?
    }

    async fn load_from_disk(path: &Path) -> anyhow::Result<egui::ColorImage> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let img = image::open(&path).with_context(|| format!("Failed to open cached thumbnail {:?}", path))?;
            let size = [img.width() as usize, img.height() as usize];
            let pixels = img.to_rgba8().into_raw();
            Ok(egui::ColorImage::from_rgba_unmultiplied(size, &pixels))
        }).await?
    }

    async fn save_to_disk(path: &Path, img: &egui::ColorImage) -> anyhow::Result<()> {
        let path = path.to_owned();
        let size = img.size;
        let pixels = img.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b(), p.a()]).collect::<Vec<u8>>();
        tokio::task::spawn_blocking(move || {
            let buffer = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(size[0] as u32, size[1] as u32, pixels)
                .context("Failed to create image buffer")?;
            buffer.save(&path).with_context(|| format!("Failed to save thumbnail to {:?}", path))
        }).await?
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
                .max_capacity(32) // Keep fewer HD images in memory
                .time_to_idle(Duration::from_secs(300))
                .build(),
            semaphore: Arc::new(Semaphore::new(2)), // Load fewer HD images concurrently
        }
    }

    pub async fn get_image(&self, path: &PathBuf) -> Option<Arc<egui::ColorImage>> {
        if let Some(img) = self.memory_cache.get(path).await {
            return Some(img);
        }

        match self.load_image(path).await {
            Ok(img) => {
                let arc_img = Arc::new(img);
                self.memory_cache.insert(path.clone(), arc_img.clone()).await;
                Some(arc_img)
            }
            Err(e) => {
                error!("Failed to load full image for {:?}: {}", path, e);
                None
            }
        }
    }

    async fn load_image(&self, path: &PathBuf) -> anyhow::Result<egui::ColorImage> {
        let _permit = self.semaphore.acquire().await?;
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let img = image::open(&path).with_context(|| format!("Failed to open image {:?}", path))?;
            let size = [img.width() as usize, img.height() as usize];
            let pixels = img.to_rgba8().into_raw();
            Ok(egui::ColorImage::from_rgba_unmultiplied(size, &pixels))
        }).await?
    }
}
