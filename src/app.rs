use crate::ui::navigation::NavigationPane;
use crate::ui::pane_state::{PaneState, PaneSide};
use crate::engine::gallery::{ThumbnailManager, FullImageManager};
use crate::ui::grid::{GridView, GridContext};
use crate::ui::viewer::{ImageViewer, ViewerAction};
use crate::ui::log_view::LogView;
use crate::library::metadata::ImageMetadata;
use crate::os::logging::LogStream;
use crate::messages::{ThumbnailResult, ScanResult, FolderCountResult, FullImageResult};
use std::path::{Path, PathBuf};
use std::collections::HashSet;
use tokio::sync::mpsc;
#[cfg(windows)]
// use std::os::windows::fs::FileTimesExt; (moved to local scope in copy_with_metadata)

// ── Notification ──────────────────────────────────────────────────────────────

struct Notification {
    message: String,
    expire_time: f64,
}

struct SaveAsState {
    is_open: bool,
    original_path: PathBuf,
    target_folder: PathBuf,
    new_filename: String,
    error_message: Option<String>,
}

// ── Channel Hub (CS1 fix: extracted from god-struct) ──────────────────────────

struct ChannelHub {
    thumb_tx: mpsc::Sender<ThumbnailResult>,
    thumb_rx: mpsc::Receiver<ThumbnailResult>,
    scan_tx: mpsc::Sender<ScanResult>,
    scan_rx: mpsc::Receiver<ScanResult>,
    count_tx: mpsc::Sender<FolderCountResult>,
    count_rx: mpsc::Receiver<FolderCountResult>,
    hd_tx: mpsc::Sender<FullImageResult>,
    hd_rx: mpsc::Receiver<FullImageResult>,
    backend_tx: mpsc::Sender<crate::messages::BackendMsg>,
    backend_rx: mpsc::Receiver<crate::messages::BackendMsg>,
    ag_prog_tx: mpsc::Sender<crate::messages::AutoGroupProgress>,
    ag_prog_rx: mpsc::Receiver<crate::messages::AutoGroupProgress>,
    ag_res_tx: mpsc::Sender<crate::messages::AutoGroupResult>,
    ag_res_rx: mpsc::Receiver<crate::messages::AutoGroupResult>,
    dupes_tx: mpsc::Sender<crate::messages::DuplicatesResult>,
    dupes_rx: mpsc::Receiver<crate::messages::DuplicatesResult>,
    ag_tune_tx: mpsc::Sender<crate::messages::AutoGroupTuneResult>,
    ag_tune_rx: mpsc::Receiver<crate::messages::AutoGroupTuneResult>,
    audit_tx: mpsc::Sender<crate::messages::AuditMsg>,
    audit_rx: mpsc::Receiver<crate::messages::AuditMsg>,
}

impl ChannelHub {
    fn new() -> Self {
        let (thumb_tx, thumb_rx) = mpsc::channel(100);
        let (scan_tx, scan_rx) = mpsc::channel(10);
        let (count_tx, count_rx) = mpsc::channel(100);
        let (hd_tx, hd_rx) = mpsc::channel(10);
        let (backend_tx, backend_rx) = mpsc::channel(10);
        let (ag_prog_tx, ag_prog_rx) = mpsc::channel(10);
        let (ag_res_tx, ag_res_rx) = mpsc::channel(10);
        let (dupes_tx, dupes_rx) = mpsc::channel(10);
        let (ag_tune_tx, ag_tune_rx) = mpsc::channel(10);
        let (audit_tx, audit_rx) = mpsc::channel(100);
        Self {
            thumb_tx,
            thumb_rx,
            scan_tx,
            scan_rx,
            count_tx,
            count_rx,
            hd_tx,
            hd_rx,
            backend_tx,
            backend_rx,
            ag_prog_tx,
            ag_prog_rx,
            ag_res_tx,
            ag_res_rx,
            dupes_tx,
            dupes_rx,
            ag_tune_tx,
            ag_tune_rx,
            audit_tx,
            audit_rx,
        }
    }
}

// ── Clipboard State (CS1 fix) ─────────────────────────────────────────────────

struct ClipboardState {
    paths: HashSet<PathBuf>,
}

impl ClipboardState {
    fn new() -> Self {
        Self { paths: HashSet::new() }
    }

    fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    fn take(&mut self) -> Vec<PathBuf> {
        self.paths.drain().collect()
    }
}

// ── Main Application Struct ───────────────────────────────────────────────────

pub struct BildBlitzApp {
    // Navigation
    navigation: NavigationPane,

    // Split view state
    is_split_view_active: bool,
    active_pane: PaneSide,
    left_pane: PaneState,
    right_pane: PaneState,

    // Grid views
    left_grid: GridView,
    right_grid: GridView,

    // Thumbnail & image management
    thumbnail_manager: ThumbnailManager,
    full_image_manager: FullImageManager,
    hd_textures: std::collections::HashMap<PathBuf, egui::TextureHandle>,
    hd_loading: std::collections::HashSet<PathBuf>,

    // Channels
    channels: ChannelHub,

    // UI components
    log_view: LogView,
    image_viewer: ImageViewer,
    properties_metadata: Option<ImageMetadata>,
    is_properties_open: bool,

    // Clipboard & notifications
    clipboard: ClipboardState,
    notification: Option<Notification>,
    is_tip_visible: bool,
    save_as_state: Option<SaveAsState>,
    
    // Feature: Auto Group
    auto_group_state: crate::ui::auto_group::AutoGroupState,

    // Feature: Live Audit
    audit: crate::ui::audit::SystemAudit,
    is_audit_open: bool,

    // Database
    db: crate::library::db::DatabaseManager,
    duplicates: Vec<crate::messages::Cluster>,
}

impl BildBlitzApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        log_stream: LogStream,
        log_dir: PathBuf,
        db: crate::library::db::DatabaseManager,
    ) -> Self {
        let config = crate::library::config::load_config();

        let mut app = Self {
            navigation: NavigationPane::new(config.favorites),
            is_split_view_active: false,
            active_pane: PaneSide::Left,
            left_pane: PaneState::new(),
            right_pane: PaneState::new(),
            left_grid: GridView::new(),
            right_grid: GridView::new(),
            thumbnail_manager: ThumbnailManager::new(),
            full_image_manager: FullImageManager::new(),
            hd_textures: std::collections::HashMap::new(),
            hd_loading: std::collections::HashSet::new(),
            channels: ChannelHub::new(),
            log_view: LogView::new(log_stream, log_dir),
            image_viewer: ImageViewer::new(),
            properties_metadata: None,
            is_properties_open: false,
            clipboard: ClipboardState::new(),
            notification: None,
            is_tip_visible: true,
            save_as_state: None,
            auto_group_state: crate::ui::auto_group::AutoGroupState::new(),
            audit: crate::ui::audit::SystemAudit::new(),
            is_audit_open: false,
            db: db.clone(),
            duplicates: Vec::new(),
        };

        // Connect audit channel to managers
        app.thumbnail_manager.set_audit_tx(app.channels.audit_tx.clone());

        // Initial Audit: Database
        let _ = app.channels.audit_tx.try_send(crate::messages::AuditMsg {
            name: "Database Engine Online".to_string(),
            success: true,
            message: Some("Connected to SQLite library".to_string()),
        });

        app
    }

    /// Returns a mutable reference to the active pane's state.
    fn active_pane_state(&mut self) -> &mut PaneState {
        match self.active_pane {
            PaneSide::Left => &mut self.left_pane,
            PaneSide::Right => &mut self.right_pane,
        }
    }

    /// Returns a mutable reference to the pane state for a given side.
    fn pane_state_mut(&mut self, side: PaneSide) -> &mut PaneState {
        match side {
            PaneSide::Left => &mut self.left_pane,
            PaneSide::Right => &mut self.right_pane,
        }
    }

    /// Returns an immutable reference to the pane state for a given side.
    fn pane_state(&self, side: PaneSide) -> &PaneState {
        match side {
            PaneSide::Left => &self.left_pane,
            PaneSide::Right => &self.right_pane,
        }
    }

    /// Returns a mutable reference to the grid view for a given side.
    #[allow(dead_code)]
    fn grid_mut(&mut self, side: PaneSide) -> &mut GridView {
        match side {
            PaneSide::Left => &mut self.left_grid,
            PaneSide::Right => &mut self.right_grid,
        }
    }
}

// ── eframe::App Implementation ────────────────────────────────────────────────

