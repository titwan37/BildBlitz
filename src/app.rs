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
}

impl ChannelHub {
    fn new() -> Self {
        let (thumb_tx, thumb_rx) = mpsc::channel(100);
        let (scan_tx, scan_rx) = mpsc::channel(10);
        let (count_tx, count_rx) = mpsc::channel(100);
        let (hd_tx, hd_rx) = mpsc::channel(10);
        Self {
            thumb_tx,
            thumb_rx,
            scan_tx,
            scan_rx,
            count_tx,
            count_rx,
            hd_tx,
            hd_rx,
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
}

impl BildBlitzApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        log_stream: LogStream,
        log_dir: PathBuf,
    ) -> Self {
        let config = crate::library::config::load_config();

        Self {
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
        }
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.log_view.update();
        self.process_keyboard_input(ctx);
        self.drain_channels(ctx);
        self.render_top_panel(ctx);
        self.render_tip_bubble(ctx);
        self.render_notification(ctx);

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
            if let Some(path) = result.invalidated_path {
                self.left_grid.folder_counts.remove(&path);
                self.left_grid.loading_counts.remove(&path);
                self.right_grid.folder_counts.remove(&path);
                self.right_grid.loading_counts.remove(&path);
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
    }
}

// ── Directory Scanning & File Operations ──────────────────────────────────────

impl BildBlitzApp {
    fn trigger_scan(&mut self, path: PathBuf, side: PaneSide, ctx: &egui::Context) {
        let state = self.pane_state_mut(side);

        if state.current_path.as_ref() != Some(&path) {
            state.current_path = Some(path.clone());
            let tx = self.channels.scan_tx.clone();
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let files =
                    crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                let _ = tx
                    .send(ScanResult {
                        pane_side: side,
                        files,
                        invalidated_path: None,
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
        let ctx = ctx.clone();
        let current_dir = self.pane_state(side).current_path.clone();

        tokio::spawn(async move {
            // Use tokio::fs for non-blocking rename (B3 fix)
            if let Err(e) = tokio::fs::rename(&path, &dest).await {
                tracing::error!("Rename failed for {:?} → {:?}: {}", path, dest, e);
            }
            if let Some(dir) = current_dir {
                let files =
                    crate::engine::gallery::GalleryScanner::scan_directory(&dir).await;
                let _ = tx
                    .send(ScanResult {
                        pane_side: side,
                        files,
                        invalidated_path: None,
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
        let ctx = ctx.clone();
        
        // Capture paths for both panes to ensure a full UI refresh after move/copy
        let left_refresh = self.left_pane.current_path.clone();
        let right_refresh = self.right_pane.current_path.clone();
        
        // Use dest_dir specifically for subfolder count invalidation
        let invalidated_path = Some(dest_dir.clone());

        tokio::spawn(async move {
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
                    invalidated_path: if target_side == PaneSide::Left { invalidated_path.clone() } else { None },
                }).await;
            }
            if let Some(path) = right_refresh {
                let files = crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                let _ = tx.send(ScanResult {
                    pane_side: PaneSide::Right,
                    files,
                    invalidated_path: if target_side == PaneSide::Right { invalidated_path } else { None },
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
                if let Some(path) = left_refresh {
                    let files = crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                    let _ = tx.send(ScanResult {
                        pane_side: PaneSide::Left,
                        files,
                        invalidated_path: if side == PaneSide::Left { Some(dest_dir.clone()) } else { None },
                    }).await;
                }
                if let Some(path) = right_refresh {
                    let files = crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                    let _ = tx.send(ScanResult {
                        pane_side: PaneSide::Right,
                        files,
                        invalidated_path: if side == PaneSide::Right { Some(dest_dir) } else { None },
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
                        invalidated_path: None,
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

                crate::ui::tools::toolbar(ui);
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
            let action =
                self.left_grid.show(ui, &mut self.left_pane, &gctx);
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

                let (state, grid) = match side {
                    PaneSide::Left => (&mut self.left_pane, &mut self.left_grid),
                    PaneSide::Right => {
                        (&mut self.right_pane, &mut self.right_grid)
                    }
                };
                let action = grid.show(ui, state, &gctx);
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

