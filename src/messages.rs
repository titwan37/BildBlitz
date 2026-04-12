use crate::ui::pane_state::PaneSide;
use crate::engine::gallery::FileInfo;
use std::path::PathBuf;
use std::sync::Arc;

/// Result of an asynchronous thumbnail generation.
pub struct ThumbnailResult {
    pub path: PathBuf,
    pub pane_side: PaneSide,
    pub image: Arc<egui::ColorImage>,
}

/// Result of an asynchronous directory scan.
pub struct ScanResult {
    pub pane_side: PaneSide,
    pub files: Vec<FileInfo>,
    pub invalidated_path: Option<PathBuf>,
}

/// Result of an asynchronous folder image count.
pub struct FolderCountResult {
    pub path: PathBuf,
    pub count: usize,
}

/// Result of an asynchronous full-resolution image load.
pub struct FullImageResult {
    pub path: PathBuf,
    pub image: Arc<egui::ColorImage>,
}

/// Actions originating from the navigation sidebar.
pub enum NavAction {
    None,
    Navigate(PathBuf),
    PasteInto(PathBuf),
}