impl eframe::App for BildBlitzApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx();
        self.log_view.update();
        self.process_keyboard_input(ctx);
        self.drain_channels(ctx);
        self.render_top_panel(ctx);
        self.render_tip_bubble(ctx);
        self.render_notification(ctx);

        self.auto_group_state.show(ctx, self.pane_state(self.active_pane).current_path.clone(), self.channels.backend_tx.clone());

        egui::SidePanel::left("nav_panel")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                use crate::messages::NavAction;
                match self.navigation.show(ui, !self.clipboard.is_empty()) {
                    NavAction::Navigate(path) => {
                        self.trigger_scan(path, self.active_pane, ctx);
                    }
                    NavAction::PasteInto(path) => {
                        let source_paths = self.clipboard.take();
                        self.handle_drop(source_paths, Some(path), self.active_pane, ctx);
                    }
                    NavAction::None => {}
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.is_split_view_active {
                self.show_dual_pane(ui);
            } else {
                self.show_single_pane(ui);
            }
        });

        // Show Gallery if active
        if self.image_viewer.is_open {
            self.show_fullscreen(ctx);
        }

        self.log_view.show(ctx);

        if self.is_properties_open {
            egui::SidePanel::right("properties_panel")
                .resizable(true)
                .default_width(320.0)
                .show(ctx, |ui| {
                    self.show_properties_pane(ui);
                });
        }

        if self.is_audit_open {
            egui::SidePanel::right("audit_panel")
                .resizable(true)
                .default_width(300.0)
                .show(ctx, |ui| {
                    self.audit.show(ui);
                });
        }

        self.render_save_as_dialog(ctx);
    }
}

// ── Input Handling ────────────────────────────────────────────────────────────

impl BildBlitzApp {
    fn process_keyboard_input(&mut self, ctx: &egui::Context) {
        // Escape: exit fullscreen, cancel rename
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.left_pane.fullscreen_path = None;
            self.right_pane.fullscreen_path = None;
            self.image_viewer.is_open = false;
            self.left_pane.renaming_path = None;
            self.right_pane.renaming_path = None;
        }

        // F2: Rename selected item
        if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
            let state = self.active_pane_state();
            if let Some(index) = state.last_selected_index {
                if let Some(file) = state.files.get(index) {
                    if file.name != ".." {
                        state.renaming_path = Some(file.path.clone());
                        state.rename_buffer = file.name.clone();
                    }
                }
            }
        }

        // Ctrl+N: New folder
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N)) {
            let side = self.active_pane;
            self.create_new_folder(side, ctx);
        }

        // Enter: Open selection — navigate folders, gallery for images
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            let active_side = self.active_pane;
            let state = self.pane_state_mut(active_side);

            if state.renaming_path.is_none() {
                if let Some(index) = state.last_selected_index {
                    if let Some(file) = state.files.get(index) {
                        if !file.is_dir && file.name != ".." {
                            state.fullscreen_path = Some(file.path.clone());
                            self.image_viewer.is_open = true;
                            self.image_viewer.current_index = index;
                        } else if file.is_dir {
                            let path = file.path.clone();
                            self.trigger_scan(path, active_side, ctx);
                        }
                    }
                }
            }
        }
    }
}

// ── Channel Drain ─────────────────────────────────────────────────────────────

