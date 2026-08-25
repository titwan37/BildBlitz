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
    Rename(PathBuf, String),
}

/// Actions originating from the top toolbar.
pub enum ToolbarAction {
    None,
    Rotate(u16),
    FlipH,
    FlipV,
    InitiateRenameNav(PathBuf),
    InitiateRenameGrid(PathBuf, crate::ui::pane_state::PaneSide),
}

/// Configuration for the auto-grouping clustering task.
#[derive(Clone, Debug)]
pub struct AutoGroupConfig {
    pub weight_color: f32,
    pub weight_time: f32,
    pub weight_name: f32,
    pub weight_sketch: f32,
    pub weight_binary: f32,
    pub weight_raytrace: f32,
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

/// Determinant forces as normalized percentages across all 6 driving dimensions.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeterminantForces {
    pub time: f32,
    pub color: f32,
    pub composition: f32,
    pub sketch: f32,
    pub binary: f32,
    pub raytrace: f32,
}

impl DeterminantForces {
    pub fn dominant_name(&self) -> &'static str {
        let mut max_val = self.time;
        let mut max_name = "Time";
        if self.color > max_val { max_val = self.color; max_name = "Color"; }
        if self.composition > max_val { max_val = self.composition; max_name = "Composition"; }
        if self.sketch > max_val { max_val = self.sketch; max_name = "Croquis / Sketch"; }
        if self.binary > max_val { max_val = self.binary; max_name = "Silhouette / Binaire"; }
        if self.raytrace > max_val { max_name = "3D Raytrace"; }
        max_name
    }
}

/// Performance telemetry & CPU timing breakdown across feature extractors.
#[derive(Clone, Debug, Default)]
pub struct PerformanceProfile {
    pub total_elapsed_ms: f64,
    pub total_images: usize,
    pub images_per_sec: f64,
    pub decode_ms: f64,
    pub color_extract_ms: f64,
    pub phash_ms: f64,
    pub sketch_ms: f64,
    pub binary_ms: f64,
    pub raytrace_ms: f64,
    pub clustering_ms: f64,
}

/// Result of the auto-grouping task.
#[derive(Clone, Debug, Default)]
pub struct AutoGroupResult {
    pub clusters: Vec<Cluster>,
    /// Determinant forces quantifying which of the 6 dimensions drove cluster formation.
    pub forces: DeterminantForces,
    /// Detailed execution timing and performance telemetry.
    pub perf: Option<PerformanceProfile>,
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
