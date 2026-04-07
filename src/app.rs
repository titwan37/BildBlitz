use crate::ui::navigation::NavigationPane;
use crate::ui::pane_state::{PaneState, PaneSide};
use crate::engine::gallery::{ThumbnailManager, FileInfo};
use crate::ui::grid::GridView;
use crate::ui::viewer::{ImageViewer, ViewerAction};
use crate::ui::log_view::LogView;
use crate::library::metadata::ImageMetadata;
use crate::os::logging::LogStream;
use crate::engine::gallery::FullImageManager;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct ThumbnailResult {
    pub path: PathBuf,
    pub image: Arc<egui::ColorImage>,
}

pub struct ScanResult {
    pub pane_side: PaneSide,
    pub files: Vec<FileInfo>,
    pub invalidated_path: Option<PathBuf>,
}

pub struct FolderCountResult {
    pub path: PathBuf,
    pub count: usize,
}

pub struct FullImageResult {
    pub path: PathBuf,
    pub image: Arc<egui::ColorImage>,
}

struct Notification {
    message: String,
    expire_time: f64,
}

pub struct BildBlitzApp {
    pub navigation: NavigationPane,
    pub is_split_view_active: bool,
    pub active_pane: PaneSide,
    pub left_pane: PaneState,
    pub right_pane: PaneState,
    pub left_grid: GridView,
    pub right_grid: GridView,
    pub thumbnail_manager: ThumbnailManager,
    pub thumb_tx: mpsc::Sender<ThumbnailResult>,
    pub thumb_rx: mpsc::Receiver<ThumbnailResult>,
    pub scan_tx: mpsc::Sender<ScanResult>,
    pub scan_rx: mpsc::Receiver<ScanResult>,
    pub count_tx: mpsc::Sender<FolderCountResult>,
    pub count_rx: mpsc::Receiver<FolderCountResult>,
    pub log_view: LogView,
    pub image_viewer: ImageViewer,
    pub properties_metadata: Option<ImageMetadata>,
    pub is_properties_open: bool,
    pub full_image_manager: crate::engine::gallery::FullImageManager,
    pub hd_textures: std::collections::HashMap<PathBuf, egui::TextureHandle>,
    pub hd_loading: std::collections::HashSet<PathBuf>,
    pub hd_tx: mpsc::Sender<FullImageResult>,
    pub hd_rx: mpsc::Receiver<FullImageResult>,
    pub is_tip_visible: bool,
    pub clipboard: Vec<PathBuf>,
    notification: Option<Notification>,
}

impl BildBlitzApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, log_stream: LogStream, log_dir: PathBuf) -> Self {
        let (thumb_tx, thumb_rx) = mpsc::channel(100);
        let (scan_tx, scan_rx) = mpsc::channel(10);
        let (count_tx, count_rx) = mpsc::channel(100);
        let config = crate::library::config::load_config();
        
        let (hd_tx, hd_rx) = mpsc::channel(10);

        Self {
            navigation: NavigationPane::new(config.favorites),
            is_split_view_active: false,
            active_pane: PaneSide::Left,
            left_pane: PaneState::new(),
            right_pane: PaneState::new(),
            left_grid: GridView::new(),
            right_grid: GridView::new(),
            thumbnail_manager: ThumbnailManager::new(),
            thumb_tx,
            thumb_rx,
            scan_tx,
            scan_rx,
            count_tx,
            count_rx,
            log_view: LogView::new(log_stream, log_dir),
            image_viewer: ImageViewer::new(),
            properties_metadata: None,
            is_properties_open: false,
            full_image_manager: FullImageManager::new(),
            hd_textures: std::collections::HashMap::new(),
            hd_loading: std::collections::HashSet::new(),
            hd_tx,
            hd_rx,
            clipboard: Vec::new(),
            is_tip_visible: true,
            notification: None,
        }
    }
    fn active_pane_state(&mut self) -> &mut PaneState {
        match self.active_pane {
            PaneSide::Left => &mut self.left_pane,
            PaneSide::Right => &mut self.right_pane,
        }
    }
}