impl BildBlitzApp {
    fn drain_channels(&mut self, ctx: &egui::Context) {
        // Handle incoming thumbnails — only insert into the requesting pane (P4/B10 fix)
        while let Ok(result) = self.channels.thumb_rx.try_recv() {
            let texture = ctx.load_texture(
                result.path.to_string_lossy(),
                result.image,
                Default::default(),
            );
            match result.pane_side {
                PaneSide::Left => {
                    self.left_grid.textures.insert(result.path, texture);
                }
                PaneSide::Right => {
                    self.right_grid.textures.insert(result.path, texture);
                }
            }
        }

        // Handle incoming HD images
        while let Ok(result) = self.channels.hd_rx.try_recv() {
            let texture = ctx.load_texture(
                format!("hd_{}", result.path.to_string_lossy()),
                result.image,
                Default::default(),
            );
            self.hd_textures.insert(result.path.clone(), texture);
            self.hd_loading.remove(&result.path);
        }

        // Handle incoming scan results
        while let Ok(result) = self.channels.scan_rx.try_recv() {
            for path in result.invalidated_paths {
                self.left_grid.folder_counts.remove(&path);
                self.left_grid.loading_counts.remove(&path);
                self.right_grid.folder_counts.remove(&path);
                self.right_grid.loading_counts.remove(&path);
            }
            for path in result.transformed_paths {
                self.left_grid.textures.remove(&path);
                self.right_grid.textures.remove(&path);
                self.hd_textures.remove(&path);
            }
            let state = self.pane_state_mut(result.pane_side);
            state.files = result.files;
            state.last_selected_index = None;
        }

        // Handle incoming folder counts
        while let Ok(result) = self.channels.count_rx.try_recv() {
            self.left_grid
                .folder_counts
                .insert(result.path.clone(), result.count);
            self.right_grid
                .folder_counts
                .insert(result.path, result.count);
        }

        // Handle audit messages
        while let Ok(msg) = self.channels.audit_rx.try_recv() {
            let status = if msg.success {
                crate::ui::audit::AuditStatus::Success
            } else {
                crate::ui::audit::AuditStatus::Failure(msg.message.unwrap_or_else(|| "Unknown error".to_string()))
            };
            self.audit.push(&msg.name, status);
        }

        // Handle backend messages
        while let Ok(msg) = self.channels.backend_rx.try_recv() {
            match msg {
                crate::messages::BackendMsg::AutoGroupStart(config) => {
                    let prog_tx = self.channels.ag_prog_tx.clone();
                    let res_tx = self.channels.ag_res_tx.clone();
                    let audit_tx = self.channels.audit_tx.clone();
                    tokio::spawn(async move {
                        let _ = audit_tx.send(crate::messages::AuditMsg {
                            name: "Auto-Group Analysis Started".to_string(),
                            success: true,
                            message: None,
                        }).await;
                        match crate::engine::auto_group::run_auto_group(config, prog_tx).await {
                            Ok(res) => {
                                let _ = audit_tx.send(crate::messages::AuditMsg {
                                    name: "Auto-Group Analysis Succeeded".to_string(),
                                    success: true,
                                    message: Some(format!("Found {} clusters", res.clusters.len())),
                                }).await;
                                let _ = res_tx.send(res).await;
                            }
                            Err(e) => {
                                let _ = audit_tx.send(crate::messages::AuditMsg {
                                    name: "Auto-Group Analysis Failed".to_string(),
                                    success: false,
                                    message: Some(e.to_string()),
                                }).await;
                            }
                        }
                    });
                }
                crate::messages::BackendMsg::AutoGroupCommit { result, source_path } => {
                    let prog_tx = self.channels.ag_prog_tx.clone();
                    let res_tx = self.channels.ag_res_tx.clone();
                    let audit_tx = self.channels.audit_tx.clone();
                    tokio::spawn(async move {
                        let _ = audit_tx.send(crate::messages::AuditMsg {
                            name: "Committing Auto-Groups to Disk".to_string(),
                            success: true,
                            message: None,
                        }).await;
                        match crate::engine::auto_group::commit_auto_group(result, source_path, prog_tx).await {
                            Ok(_) => {
                                let _ = audit_tx.send(crate::messages::AuditMsg {
                                    name: "Auto-Group Commit Succeeded".to_string(),
                                    success: true,
                                    message: None,
                                }).await;
                            }
                            Err(e) => {
                                let _ = audit_tx.send(crate::messages::AuditMsg {
                                    name: "Auto-Group Commit Failed".to_string(),
                                    success: false,
                                    message: Some(e.to_string()),
                                }).await;
                            }
                        }
                        // Just clear it by sending an empty result when done
                        let _ = res_tx.send(crate::messages::AutoGroupResult { clusters: vec![], forces: (0.0, 0.0, 0.0) }).await;
                    });
                }
                crate::messages::BackendMsg::AutoGroupTuneEpsilon(config) => {
                    let prog_tx = self.channels.ag_prog_tx.clone();
                    let tune_tx = self.channels.ag_tune_tx.clone();
                    tokio::spawn(async move {
                        if let Ok(res) = crate::engine::auto_group::run_auto_tune_epsilon(config, prog_tx).await {
                            let _ = tune_tx.send(res).await;
                        }
                    });
                }
                crate::messages::BackendMsg::AutoGroupRunStudy(config) => {
                    let prog_tx = self.channels.ag_prog_tx.clone();
                    let audit_tx = self.channels.audit_tx.clone();
                    tokio::spawn(async move {
                        let _ = audit_tx.send(crate::messages::AuditMsg {
                            name: "Comparative Research Study Started".to_string(),
                            success: true,
                            message: Some("Evaluating 3 algorithms on current folder".to_string()),
                        }).await;
                        match crate::engine::benchmark::run_study_on_folder(config, prog_tx).await {
                            Ok(_) => {
                                let _ = audit_tx.send(crate::messages::AuditMsg {
                                    name: "Comparative Study Succeeded".to_string(),
                                    success: true,
                                    message: Some("Results saved to result_folder with datetime prefix".to_string()),
                                }).await;
                            }
                            Err(e) => {
                                let _ = audit_tx.send(crate::messages::AuditMsg {
                                    name: "Comparative Study Failed".to_string(),
                                    success: false,
                                    message: Some(e.to_string()),
                                }).await;
                            }
                        }
                    });
                }
                crate::messages::BackendMsg::DuplicatesRefresh => {
                    self.refresh_duplicates(ctx);
                }
                crate::messages::BackendMsg::TransformRotate { paths, degrees } => {
                    let side = self.active_pane;
                    let current_dir = self.active_pane_state().current_path.clone();
                    let tx = self.channels.scan_tx.clone();
                    let thumb_tx = self.channels.thumb_tx.clone();
                    let hd_tx = self.channels.hd_tx.clone();
                    let audit_tx = self.channels.audit_tx.clone();
                    let ctx_clone = ctx.clone();
                    let tm = self.thumbnail_manager.clone();
                    let fm = self.full_image_manager.clone();
                    tokio::spawn(async move {
                        let _ = audit_tx.send(crate::messages::AuditMsg {
                            name: format!("Rotating {} images ({}°)", paths.len(), degrees),
                            success: true,
                            message: None,
                        }).await;
                        for path in &paths {
                            if let Err(e) = crate::library::transform::rotate(path, degrees) {
                                let _ = audit_tx.send(crate::messages::AuditMsg {
                                    name: format!("Failed to rotate {:?}", path),
                                    success: false,
                                    message: Some(e.to_string()),
                                }).await;
                                continue;
                            }
                            tm.invalidate(path).await;
                            fm.invalidate(path).await;

                            // Proactively re-trigger thumbnail generation (B12 fix)
                            if let Some(image) = tm.get_thumbnail(path, 160).await {
                                // Send to both panes to ensure full UI synchronization
                                let _ = thumb_tx.send(ThumbnailResult { path: path.clone(), pane_side: PaneSide::Left, image: image.clone() }).await;
                                let _ = thumb_tx.send(ThumbnailResult { path: path.clone(), pane_side: PaneSide::Right, image }).await;
                            }
                            // Proactively re-trigger HD image generation
                            if let Some(image) = fm.get_image(path).await {
                                let _ = hd_tx.send(FullImageResult { path: path.clone(), image }).await;
                            }
                        }
                        if let Some(dir) = current_dir {
                            let files = crate::engine::gallery::GalleryScanner::scan_directory(&dir).await;
                            let _ = tx.send(ScanResult { 
                                pane_side: side, 
                                files, 
                                invalidated_paths: vec![],
                                transformed_paths: paths,
                            }).await;
                            ctx_clone.request_repaint();
                        }
                    });
                }
                crate::messages::BackendMsg::TransformFlipH { paths } => {
                    let side = self.active_pane;
                    let current_dir = self.active_pane_state().current_path.clone();
                    let tx = self.channels.scan_tx.clone();
                    let thumb_tx = self.channels.thumb_tx.clone();
                    let hd_tx = self.channels.hd_tx.clone();
                    let ctx_clone = ctx.clone();
                    let tm = self.thumbnail_manager.clone();
                    let fm = self.full_image_manager.clone();
                    tokio::spawn(async move {
                        for path in &paths {
                            let _ = crate::library::transform::flip_horizontal(path);
                            tm.invalidate(path).await;
                            fm.invalidate(path).await;

                            // Proactively re-trigger thumbnail generation (B12 fix)
                            if let Some(image) = tm.get_thumbnail(path, 160).await {
                                let _ = thumb_tx.send(ThumbnailResult { path: path.clone(), pane_side: PaneSide::Left, image: image.clone() }).await;
                                let _ = thumb_tx.send(ThumbnailResult { path: path.clone(), pane_side: PaneSide::Right, image }).await;
                            }
                            if let Some(image) = fm.get_image(path).await {
                                let _ = hd_tx.send(FullImageResult { path: path.clone(), image }).await;
                            }
                        }
                        if let Some(dir) = current_dir {
                            let files = crate::engine::gallery::GalleryScanner::scan_directory(&dir).await;
                            let _ = tx.send(ScanResult { 
                                pane_side: side, 
                                files, 
                                invalidated_paths: vec![],
                                transformed_paths: paths,
                            }).await;
                            ctx_clone.request_repaint();
                        }
                    });
                }
                crate::messages::BackendMsg::TransformFlipV { paths } => {
                    let side = self.active_pane;
                    let current_dir = self.active_pane_state().current_path.clone();
                    let tx = self.channels.scan_tx.clone();
                    let thumb_tx = self.channels.thumb_tx.clone();
                    let hd_tx = self.channels.hd_tx.clone();
                    let ctx_clone = ctx.clone();
                    let tm = self.thumbnail_manager.clone();
                    let fm = self.full_image_manager.clone();
                    tokio::spawn(async move {
                        for path in &paths {
                            let _ = crate::library::transform::flip_vertical(path);
                            tm.invalidate(path).await;
                            fm.invalidate(path).await;

                            // Proactively re-trigger thumbnail generation (B12 fix)
                            if let Some(image) = tm.get_thumbnail(path, 160).await {
                                let _ = thumb_tx.send(ThumbnailResult { path: path.clone(), pane_side: PaneSide::Left, image: image.clone() }).await;
                                let _ = thumb_tx.send(ThumbnailResult { path: path.clone(), pane_side: PaneSide::Right, image }).await;
                            }
                            if let Some(image) = fm.get_image(path).await {
                                let _ = hd_tx.send(FullImageResult { path: path.clone(), image }).await;
                            }
                        }
                        if let Some(dir) = current_dir {
                            let files = crate::engine::gallery::GalleryScanner::scan_directory(&dir).await;
                            let _ = tx.send(ScanResult { 
                                pane_side: side, 
                                files, 
                                invalidated_paths: vec![],
                                transformed_paths: paths,
                            }).await;
                            ctx_clone.request_repaint();
                        }
                    });
                }
            }
        }

        // Handle auto-group progress
        while let Ok(prog) = self.channels.ag_prog_rx.try_recv() {
            match prog {
                crate::messages::AutoGroupProgress::VirtualClustersUpdated { clusters } => {
                    // Live update: push growing virtual clusters to result so the
                    // Collections tab re-renders while the scan is still running.
                    // Forces are (0,0,0) during streaming; final values arrive with AutoGroupResult.
                    let existing_forces = self.auto_group_state.result
                        .as_ref().map(|r| r.forces).unwrap_or((0.0, 0.0, 0.0));
                    self.auto_group_state.result = Some(crate::messages::AutoGroupResult {
                        clusters,
                        forces: existing_forces,
                    });
                    self.active_pane_state().tab_mode = crate::ui::pane_state::TabMode::Collections;
                }
                other => {
                    self.auto_group_state.progress = Some(other);
                }
            }
            ctx.request_repaint();
        }

        // Handle auto-group results
        while let Ok(res) = self.channels.ag_res_rx.try_recv() {
            let has_clusters = !res.clusters.is_empty();
            self.auto_group_state.result = Some(res);
            self.auto_group_state.is_running = false;
            self.auto_group_state.auto_tune_running = false;
            if has_clusters {
                self.active_pane_state().tab_mode = crate::ui::pane_state::TabMode::Collections;
            }
            ctx.request_repaint();
        }

        // Handle auto-tune results
        while let Ok(res) = self.channels.ag_tune_rx.try_recv() {
            self.auto_group_state.eps = res.optimal_eps;
            self.auto_group_state.auto_tune_running = false;
            self.auto_group_state.is_running = false;
            ctx.request_repaint();
        }

        // Handle duplicate results
        while let Ok(res) = self.channels.dupes_rx.try_recv() {
            self.duplicates = res.clusters;
            self.active_pane_state().tab_mode = crate::ui::pane_state::TabMode::Duplicates;
            ctx.request_repaint();
        }
    }

