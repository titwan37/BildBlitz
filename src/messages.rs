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
    pub invalidated_paths: Vec<PathBuf>,
    pub transformed_paths: Vec<PathBuf>,
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

/// Actions originating from the top toolbar.
pub enum ToolbarAction {
    None,
    Rotate(u16),
    FlipH,
    FlipV,
}

/// Configuration for the auto-grouping clustering task.
#[derive(Clone, Debug)]
pub struct AutoGroupConfig {
    pub weight_color: f32,
    pub weight_time: f32,
    pub weight_name: f32,
    pub eps: f32,
    pub min_samples: usize,
    pub create_physical: bool,
    pub source_path: PathBuf,
}

/// Progress updates from the auto-grouping background task.
#[derive(Clone, Debug)]
pub enum AutoGroupProgress {
    Extracted { done: usize, total: usize },
    Clustering { percent: f32 },
    Moving { done: usize, total: usize },
    /// Live snapshot of clusters as they grow — enables real-time UI rendering.
    VirtualClustersUpdated { clusters: Vec<Cluster> },
}

/// A resulting cluster of files.
#[derive(Clone, Debug)]
pub struct Cluster {
    pub id: usize,
    pub members: Vec<PathBuf>,
    pub label: Option<String>,
}

/// Result of the auto-grouping task.
#[derive(Clone, Debug)]
pub struct AutoGroupResult {
    pub clusters: Vec<Cluster>,
    /// Determinant forces as normalized percentages: (time%, color%, palette%)
    /// These quantify which feature dimension drove the cluster formation.
    pub forces: (f32, f32, f32),
}

/// Result of a duplicates scan.
#[derive(Clone, Debug)]
pub struct DuplicatesResult {
    pub clusters: Vec<Cluster>,
}

/// Result of the auto-tune task.
#[derive(Clone, Debug)]
pub struct AutoGroupTuneResult {
    pub optimal_eps: f32,
}

/// Messages for background tasks
#[derive(Clone, Debug)]
pub enum BackendMsg {
    AutoGroupStart(AutoGroupConfig),
    AutoGroupTuneEpsilon(AutoGroupConfig),
    AutoGroupCommit {
        result: AutoGroupResult,
        source_path: std::path::PathBuf,
    },
    AutoGroupRunStudy(AutoGroupConfig),
    DuplicatesRefresh,
    TransformRotate {
        paths: Vec<std::path::PathBuf>,
        degrees: u16,
    },
    TransformFlipH {
        paths: Vec<std::path::PathBuf>,
    },
    TransformFlipV {
        paths: Vec<std::path::PathBuf>,
    },
}

#[derive(Clone, Debug)]
pub struct AuditMsg {
    pub name: String,
    pub success: bool,
    pub message: Option<String>,
}