impl eframe::App for BildBlitzApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.log_view.update();

        // Handle Escape to exit fullscreen or cancel renaming
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.left_pane.fullscreen_path = None;
            self.right_pane.fullscreen_path = None;
            self.image_viewer.is_open = false;
            
            // Also cancel renaming
            self.left_pane.renaming_path = None;
            self.right_pane.renaming_path = None;
        }

        // Handle F2 for Rename
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

        // Handle Ctrl + N for new folder
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N)) {
            let side = self.active_pane;
            self.create_new_folder(side, ctx);
        }

        // Handle Enter for Gallery Mode (Open selection)
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            let active_side = self.active_pane;
            let state = match active_side {
                PaneSide::Left => &mut self.left_pane,
                PaneSide::Right => &mut self.right_pane,
            };

            if state.renaming_path.is_none() { // Don't trigger if renaming
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

        // Handle incoming thumbnails
        while let Ok(result) = self.thumb_rx.try_recv() {
            let texture = ctx.load_texture(
                result.path.to_string_lossy(),
                result.image,
                Default::default()
            );
            self.left_grid.textures.insert(result.path.clone(), texture.clone());
            self.right_grid.textures.insert(result.path.clone(), texture);
        }

        // Handle incoming HD images
        while let Ok(result) = self.hd_rx.try_recv() {
            let texture = ctx.load_texture(
                format!("hd_{}", result.path.to_string_lossy()),
                result.image,
                Default::default()
            );
            self.hd_textures.insert(result.path.clone(), texture);
            self.hd_loading.remove(&result.path);
        }
        // Handle incoming scan results
        while let Ok(result) = self.scan_rx.try_recv() {
            if let Some(path) = result.invalidated_path {
                self.left_grid.folder_counts.remove(&path);
                self.left_grid.loading_counts.remove(&path);
                self.right_grid.folder_counts.remove(&path);
                self.right_grid.loading_counts.remove(&path);
            }
            match result.pane_side {
                PaneSide::Left => { self.left_pane.files = result.files; self.left_pane.last_selected_index = None; }
                PaneSide::Right => { self.right_pane.files = result.files; self.right_pane.last_selected_index = None; }
            }
        }

        // Handle incoming folder counts
        while let Ok(result) = self.count_rx.try_recv() {
            self.left_grid.folder_counts.insert(result.path.clone(), result.count);
            self.right_grid.folder_counts.insert(result.path, result.count);
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("BildBlitz");
                ui.separator();
                
                crate::ui::tools::toolbar(ui);
                ui.separator();
                
                if ui.button(if self.is_split_view_active { "🔲 Single" } else { "👥 Split" }).clicked() {
                    self.is_split_view_active = !self.is_split_view_active;
                    if !self.is_split_view_active { self.active_pane = PaneSide::Left; }
                }

                if self.is_split_view_active {
                    ui.separator();
                    ui.label("Focus:");
                    ui.selectable_value(&mut self.active_pane, PaneSide::Left, "⬅ Left");
                    ui.selectable_value(&mut self.active_pane, PaneSide::Right, "Right ➡");
                }

                ui.separator();
                let mut view_state = self.active_pane_state().view_state;
                ui.selectable_value(&mut view_state, crate::ui::pane_state::ViewState::Grid, "⣿ Grid");
                ui.selectable_value(&mut view_state, crate::ui::pane_state::ViewState::List, "☰ List");
                self.active_pane_state().view_state = view_state;

                ui.separator();
                ui.label("Thumbnail Size:");
                let mut size = self.active_pane_state().thumbnail_size;
                if ui.add(egui::Slider::new(&mut size, 64.0..=512.0)).changed() {
                    self.active_pane_state().thumbnail_size = size;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.selectable_label(self.log_view.is_open, "📋 Logs").clicked() {
                        self.log_view.is_open = !self.log_view.is_open;
                    }
                    
                    ui.separator();

                    let can_create = self.active_pane_state().current_path.is_some();
                    if ui.add_enabled(can_create, egui::Button::new("📁+ New Folder")).clicked() {
                        let side = self.active_pane;
                        let ctx = ui.ctx().clone();
                        self.create_new_folder(side, &ctx);
                    }
                });
            });
        });

        if self.is_tip_visible {
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
                                    ui.label(egui::RichText::new("💡 PRO TIP").strong().color(ui.visuals().selection.bg_fill).size(14.0));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button("✕").clicked() {
                                            self.is_tip_visible = false;
                                        }
                                    });
                                });
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("You can still use Cmd/Ctrl + Drag to copy files instead of moving them. The counters will also update correctly for copy operations!").size(12.5).line_height(Some(16.0)));
                            });
                        });
                });
        }

        if let Some(notification) = &self.notification {
            if ctx.input(|i| i.time) < notification.expire_time {
                egui::Area::new("notification_bubble".into())
                    .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
                    .show(ctx, |ui| {
                        egui::Frame::window(ui.style())
                            .fill(ui.visuals().window_fill().gamma_multiply(0.9))
                            .corner_radius(32.0)
                            .inner_margin(egui::Margin::symmetric(24, 12))
                            .stroke(ui.visuals().selection.stroke)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("✨").size(18.0));
                                    ui.label(egui::RichText::new(&notification.message).strong().size(14.0));
                                });
                            });
                    });
                ctx.request_repaint(); // Keep repainting while notification is visible
            } else {
                self.notification = None;
            }
        }

        egui::SidePanel::left("nav_panel")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                if let Some(path) = self.navigation.show(ui) {
                    self.trigger_scan(path, self.active_pane, ctx);
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

impl BildBlitzApp {
    fn trigger_scan(&mut self, path: PathBuf, side: PaneSide, ctx: &egui::Context) {
        let state = match side {
            PaneSide::Left => &mut self.left_pane,
            PaneSide::Right => &mut self.right_pane,
        };

        if state.current_path.as_ref() != Some(&path) {
            state.current_path = Some(path.clone());
            let tx = self.scan_tx.clone();
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let files = crate::engine::gallery::GalleryScanner::scan_directory(&path).await;
                let _ = tx.send(ScanResult { pane_side: side, files, invalidated_path: None }).await;
                ctx.request_repaint();
            });
        }
    }

    fn handle_grid_action(&mut self, action: crate::ui::grid::GridAction, side: PaneSide, ctx: &egui::Context) {
        match action {
            crate::ui::grid::GridAction::None => {},
            crate::ui::grid::GridAction::Open(path) => {
                let files = match side {
                    PaneSide::Left => &self.left_pane.files,
                    PaneSide::Right => &self.right_pane.files,
                };
                if let Some(index) = files.iter().position(|f| f.path == path) {
                    self.image_viewer.current_index = index;
                    self.image_viewer.is_open = true;
                }
            },
            crate::ui::grid::GridAction::Details(path) => {
                match crate::library::metadata::MetadataParser::extract_metadata(&path) {
                    Ok(meta) => {
                        self.properties_metadata = Some(meta);
                        self.is_properties_open = true;
                    }
                    Err(e) => tracing::error!("Failed to extract metadata: {}", e),
                }
            },
            crate::ui::grid::GridAction::Navigate(path) => {
                self.trigger_scan(path, side, ctx);
            },
            crate::ui::grid::GridAction::Drop(paths, target_subfolder) => {
                self.handle_drop(paths, target_subfolder, side, ctx);
            },
            crate::ui::grid::GridAction::Rename(path, new_name) => {
                if new_name.trim().is_empty() { return; }
                let mut dest = path.clone();
                dest.set_file_name(new_name);
                
                if dest == path { return; }

                let tx = self.scan_tx.clone();
                let ctx = ctx.clone();
                let current_dir = match side {
                    PaneSide::Left => self.left_pane.current_path.clone(),
                    PaneSide::Right => self.right_pane.current_path.clone(),
                };
  
                tokio::spawn(async move {
                    if let Err(e) = std::fs::rename(&path, &dest) {
                        tracing::error!("Rename failed: {}", e);
                    }
                    if let Some(dir) = current_dir {
                        let files = crate::engine::gallery::GalleryScanner::scan_directory(&dir).await;
                        let _ = tx.send(ScanResult { pane_side: side, files, invalidated_path: None }).await;
                        ctx.request_repaint();
                    }
                });
            },
            crate::ui::grid::GridAction::Cut(paths) => {
                self.clipboard = paths;
            },
            crate::ui::grid::GridAction::Paste(target_path) => {
                if !self.clipboard.is_empty() {
                    let source_paths = self.clipboard.clone();
                    self.clipboard.clear();
                    self.handle_drop(source_paths, target_path, side, ctx);
                }
            }
        }
    }

    fn show_single_pane(&mut self, ui: &mut egui::Ui) {
        if let Some(path) = self.left_pane.current_path.clone() {
            self.show_path_bar(ui, &path);
            ui.add_space(4.0);
            
            let can_paste = !self.clipboard.is_empty();
            let action = self.left_grid.show(ui, &mut self.left_pane, PaneSide::Left, &self.thumbnail_manager, &self.thumb_tx, &self.count_tx, can_paste);
            self.handle_grid_action(action, PaneSide::Left, ui.ctx());
        } else {
            ui.centered_and_justified(|ui| {
                self.show_welcome(ui);
            });
        }
    }

    fn show_dual_pane(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |columns| {
            // Left Pane
            columns[0].push_id("left_pane", |ui| {
                ui.vertical(|ui| {
                    let is_active = self.active_pane == PaneSide::Left;
                    let frame = if is_active {
                        egui::Frame::group(ui.style())
                            .fill(ui.visuals().selection.bg_fill.gamma_multiply(0.1))
                            .stroke(egui::Stroke::new(2.5, ui.visuals().selection.bg_fill))
                    } else {
                        egui::Frame::group(ui.style())
                            .fill(ui.visuals().panel_fill)
                            .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
                    };

                    frame.show(ui, |ui| {
                        if ui.interact(ui.available_rect_before_wrap(), ui.id(), egui::Sense::click()).clicked() {
                            self.active_pane = PaneSide::Left;
                        }
                        if let Some(path) = self.left_pane.current_path.clone() {
                            self.show_path_bar(ui, &path);
                            ui.add_space(4.0);
                            
                            let can_paste = !self.clipboard.is_empty();
                            let action = self.left_grid.show(ui, &mut self.left_pane, PaneSide::Left, &self.thumbnail_manager, &self.thumb_tx, &self.count_tx, can_paste);
                            self.handle_grid_action(action, PaneSide::Left, ui.ctx());
                        } else {
                            ui.centered_and_justified(|ui| ui.label("Select a folder for Left Pane"));
                        }
                    });
                });
            });

            // Right Pane
            columns[1].push_id("right_pane", |ui| {
                ui.vertical(|ui| {
                    let is_active = self.active_pane == PaneSide::Right;
                    let frame = if is_active {
                        egui::Frame::group(ui.style())
                            .fill(ui.visuals().selection.bg_fill.gamma_multiply(0.1))
                            .stroke(egui::Stroke::new(2.5, ui.visuals().selection.bg_fill))
                    } else {
                        egui::Frame::group(ui.style())
                            .fill(ui.visuals().panel_fill)
                            .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
                    };

                    frame.show(ui, |ui| {
                        if ui.interact(ui.available_rect_before_wrap(), ui.id(), egui::Sense::click()).clicked() {
                            self.active_pane = PaneSide::Right;
                        }
                        if let Some(path) = self.right_pane.current_path.clone() {
                            self.show_path_bar(ui, &path);
                            ui.add_space(4.0);
                            
                            let can_paste = !self.clipboard.is_empty();
                            let action = self.right_grid.show(ui, &mut self.right_pane, PaneSide::Right, &self.thumbnail_manager, &self.thumb_tx, &self.count_tx, can_paste);
                            self.handle_grid_action(action, PaneSide::Right, ui.ctx());
                        } else {
                            ui.centered_and_justified(|ui| ui.label("Select a folder for Right Pane"));
                        }
                    });
                });
            });
        });
    }

    fn handle_drop(&mut self, source_paths: Vec<PathBuf>, target_subfolder: Option<PathBuf>, target_side: PaneSide, ctx: &egui::Context) {
        let base_target_path = match target_side {
            PaneSide::Left => self.left_pane.current_path.clone(),
            PaneSide::Right => self.right_pane.current_path.clone(),
        };

        let dest_dir = if let Some(subfolder) = target_subfolder {
            subfolder
        } else if let Some(base) = base_target_path {
            base
        } else {
            return;
        };

        let is_copy = ctx.input(|i| i.modifiers.command);
        let tx = self.scan_tx.clone();
        let ctx = ctx.clone();
        let side = target_side;
        let refresh_dir = match side {
            PaneSide::Left => self.left_pane.current_path.clone(),
            PaneSide::Right => self.right_pane.current_path.clone(),
        };

        tokio::spawn(async move {
            for source_path in source_paths {
                if source_path.parent() == Some(&dest_dir) {
                    continue; // Same folder, ignore
                }

                let dest_path = dest_dir.join(source_path.file_name().unwrap());
                let res = if is_copy {
                    std::fs::copy(&source_path, &dest_path).map(|_| ())
                } else {
                    std::fs::rename(&source_path, &dest_path)
                };
                
                if let Err(e) = res {
                    tracing::error!("File operation failed for {:?}: {}", source_path, e);
                }
            }

            if let Some(dir) = refresh_dir {
                let files = crate::engine::gallery::GalleryScanner::scan_directory(&dir).await;
                let _ = tx.send(ScanResult { pane_side: side, files, invalidated_path: Some(dest_dir.clone()) }).await;
                ctx.request_repaint();
            }
        });
    }

    fn show_welcome(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.heading("Welcome to BildBlitz");
            ui.add_space(10.0);
            ui.label("Select a folder from the sidebar to start browsing.");
            ui.add_space(20.0);
            ui.label("⚡ Blazing fast • 🚀 Hardware accelerated • 🎨 Native Windows 11");
        });
    }
    
    fn find_common_name(paths: &[PathBuf]) -> String {
        if paths.is_empty() { return "New Folder".to_string(); }
        
        let names: Vec<String> = paths.iter()
            .filter_map(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        
        if names.is_empty() { return "New Folder".to_string(); }
        
        let first = &names[0];
        let mut common_len = first.len();
        
        for name in &names[1..] {
            common_len = first.chars().zip(name.chars())
                .take_while(|(a, b)| a == b)
                .count();
            if common_len == 0 { break; }
        }
        
        let mut result = first[..common_len].to_string();
        while !result.is_empty() && !result.chars().last().unwrap().is_alphanumeric() {
            result.pop();
        }
        
        if result.len() < 3 { "Collection".to_string() } else { result }
    }

    fn create_new_folder(&mut self, side: PaneSide, ctx: &egui::Context) {
        let (state, _other_state) = match side {
            PaneSide::Left => (&mut self.left_pane, &mut self.right_pane),
            PaneSide::Right => (&mut self.right_pane, &mut self.left_pane),
        };

        if let Some(current_path) = state.current_path.clone() {
            let selected_paths: Vec<PathBuf> = state.selected_files.iter().cloned().collect();
            
            if selected_paths.len() > 1 {
                let base_name = Self::find_common_name(&selected_paths);
                let mut folder_name = base_name.clone();
                let mut new_path = current_path.join(&folder_name);
                let mut counter = 2;

                while new_path.exists() {
                    folder_name = format!("{} ({})", base_name, counter);
                    new_path = current_path.join(&folder_name);
                    counter += 1;
                }

                if let Err(e) = std::fs::create_dir(&new_path) {
                    tracing::error!("Failed to create smart folder: {}", e);
                    return;
                }

                let count = selected_paths.len();
                let dest_dir = new_path.clone();
                let tx = self.scan_tx.clone();
                let ctx_clone = ctx.clone();
                let side_clone = side;
                let refresh_dir = current_path.clone();

                tokio::spawn(async move {
                    for source in selected_paths {
                        let dest = dest_dir.join(source.file_name().unwrap());
                        let _ = std::fs::rename(&source, &dest);
                    }
                    
                    let files = crate::engine::gallery::GalleryScanner::scan_directory(&refresh_dir).await;
                    let _ = tx.send(ScanResult { pane_side: side_clone, files, invalidated_path: Some(dest_dir) }).await;
                    ctx_clone.request_repaint();
                });

                state.selected_files.clear();
                self.notification = Some(Notification {
                    message: format!("Moved {} items to \"{}\"", count, folder_name),
                    expire_time: ctx.input(|i| i.time) + 4.0,
                });
            } else {
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

                state.renaming_path = Some(new_path.clone());
                state.rename_buffer = folder_name;
                
                let tx = self.scan_tx.clone();
                let ctx_clone = ctx.clone();
                tokio::spawn(async move {
                    let files = crate::engine::gallery::GalleryScanner::scan_directory(&current_path).await;
                    let _ = tx.send(ScanResult { pane_side: side, files, invalidated_path: None }).await;
                    ctx_clone.request_repaint();
                });
            }
        }
    }

    fn handle_viewer_action(&mut self, action: ViewerAction, side: PaneSide) {
        let files = match side {
            PaneSide::Left => &self.left_pane.files,
            PaneSide::Right => &self.right_pane.files,
        };

        match action {
            ViewerAction::None => {},
            ViewerAction::Next => {
                if self.image_viewer.current_index < files.len() - 1 {
                    self.image_viewer.current_index += 1;
                    while self.image_viewer.current_index < files.len() - 1 && files[self.image_viewer.current_index].is_dir {
                        self.image_viewer.current_index += 1;
                    }
                }
            },
            ViewerAction::Prev => {
                if self.image_viewer.current_index > 0 {
                    self.image_viewer.current_index -= 1;
                    while self.image_viewer.current_index > 0 && files[self.image_viewer.current_index].is_dir {
                        self.image_viewer.current_index -= 1;
                    }
                }
            },
            ViewerAction::JumpToIndex(idx) => {
                self.image_viewer.current_index = idx;
            },
            ViewerAction::Close => {
                self.image_viewer.is_open = false;
            }
        }
    }

    fn show_properties_pane(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.heading("📋 Properties");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕").clicked() {
                        self.is_properties_open = false;
                    }
                });
            });
            ui.separator();

            if let Some(meta) = &self.properties_metadata {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(8.0);
                    
                    // Thumbnail preview (shrunk)
                    if let Some(texture) = self.left_grid.textures.get(&meta.path).or_else(|| self.right_grid.textures.get(&meta.path)) {
                        let size = ui.available_width() - 32.0;
                        let tex_size = texture.size_vec2();
                        let ratio = (size / tex_size.x).min(size / tex_size.y).min(1.0);
                        ui.image((texture.id(), tex_size * ratio));
                        ui.add_space(12.0);
                    }

                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new(&meta.name).strong().size(14.0));
                        ui.label(egui::RichText::new(meta.path.to_string_lossy()).weak().size(10.0));
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
                            let size_mb = meta.size_bytes as f64 / (1024.0 * 1024.0);
                            ui.label(format!("{:.2} MB", size_mb));
                            ui.end_row();

                            ui.label("Format:");
                            ui.label(&meta.format);
                            ui.end_row();

                            ui.label("Modified:");
                            let datetime: chrono::DateTime<chrono::Local> = meta.modified.into();
                            ui.label(datetime.format("%Y-%m-%d %H:%M:%S").to_string());
                            ui.end_row();
                        });

                    ui.add_space(20.0);
                    let reveal_btn = egui::Button::new("📁 Reveal in Explorer")
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

    fn show_fullscreen(&mut self, ctx: &egui::Context) {
        let side = self.active_pane;
        let current_files = match side {
            PaneSide::Left => &self.left_pane.files,
            PaneSide::Right => &self.right_pane.files,
        };

        // Trigger HD Loading for current image if needed
        if let Some(file) = current_files.get(self.image_viewer.current_index) {
            if !file.is_dir && !self.hd_textures.contains_key(&file.path) && !self.hd_loading.contains(&file.path) {
                self.request_hd_image(file.path.clone(), ctx);
            }
        }

        let action = match side {
            PaneSide::Left => self.image_viewer.show(ctx, &self.left_pane.files, &self.left_grid.textures, &self.hd_textures),
            PaneSide::Right => self.image_viewer.show(ctx, &self.right_pane.files, &self.right_grid.textures, &self.hd_textures),
        };
        self.handle_viewer_action(action, side);
    }

    fn request_hd_image(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.hd_loading.insert(path.clone());
        let tx = self.hd_tx.clone();
        let manager = self.full_image_manager.clone();
        let ctx = ctx.clone();
        
        tokio::spawn(async move {
            if let Some(image) = manager.get_image(&path).await {
                let _ = tx.send(FullImageResult { path, image }).await;
                ctx.request_repaint();
            }
        });
    }

    fn show_path_bar(&self, ui: &mut egui::Ui, path: &std::path::Path) {
        let frame = egui::Frame::NONE
            .fill(ui.visuals().widgets.noninteractive.bg_fill.gamma_multiply(0.5))
            .inner_margin(egui::Margin::symmetric(12, 6))
            .corner_radius(4);

        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(egui::RichText::new("📂").size(14.0));
                
                let path_str = path.to_string_lossy();
                ui.add(egui::Label::new(egui::RichText::new(path_str)
                    .size(11.0)
                    .color(ui.visuals().strong_text_color())
                    .monospace())
                ).on_hover_text("Current full directory path");
            });
        });
    }
}