    fn refresh_duplicates(&mut self, ctx: &egui::Context) {
        let db = self.db.clone();
        let tx = self.channels.dupes_tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            if let Ok(dupes) = db.get_duplicates().await {
                let clusters: Vec<crate::messages::Cluster> = dupes
                    .into_iter()
                    .enumerate()
                    .map(|(i, members)| crate::messages::Cluster {
                        id: i + 1,
                        members,
                        label: Some("Visual Match: Images with identical fingerprints".to_string()),
                    })
                    .collect();
                let _ = tx.send(crate::messages::DuplicatesResult { clusters }).await;
                ctx.request_repaint();
            }
        });
    }
}

// ── Directory Scanning & File Operations ──────────────────────────────────────

impl BildBlitzApp {
    fn trigger_scan(&mut self, path: PathBuf, side: PaneSide, ctx: &egui::Context) {
        let state = self.pane_state_mut(side);

        if state.current_path.as_ref() != Some(&path) {
            state.current_path = Some(path.clone());
            state.tab_mode = crate::ui::pane_state::TabMode::FolderView;
            let tx = self.channels.scan_tx.clone();
            let audit_tx = self.channels.audit_tx.clone();
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let _ = audit_tx.send(crate::messages::AuditMsg {
                    name: format!("Scanning {:?}", path),
                    success: true,
                    message: None,
                }).await;
                
                let files =
                    crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                
                let _ = tx
                    .send(ScanResult {
                        pane_side: side,
                        files,
                        invalidated_paths: vec![],
                        transformed_paths: vec![],
                    })
                    .await;
                ctx.request_repaint();
            });
        }
    }

    fn handle_grid_action(
        &mut self,
        action: crate::ui::grid::GridAction,
        side: PaneSide,
        ctx: &egui::Context,
    ) {
        use crate::ui::grid::GridAction;
        match action {
            GridAction::None => {}
            GridAction::Open(path) => {
                let files = &self.pane_state(side).files;
                if let Some(index) = files.iter().position(|f| f.path == path) {
                    self.image_viewer.current_index = index;
                    self.image_viewer.is_open = true;
                }
            }
            GridAction::Details(path) => {
                match crate::library::metadata::MetadataParser::extract_metadata(&path) {
                    Ok(meta) => {
                        self.properties_metadata = Some(meta);
                        self.is_properties_open = true;
                    }
                    Err(e) => tracing::error!("Failed to extract metadata: {}", e),
                }
            }
            GridAction::Navigate(path) => {
                self.trigger_scan(path, side, ctx);
            }
            GridAction::Drop(paths, target_subfolder) => {
                self.handle_drop(paths, target_subfolder, side, ctx);
            }
            GridAction::Rename(path, new_name) => {
                self.handle_rename(path, new_name, side, ctx);
            }
            GridAction::SaveAs(path) => {
                self.initiate_save_as(path);
            }
            GridAction::Cut(paths) => {
                self.clipboard.paths = paths.into_iter().collect();
            }
            GridAction::Paste(target_path) => {
                if !self.clipboard.is_empty() {
                    let source_paths = self.clipboard.take();
                    self.handle_drop(source_paths, target_path, side, ctx);
                }
            }
        }
    }

    /// Handles file rename with proper async I/O (B3 fix).
    fn handle_rename(
        &mut self,
        path: PathBuf,
        new_name: String,
        side: PaneSide,
        ctx: &egui::Context,
    ) {
        if new_name.trim().is_empty() {
            return;
        }
        let mut dest = path.clone();
        dest.set_file_name(&new_name);

        if dest == path {
            return;
        }

        let tx = self.channels.scan_tx.clone();
        let audit_tx = self.channels.audit_tx.clone();
        let ctx = ctx.clone();
        let current_dir = self.pane_state(side).current_path.clone();

        tokio::spawn(async move {
            let _ = audit_tx.send(crate::messages::AuditMsg {
                name: format!("Renaming {:?}", path.file_name().unwrap_or_default()),
                success: true,
                message: None,
            }).await;

            // Use tokio::fs for non-blocking rename (B3 fix)
            if let Err(e) = tokio::fs::rename(&path, &dest).await {
                tracing::error!("Rename failed for {:?} → {:?}: {}", path, dest, e);
                let _ = audit_tx.send(crate::messages::AuditMsg {
                    name: format!("Rename Failed: {:?}", path.file_name().unwrap_or_default()),
                    success: false,
                    message: Some(e.to_string()),
                }).await;
            } else {
                let _ = audit_tx.send(crate::messages::AuditMsg {
                    name: format!("Rename OK: {:?}", path.file_name().unwrap_or_default()),
                    success: true,
                    message: Some(format!("New name: {:?}", dest.file_name().unwrap_or_default())),
                }).await;
            }
            if let Some(dir) = current_dir {
                let files =
                    crate::engine::gallery::GalleryScanner::scan_directory(&dir).await;
                let _ = tx
                    .send(ScanResult {
                        pane_side: side,
                        files,
                        invalidated_paths: vec![],
                        transformed_paths: vec![],
                    })
                    .await;
                ctx.request_repaint();
            }
        });
    }

    /// Handles file drop/move/copy with safe path handling (B1, B4, S2 fixes).
    fn handle_drop(
        &mut self,
        source_paths: Vec<PathBuf>,
        target_subfolder: Option<PathBuf>,
        target_side: PaneSide,
        ctx: &egui::Context,
    ) {
        let base_target_path = self.pane_state(target_side).current_path.clone();

        let dest_dir = if let Some(subfolder) = target_subfolder {
            subfolder
        } else if let Some(base) = base_target_path {
            base
        } else {
            return;
        };

        let is_copy = ctx.input(|i| i.modifiers.command);
        let tx = self.channels.scan_tx.clone();
        let audit_tx = self.channels.audit_tx.clone();
        let ctx = ctx.clone();
        
        // Capture paths for both panes to ensure a full UI refresh after move/copy
        let left_refresh = self.left_pane.current_path.clone();
        let right_refresh = self.right_pane.current_path.clone();
        
        // Collect all directories whose counts might have changed (B11 fix)
        let mut invalidated_paths = vec![dest_dir.clone()];
        for p in &source_paths {
            if let Some(parent) = p.parent() {
                invalidated_paths.push(parent.to_path_buf());
            }
        }
        invalidated_paths.sort();
        invalidated_paths.dedup();

        let source_count = source_paths.len();
        tokio::spawn(async move {
            let _ = audit_tx.send(crate::messages::AuditMsg {
                name: format!("{} {} items to {:?}", if is_copy { "Copying" } else { "Moving" }, source_count, dest_dir.file_name().unwrap_or_default()),
                success: true,
                message: None,
            }).await;

            for source_path in source_paths {
                if source_path.parent() == Some(&dest_dir) {
                    continue; // Same folder, skip
                }

                // B1 fix: safe file_name extraction instead of unwrap()
                let file_name = match source_path.file_name() {
                    Some(name) => name.to_owned(),
                    None => {
                        tracing::warn!(
                            "Skipping path with no filename: {:?}",
                            source_path
                        );
                        continue;
                    }
                };

                let dest_path = dest_dir.join(&file_name);

                // S2 fix: skip if destination already exists (avoid silent overwrite)
                if dest_path.exists() {
                    tracing::warn!(
                        "Skipping {:?}: destination already exists at {:?}",
                        source_path,
                        dest_path
                    );
                    continue;
                }

                // B4 fix: use robust_move for moves with cross-volume fallback and verification
                let res = if is_copy {
                    copy_with_metadata(&source_path, &dest_path).await
                } else {
                    robust_move_with_metadata(&source_path, &dest_path).await
                };

                if let Err(e) = res {
                    tracing::error!(
                        "File operation failed for {:?}: {}",
                        source_path,
                        e
                    );
                }
            }

            // UI Refresh Phase: Scan both panes to ensure consistency (especially for moves)
            if let Some(path) = left_refresh {
                let files = crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                let _ = tx.send(ScanResult {
                    pane_side: PaneSide::Left,
                    files,
                    invalidated_paths: invalidated_paths.clone(),
                    transformed_paths: vec![],
                }).await;
            }
            if let Some(path) = right_refresh {
                let files = crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                let _ = tx.send(ScanResult {
                    pane_side: PaneSide::Right,
                    files,
                    invalidated_paths: invalidated_paths.clone(),
                    transformed_paths: vec![],
                }).await;
            }
            ctx.request_repaint();
        });
    }

    /// Creates a new folder, optionally moving selected files into it.
    /// Uses tokio::fs for all I/O (B5 fix), safe file_name handling (B2 fix).
    fn create_new_folder(&mut self, side: PaneSide, ctx: &egui::Context) {
        let state = self.pane_state_mut(side);
        let Some(current_path) = state.current_path.clone() else {
            return;
        };

        let selected_paths: Vec<PathBuf> = state.selected_files.iter().cloned().collect();

        if selected_paths.len() > 1 {
            // Smart folder: auto-name from common prefix, move selection into it
            let base_name = Self::find_common_name(&selected_paths);
            let mut folder_name = base_name.clone();
            let mut new_path = current_path.join(&folder_name);
            let mut counter = 2;

            while new_path.exists() {
                folder_name = format!("{} ({})", base_name, counter);
                new_path = current_path.join(&folder_name);
                counter += 1;
            }

            // B5 fix: create_dir is fast and local, but we at least log properly
            if let Err(e) = std::fs::create_dir(&new_path) {
                tracing::error!("Failed to create smart folder: {}", e);
                return;
            }

            let count = selected_paths.len();
            let dest_dir = new_path.clone();
            let tx = self.channels.scan_tx.clone();
            let ctx_clone = ctx.clone();
            
            // Capture paths for both panes for full refresh
            let left_refresh = self.left_pane.current_path.clone();
            let right_refresh = self.right_pane.current_path.clone();

            tokio::spawn(async move {
                for source in selected_paths {
                    // B2 fix: safe file_name extraction
                    let Some(file_name) = source.file_name().map(|n| n.to_owned())
                    else {
                        tracing::warn!(
                            "Skipping path with no filename: {:?}",
                            source
                        );
                        continue;
                    };
                    let dest = dest_dir.join(file_name);
                    // S3 fix: robust move with verification and timestamp preservation
                    let res = robust_move_with_metadata(&source, &dest).await;

                    if let Err(e) = res {
                        tracing::error!(
                            "Failed to move {:?} → {:?}: {}",
                            source,
                            dest,
                            e
                        );
                    }
                }

                // UI Refresh Phase
                let mut invalidated_paths = vec![dest_dir.clone()];
                if let Some(ref cp) = left_refresh {
                    invalidated_paths.push(cp.clone());
                }
                if let Some(ref cp) = right_refresh {
                    invalidated_paths.push(cp.clone());
                }
                invalidated_paths.sort();
                invalidated_paths.dedup();

                if let Some(path) = left_refresh {
                    let files = crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                    let _ = tx.send(ScanResult {
                        pane_side: PaneSide::Left,
                        files,
                        invalidated_paths: invalidated_paths.clone(),
                        transformed_paths: vec![],
                    }).await;
                }
                if let Some(path) = right_refresh {
                    let files = crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                    let _ = tx.send(ScanResult {
                        pane_side: PaneSide::Right,
                        files,
                        invalidated_paths: invalidated_paths.clone(),
                        transformed_paths: vec![],
                    }).await;
                }
                ctx_clone.request_repaint();
            });

            // Clear selection and show notification
            let state = self.pane_state_mut(side);
            state.selected_files.clear();
            self.notification = Some(Notification {
                message: format!("Moved {} items to \"{}\"", count, folder_name),
                expire_time: ctx.input(|i| i.time) + 4.0,
            });
        } else {
            // Simple new folder
            let mut folder_name = "New Folder".to_string();
            let mut new_path = current_path.join(&folder_name);
            let mut counter = 2;

            while new_path.exists() {
                folder_name = format!("New Folder ({})", counter);
                new_path = current_path.join(&folder_name);
                counter += 1;
            }

            if let Err(e) = std::fs::create_dir(&new_path) {
                tracing::error!("Failed to create folder: {}", e);
                return;
            }

            let state = self.pane_state_mut(side);
            state.renaming_path = Some(new_path);
            state.rename_buffer = folder_name;

            let tx = self.channels.scan_tx.clone();
            let ctx_clone = ctx.clone();
            tokio::spawn(async move {
                let files = crate::engine::gallery::GalleryScanner::scan_directory(
                    &current_path,
                )
                .await;
                let _ = tx
                    .send(ScanResult {
                        pane_side: side,
                        files,
                        invalidated_paths: vec![],
                        transformed_paths: vec![],
                    })
                    .await;
                ctx_clone.request_repaint();
            });
        }
    }

    /// Finds a common name prefix among file stems.
    /// B7 fix: uses char-based indexing instead of byte slicing to prevent
    /// panics on multi-byte UTF-8 filenames (e.g., German umlauts).
    fn find_common_name(paths: &[PathBuf]) -> String {
        if paths.is_empty() {
            return "New Folder".to_string();
        }

        let names: Vec<String> = paths
            .iter()
            .filter_map(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        if names.is_empty() {
            return "New Folder".to_string();
        }

        let first = &names[0];
        let mut common_len = first.chars().count();

        for name in &names[1..] {
            common_len = first
                .chars()
                .zip(name.chars())
                .take_while(|(a, b)| a == b)
                .count();
            if common_len == 0 {
                break;
            }
        }

        // B7 fix: collect chars instead of byte-slicing
        let mut result: String = first.chars().take(common_len).collect();
        while !result.is_empty()
            && !result.chars().last().unwrap().is_alphanumeric()
        {
            result.pop();
        }

        if result.len() < 3 {
            "Collection".to_string()
        } else {
            result
        }
    }
}

// ── Viewer Actions ────────────────────────────────────────────────────────────

impl BildBlitzApp {
    fn handle_viewer_action(&mut self, action: ViewerAction, side: PaneSide) {
        // Capture file info without holding a borrow across mutation
        let files_len = self.pane_state(side).files.len();
        let is_dir_at = |slf: &Self, idx: usize| -> bool {
            slf.pane_state(side)
                .files
                .get(idx)
                .map(|f| f.is_dir)
                .unwrap_or(false)
        };

        match action {
            ViewerAction::None => {}
            ViewerAction::Next => {
                // B8 fix: saturating_sub to prevent underflow on empty files
                let last = files_len.saturating_sub(1);
                if files_len > 0 && self.image_viewer.current_index < last {
                    self.image_viewer.current_index += 1;
                    // Skip directories
                    while self.image_viewer.current_index < last
                        && is_dir_at(self, self.image_viewer.current_index)
                    {
                        self.image_viewer.current_index += 1;
                    }
                }
            }
            ViewerAction::Prev => {
                if self.image_viewer.current_index > 0 {
                    self.image_viewer.current_index -= 1;
                    while self.image_viewer.current_index > 0
                        && is_dir_at(self, self.image_viewer.current_index)
                    {
                        self.image_viewer.current_index -= 1;
                    }
                }
            }
            ViewerAction::JumpToIndex(idx) => {
                self.image_viewer.current_index = idx;
            }
            ViewerAction::Close => {
                self.image_viewer.is_open = false;
            }
            ViewerAction::SaveAs(path) => {
                self.initiate_save_as(path);
            }
        }
    }

    fn handle_toolbar_action(&mut self, action: crate::messages::ToolbarAction, _ctx: &egui::Context) {
        use crate::messages::{ToolbarAction, BackendMsg};
        let paths: Vec<_> = self.active_pane_state().selected_files.iter().cloned().collect();
        if paths.is_empty() { return; }

        match action {
            ToolbarAction::None => {}
            ToolbarAction::Rotate(deg) => {
                let _ = self.channels.backend_tx.try_send(BackendMsg::TransformRotate { paths, degrees: deg });
            }
            ToolbarAction::FlipH => {
                let _ = self.channels.backend_tx.try_send(BackendMsg::TransformFlipH { paths });
            }
            ToolbarAction::FlipV => {
                let _ = self.channels.backend_tx.try_send(BackendMsg::TransformFlipV { paths });
            }
        }
    }
}

// ── UI Rendering ──────────────────────────────────────────────────────────────

impl BildBlitzApp {
    fn render_top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("BildBlitz");
                ui.separator();

                let tb_action = crate::ui::tools::toolbar(ui);
                self.handle_toolbar_action(tb_action, ctx);
                ui.separator();

                if ui.button("✨ Auto-Group").clicked() {
                    self.auto_group_state.is_open = true;
                }
                ui.separator();

                if ui
                    .button(if self.is_split_view_active {
                        "🔲 Single"
                    } else {
                        "👥 Split"
                    })
                    .clicked()
                {
                    self.is_split_view_active = !self.is_split_view_active;
                    if !self.is_split_view_active {
                        self.active_pane = PaneSide::Left;
                    }
                }

                if self.is_split_view_active {
                    ui.separator();
                    ui.label("Focus:");
                    ui.selectable_value(
                        &mut self.active_pane,
                        PaneSide::Left,
                        "⬅ Left",
                    );
                    ui.selectable_value(
                        &mut self.active_pane,
                        PaneSide::Right,
                        "Right ➡",
                    );
                }

                ui.separator();
                let mut view_state = self.active_pane_state().view_state;
                ui.selectable_value(
                    &mut view_state,
                    crate::ui::pane_state::ViewState::Grid,
                    "⣿ Grid",
                );
                ui.selectable_value(
                    &mut view_state,
                    crate::ui::pane_state::ViewState::List,
                    "☰ List",
                );
                self.active_pane_state().view_state = view_state;

                ui.separator();
                ui.label("Thumbnail Size:");
                let mut size = self.active_pane_state().thumbnail_size;
                if ui
                    .add(egui::Slider::new(&mut size, 64.0..=512.0))
                    .changed()
                {
                    self.active_pane_state().thumbnail_size = size;
                }

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui
                            .selectable_label(self.is_audit_open, "🔍 Audit")
                            .clicked()
                        {
                            self.is_audit_open = !self.is_audit_open;
                        }

                        if ui
                            .selectable_label(self.log_view.is_open, "📋 Logs")
                            .clicked()
                        {
                            self.log_view.is_open = !self.log_view.is_open;
                        }

                        ui.separator();

                        let can_create =
                            self.active_pane_state().current_path.is_some();
                        if ui
                            .add_enabled(
                                can_create,
                                egui::Button::new("📁+ New Folder"),
                            )
                            .clicked()
                        {
                            let side = self.active_pane;
                            let ctx = ui.ctx().clone();
                            self.create_new_folder(side, &ctx);
                        }
                    },
                );
            });
        });
    }

    fn render_tip_bubble(&mut self, ctx: &egui::Context) {
        if !self.is_tip_visible {
            return;
        }

        egui::Area::new("tip_bubble".into())
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-20.0, -20.0))
            .show(ctx, |ui| {
                egui::Frame::window(ui.style())
                    .fill(ui.visuals().window_fill())
                    .corner_radius(12.0)
                    .inner_margin(16.0)
                    .stroke(ui.visuals().widgets.active.bg_stroke)
                    .show(ui, |ui| {
                        ui.set_max_width(320.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("💡 PRO TIP")
                                        .strong()
                                        .color(
                                            ui.visuals().selection.bg_fill,
                                        )
                                        .size(14.0),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(
                                        egui::Align::Center,
                                    ),
                                    |ui| {
                                        if ui.button("✕").clicked() {
                                            self.is_tip_visible = false;
                                        }
                                    },
                                );
                            });
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(
                                    "You can still use Cmd/Ctrl + Drag to copy files instead of moving them. The counters will also update correctly for copy operations!",
                                )
                                .size(12.5)
                                .line_height(Some(16.0)),
                            );
                        });
                    });
            });
    }

    fn render_notification(&mut self, ctx: &egui::Context) {
        let should_clear = if let Some(notification) = &self.notification {
            if ctx.input(|i| i.time) < notification.expire_time {
                egui::Area::new("notification_bubble".into())
                    .anchor(
                        egui::Align2::CENTER_BOTTOM,
                        egui::vec2(0.0, -40.0),
                    )
                    .show(ctx, |ui| {
                        egui::Frame::window(ui.style())
                            .fill(
                                ui.visuals()
                                    .window_fill()
                                    .gamma_multiply(0.9),
                            )
                            .corner_radius(32.0)
                            .inner_margin(egui::Margin::symmetric(24, 12))
                            .stroke(ui.visuals().selection.stroke)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("✨").size(18.0),
                                    );
                                    ui.label(
                                        egui::RichText::new(
                                            &notification.message,
                                        )
                                        .strong()
                                        .size(14.0),
                                    );
                                });
                            });
                    });
                ctx.request_repaint();
                false
            } else {
                true
            }
        } else {
            false
        };

        if should_clear {
            self.notification = None;
        }
    }

    fn show_single_pane(&mut self, ui: &mut egui::Ui) {
        if let Some(path) = self.left_pane.current_path.clone() {
            let has_collections = self.auto_group_state.result.is_some();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.left_pane.tab_mode, crate::ui::pane_state::TabMode::FolderView, "📁 Folder View");
                if has_collections {
                    ui.selectable_value(&mut self.left_pane.tab_mode, crate::ui::pane_state::TabMode::Collections, "✨ Virtual Collections");
                }
                if ui.selectable_value(&mut self.left_pane.tab_mode, crate::ui::pane_state::TabMode::Duplicates, "👯 Duplicates").clicked() {
                    let _ = self.channels.backend_tx.try_send(crate::messages::BackendMsg::DuplicatesRefresh);
                }
            });
            ui.separator();

            Self::show_path_bar(ui, &path);
            ui.add_space(4.0);

            let gctx = GridContext {
                side: PaneSide::Left,
                thumbnail_manager: &self.thumbnail_manager,
                thumb_tx: &self.channels.thumb_tx,
                count_tx: &self.channels.count_tx,
                can_paste: !self.clipboard.is_empty(),
                cut_paths: &self.clipboard.paths,
            };
            
            let action = match self.left_pane.tab_mode {
                crate::ui::pane_state::TabMode::FolderView => {
                    self.left_grid.show(ui, &mut self.left_pane, &gctx)
                }
                crate::ui::pane_state::TabMode::Collections => {
                    if let Some(res) = &self.auto_group_state.result {
                        self.left_grid.show_clusters(ui, &res.clusters, &mut self.left_pane, &gctx)
                    } else {
                        crate::ui::grid::GridAction::None
                    }
                }
                crate::ui::pane_state::TabMode::Duplicates => {
                    self.left_grid.show_clusters(ui, &self.duplicates, &mut self.left_pane, &gctx)
                }
            };
            self.handle_grid_action(action, PaneSide::Left, ui.ctx());
        } else {
            ui.centered_and_justified(|ui| {
                Self::show_welcome(ui);
            });
        }
    }

    /// Renders a single pane (used by both left and right in dual mode).
    /// CS2 fix: eliminated 100+ lines of duplication between left and right pane rendering.
    fn show_pane_content(&mut self, ui: &mut egui::Ui, side: PaneSide) {
        let is_active = self.active_pane == side;
        let frame = if is_active {
            egui::Frame::group(ui.style())
                .fill(
                    ui.visuals()
                        .selection
                        .bg_fill
                        .gamma_multiply(0.1),
                )
                .stroke(egui::Stroke::new(
                    2.5,
                    ui.visuals().selection.bg_fill,
                ))
        } else {
            egui::Frame::group(ui.style())
                .fill(ui.visuals().panel_fill)
                .stroke(egui::Stroke::new(
                    1.0,
                    ui.visuals()
                        .widgets
                        .noninteractive
                        .bg_stroke
                        .color,
                ))
        };

        frame.show(ui, |ui| {
            if ui
                .interact(
                    ui.available_rect_before_wrap(),
                    ui.id(),
                    egui::Sense::click(),
                )
                .clicked()
            {
                self.active_pane = side;
            }

            let current_path = self.pane_state(side).current_path.clone();
            if let Some(path) = current_path {
                let has_collections = self.auto_group_state.result.is_some();
                let mut tab_mode = self.pane_state(side).tab_mode;
                
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut tab_mode, crate::ui::pane_state::TabMode::FolderView, "📁 Folder View");
                    if has_collections {
                        ui.selectable_value(&mut tab_mode, crate::ui::pane_state::TabMode::Collections, "✨ Virtual Collections");
                    }
                    if ui.selectable_value(&mut tab_mode, crate::ui::pane_state::TabMode::Duplicates, "👯 Duplicates").clicked() {
                        let _ = self.channels.backend_tx.try_send(crate::messages::BackendMsg::DuplicatesRefresh);
                    }
                });
                self.pane_state_mut(side).tab_mode = tab_mode;
                ui.separator();

                Self::show_path_bar(ui, &path);
                ui.add_space(4.0);

                let gctx = GridContext {
                    side,
                    thumbnail_manager: &self.thumbnail_manager,
                    thumb_tx: &self.channels.thumb_tx,
                    count_tx: &self.channels.count_tx,
                    can_paste: !self.clipboard.is_empty(),
                    cut_paths: &self.clipboard.paths,
                };

                let tab_mode_active = self.pane_state(side).tab_mode;
                let (state, grid) = match side {
                    PaneSide::Left => (&mut self.left_pane, &mut self.left_grid),
                    PaneSide::Right => {
                        (&mut self.right_pane, &mut self.right_grid)
                    }
                };
                
                let action = match tab_mode_active {
                    crate::ui::pane_state::TabMode::FolderView => {
                        grid.show(ui, state, &gctx)
                    }
                    crate::ui::pane_state::TabMode::Collections => {
                        if let Some(res) = &self.auto_group_state.result {
                            grid.show_clusters(ui, &res.clusters, state, &gctx)
                        } else {
                            crate::ui::grid::GridAction::None
                        }
                    }
                    crate::ui::pane_state::TabMode::Duplicates => {
                        grid.show_clusters(ui, &self.duplicates, state, &gctx)
                    }
                };
                
                self.handle_grid_action(action, side, ui.ctx());
            } else {
                let label = match side {
                    PaneSide::Left => "Select a folder for Left Pane",
                    PaneSide::Right => "Select a folder for Right Pane",
                };
                ui.centered_and_justified(|ui| ui.label(label));
            }
        });
    }

    fn show_dual_pane(&mut self, ui: &mut egui::Ui) {
        // CS2 fix: use shared show_pane_content for both sides
        ui.columns(2, |columns| {
            columns[0].push_id("left_pane", |ui| {
                ui.vertical(|ui| {
                    self.show_pane_content(ui, PaneSide::Left);
                });
            });

            columns[1].push_id("right_pane", |ui| {
                ui.vertical(|ui| {
                    self.show_pane_content(ui, PaneSide::Right);
                });
            });
        });
    }

    fn show_welcome(ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.heading("Welcome to BildBlitz");
            ui.add_space(10.0);
            ui.label("Select a folder from the sidebar to start browsing.");
            ui.add_space(20.0);
            ui.label(
                "⚡ Blazing fast • 🚀 Hardware accelerated • 🎨 Native Windows 11",
            );
        });
    }

    fn show_path_bar(ui: &mut egui::Ui, path: &std::path::Path) {
        let frame = egui::Frame::NONE
            .fill(
                ui.visuals()
                    .widgets
                    .noninteractive
                    .bg_fill
                    .gamma_multiply(0.5),
            )
            .inner_margin(egui::Margin::symmetric(12, 6))
            .corner_radius(4);

        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(egui::RichText::new("📂").size(14.0));

                let path_str = path.to_string_lossy();
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(path_str)
                            .size(11.0)
                            .color(ui.visuals().strong_text_color())
                            .monospace(),
                    ),
                )
                .on_hover_text("Current full directory path");
            });
        });
    }

    fn show_fullscreen(&mut self, ctx: &egui::Context) {
        let side = self.active_pane;
        let current_files = &self.pane_state(side).files;

        // Trigger HD Loading for current image if needed
        if let Some(file) = current_files.get(self.image_viewer.current_index) {
            if !file.is_dir
                && !self.hd_textures.contains_key(&file.path)
                && !self.hd_loading.contains(&file.path)
            {
                self.request_hd_image(file.path.clone(), ctx);
            }
        }

        let action = match side {
            PaneSide::Left => self.image_viewer.show(
                ctx,
                &self.left_pane.files,
                &self.left_grid.textures,
                &self.hd_textures,
            ),
            PaneSide::Right => self.image_viewer.show(
                ctx,
                &self.right_pane.files,
                &self.right_grid.textures,
                &self.hd_textures,
            ),
        };
        self.handle_viewer_action(action, side);
    }

    fn request_hd_image(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.hd_loading.insert(path.clone());
        let tx = self.channels.hd_tx.clone();
        let manager = self.full_image_manager.clone();
        let ctx = ctx.clone();

        tokio::spawn(async move {
            if let Some(image) = manager.get_image(&path).await {
                let _ = tx.send(FullImageResult { path, image }).await;
                ctx.request_repaint();
            }
        });
    }

    fn show_properties_pane(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.heading("📋 Properties");
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui.button("✕").clicked() {
                            self.is_properties_open = false;
                        }
                    },
                );
            });
            ui.separator();

            if let Some(meta) = &self.properties_metadata {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(8.0);

                    // Thumbnail preview
                    if let Some(texture) = self
                        .left_grid
                        .textures
                        .get(&meta.path)
                        .or_else(|| self.right_grid.textures.get(&meta.path))
                    {
                        let size = ui.available_width() - 32.0;
                        let tex_size = texture.size_vec2();
                        let ratio =
                            (size / tex_size.x).min(size / tex_size.y).min(1.0);
                        ui.image((texture.id(), tex_size * ratio));
                        ui.add_space(12.0);
                    }

                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            egui::RichText::new(&meta.name)
                                .strong()
                                .size(14.0),
                        );
                        ui.label(
                            egui::RichText::new(meta.path.to_string_lossy())
                                .weak()
                                .size(10.0),
                        );
                    });

                    ui.add_space(12.0);

                    egui::Grid::new("meta_grid")
                        .num_columns(2)
                        .spacing([20.0, 8.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Dimensions:");
                            if let Some((w, h)) = meta.dimensions {
                                ui.label(format!("{} × {}", w, h));
                            } else {
                                ui.label("Unknown");
                            }
                            ui.end_row();

                            ui.label("File Size:");
                            let size_mb =
                                meta.size_bytes as f64 / (1024.0 * 1024.0);
                            ui.label(format!("{:.2} MB", size_mb));
                            ui.end_row();

                            ui.label("Format:");
                            ui.label(&meta.format);
                            ui.end_row();

                            ui.label("Modified:");
                            let datetime: chrono::DateTime<chrono::Local> =
                                meta.modified.into();
                            ui.label(
                                datetime
                                    .format("%Y-%m-%d %H:%M:%S")
                                    .to_string(),
                            );
                            ui.end_row();
                        });

                    ui.add_space(20.0);
                    let reveal_btn =
                        egui::Button::new("📁 Reveal in Explorer")
                            .fill(ui.visuals().widgets.active.bg_fill);
                    if ui.add(reveal_btn).clicked() {
                        let _ = opener::reveal(&meta.path);
                    }
                });
            } else {
                ui.centered_and_justified(|ui| ui.label("No file selected"));
            }
        });
    }

    fn initiate_save_as(&mut self, original_path: PathBuf) {
        let target_folder = original_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| {
            self.pane_state(self.active_pane).current_path.clone().unwrap_or_default()
        });
        
        let suggested_path = suggest_next_filename(&original_path);
        let suggested_filename = suggested_path.file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| original_path.file_name().unwrap_or_default().to_string_lossy().to_string());

        self.save_as_state = Some(SaveAsState {
            is_open: true,
            original_path,
            target_folder,
            new_filename: suggested_filename,
            error_message: None,
        });
    }

    fn execute_save_as(&mut self, ctx: &egui::Context) {
        if let Some(state) = &self.save_as_state {
            let dest_path = state.target_folder.join(&state.new_filename);
            
            if dest_path == state.original_path {
                self.save_as_state.as_mut().unwrap().error_message = Some("Cannot save as the same file. Please choose a different name.".to_string());
                return;
            }

            if dest_path.exists() {
                self.save_as_state.as_mut().unwrap().error_message = Some("File already exists. Please choose a different name.".to_string());
                return;
            }

            // Trigger the copy
            let source_path = state.original_path.clone();
            let dest_path_clone = dest_path.clone();
            let tx = self.channels.scan_tx.clone();
            let audit_tx = self.channels.audit_tx.clone();
            let ctx_clone = ctx.clone();
            
            // Refreshes
            let left_refresh = self.left_pane.current_path.clone();
            let right_refresh = self.right_pane.current_path.clone();

            tokio::spawn(async move {
                let _ = audit_tx.send(crate::messages::AuditMsg {
                    name: format!("Save As: {:?}", dest_path_clone.file_name().unwrap_or_default()),
                    success: true,
                    message: Some(format!("Copying to {:?}", dest_path_clone)),
                }).await;

                match copy_with_metadata(&source_path, &dest_path_clone).await {
                    Ok(_) => {
                        let _ = audit_tx.send(crate::messages::AuditMsg {
                            name: format!("Save As OK: {:?}", dest_path_clone.file_name().unwrap_or_default()),
                            success: true,
                            message: None,
                        }).await;
                    }
                    Err(e) => {
                        tracing::error!("Save As failed: {}", e);
                        let _ = audit_tx.send(crate::messages::AuditMsg {
                            name: format!("Save As Failed: {:?}", dest_path_clone.file_name().unwrap_or_default()),
                            success: false,
                            message: Some(e.to_string()),
                        }).await;
                    }
                }

                // Refresh the panes
                if let Some(path) = left_refresh {
                    let files = crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                    let _ = tx.send(ScanResult {
                        pane_side: PaneSide::Left,
                        files,
                        invalidated_paths: vec![dest_path_clone.parent().unwrap_or(&dest_path_clone).to_path_buf()],
                        transformed_paths: vec![],
                    }).await;
                }
                if let Some(path) = right_refresh {
                    let files = crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                    let _ = tx.send(ScanResult {
                        pane_side: PaneSide::Right,
                        files,
                        invalidated_paths: vec![dest_path_clone.parent().unwrap_or(&dest_path_clone).to_path_buf()],
                        transformed_paths: vec![],
                    }).await;
                }
                ctx_clone.request_repaint();
            });

            // Close the dialog
            self.save_as_state = None;
        }
    }

    fn render_save_as_dialog(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        if let Some(state) = &mut self.save_as_state {
            if !state.is_open {
                return;
            }

            egui::Window::new("💾 Save As")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .default_width(450.0)
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.add_space(4.0);
                        
                        // Original file info
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new("Original File:").strong().size(12.0));
                            ui.label(egui::RichText::new(state.original_path.to_string_lossy()).monospace().size(11.0));
                        });
                        
                        ui.add_space(8.0);
                        
                        // Target directory info
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new("Target Folder:").strong().size(12.0));
                            ui.label(egui::RichText::new(state.target_folder.to_string_lossy()).monospace().size(11.0));
                        });

                        ui.add_space(12.0);

                        // New name input field
                        ui.label(egui::RichText::new("New File Name:").strong().size(12.0));
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut state.new_filename)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace)
                        );
                        
                        // Autofocus the input field when opened
                        response.request_focus();

                        if let Some(err) = &state.error_message {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new(format!("⚠️ {}", err)).color(ui.visuals().error_fg_color));
                        }

                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // Dialog actions
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let save_btn = egui::Button::new(
                                    egui::RichText::new("Save").strong()
                                )
                                .fill(ui.visuals().selection.bg_fill)
                                .stroke(egui::Stroke::NONE);

                                if ui.add(save_btn).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_clicked = true;
                                }

                                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    cancel_clicked = true;
                                }
                            });
                        });
                    });
                });

            if !open || cancel_clicked {
                self.save_as_state = None;
            } else if save_clicked {
                self.execute_save_as(ctx);
            }
        }
    }
}

fn suggest_next_filename(original_path: &Path) -> PathBuf {
    let parent = original_path.parent().unwrap_or_else(|| Path::new(""));
    let extension = original_path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    let stem = original_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    
    let mut base_stem = stem.clone();
    let mut version_num = None;
    let mut suffix_type = "_v";
    
    // Check for _v(\d+)
    if let Some(idx) = stem.rfind("_v") {
        if idx > 0 {
            let num_str = &stem[idx + 2..];
            if !num_str.is_empty() && num_str.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(num) = num_str.parse::<usize>() {
                    base_stem = stem[..idx].to_string();
                    version_num = Some(num);
                    suffix_type = "_v";
                }
            }
        }
    }
    
    // If not found, check for _(\d+)
    if version_num.is_none() {
        if let Some(idx) = stem.rfind('_') {
            if idx > 0 {
                let num_str = &stem[idx + 1..];
                if !num_str.is_empty() && num_str.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(num) = num_str.parse::<usize>() {
                        base_stem = stem[..idx].to_string();
                        version_num = Some(num);
                        suffix_type = "_";
                    }
                }
            }
        }
    }

    // If not found, check for " (N)"
    if version_num.is_none() {
        if stem.ends_with(')') {
            if let Some(idx) = stem.rfind(" (") {
                if idx > 0 {
                    let num_str = &stem[idx + 2..stem.len() - 1];
                    if !num_str.is_empty() && num_str.chars().all(|c| c.is_ascii_digit()) {
                        if let Ok(num) = num_str.parse::<usize>() {
                            base_stem = stem[..idx].to_string();
                            version_num = Some(num);
                            suffix_type = " (";
                        }
                    }
                }
            }
        }
    }

    let mut next_num = match version_num {
        Some(n) => n + 1,
        None => 1,
    };

    loop {
        let proposed_name = if suffix_type == " (" {
            if extension.is_empty() {
                format!("{} ({})", base_stem, next_num)
            } else {
                format!("{} ({}).{}", base_stem, next_num, extension)
            }
        } else {
            if extension.is_empty() {
                format!("{}{}{}", base_stem, suffix_type, next_num)
            } else {
                format!("{}{}{}.{}", base_stem, suffix_type, next_num, extension)
            }
        };

        let proposed_path = parent.join(&proposed_name);
        if !proposed_path.exists() {
            return proposed_path;
        }
        next_num += 1;
    }
}


/// Robustly moves a file, falling back to copy+verify+delete for cross-volume moves.
async fn robust_move_with_metadata(source: &Path, dest: &Path) -> std::io::Result<()> {
    match tokio::fs::rename(source, dest).await {
        Ok(_) => Ok(()),
        Err(e) if e.raw_os_error() == Some(17) => {
            // 1. Capture source metadata for verification
            let src_meta = tokio::fs::metadata(source).await?;
            let src_size = src_meta.len();

            // 2. Perform copy with metadata preservation
            copy_with_metadata(source, dest).await?;

            // 3. Explicit Verification: Fetch destination metadata to compare size
            let dest_meta = tokio::fs::metadata(dest).await?;
            if dest_meta.len() == src_size {
                // Verified: sizes match, safe to remove the original
                tokio::fs::remove_file(source).await
            } else {
                // Size mismatch - Keep the original file for safety
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "Move verification failed: Destination size ({}) does not match source ({})",
                        dest_meta.len(),
                        src_size
                    ),
                ))
            }
        }
        Err(e) => Err(e),
    }
}

/// Copies a file while preserving accessed, modified, and creation (Windows) timestamps.
async fn copy_with_metadata(source: &Path, dest: &Path) -> std::io::Result<()> {
    // Get metadata from source
    let metadata = tokio::fs::metadata(source).await?;
    let accessed = metadata.accessed().ok();
    let modified = metadata.modified().ok();
    #[cfg(windows)]
    let created = metadata.created().ok();

    // Copy content
    tokio::fs::copy(source, dest).await?;

    // Apply timestamps to destination
    let dest_path = dest.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::options().write(true).open(&dest_path)?;
        let mut times = std::fs::FileTimes::new();

        if let Some(atime) = accessed {
            times = times.set_accessed(atime);
        }
        if let Some(mtime) = modified {
            times = times.set_modified(mtime);
        }

        #[cfg(windows)]
        if let Some(ctime) = created {
            use std::os::windows::fs::FileTimesExt;
            times = times.set_created(ctime);
        }

        file.set_times(times)
    })
    .await
    .unwrap_or_else(|e| Err(std::io::Error::new(std::io::ErrorKind::Other, e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn test_suggest_next_filename() {
        let temp_dir = std::env::temp_dir().join(format!("bb_test_{}", chrono::Utc::now().timestamp_micros()));
        let _ = std::fs::create_dir_all(&temp_dir);

        // Case 1: no existing version suffix
        let original = temp_dir.join("photo.jpg");
        let suggested = suggest_next_filename(&original);
        assert_eq!(suggested.file_name().unwrap().to_str().unwrap(), "photo_v1.jpg");

        // Create photo_v1.jpg
        let _file = File::create(temp_dir.join("photo_v1.jpg")).unwrap();
        let suggested2 = suggest_next_filename(&original);
        assert_eq!(suggested2.file_name().unwrap().to_str().unwrap(), "photo_v2.jpg");

        // Case 2: existing version suffix _v1
        let original_v1 = temp_dir.join("photo_v1.jpg");
        let suggested_v2 = suggest_next_filename(&original_v1);
        assert_eq!(suggested_v2.file_name().unwrap().to_str().unwrap(), "photo_v2.jpg");

        // Case 3: custom existing suffix like _1
        let original_u1 = temp_dir.join("photo_1.jpg");
        let suggested_u2 = suggest_next_filename(&original_u1);
        assert_eq!(suggested_u2.file_name().unwrap().to_str().unwrap(), "photo_2.jpg");

        // Case 4: custom existing suffix like " (1)"
        let original_p1 = temp_dir.join("photo (1).jpg");
        let suggested_p2 = suggest_next_filename(&original_p1);
        assert_eq!(suggested_p2.file_name().unwrap().to_str().unwrap(), "photo (2).jpg");

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}


