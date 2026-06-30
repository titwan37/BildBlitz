use std::collections::HashSet;
use std::path::PathBuf;
use crate::engine::gallery::{FileInfo, ThumbnailManager};
use crate::messages::{ThumbnailResult, FolderCountResult};
use crate::ui::pane_state::{PaneState, PaneSide};
use tokio::sync::mpsc;

pub struct GridView {
    pub textures: std::collections::HashMap<PathBuf, egui::TextureHandle>,
    pub loading: std::collections::HashSet<PathBuf>,
    pub folder_counts: std::collections::HashMap<PathBuf, usize>,
    pub loading_counts: std::collections::HashSet<PathBuf>,
}

pub enum GridAction {
    None,
    Open(PathBuf),
    Navigate(PathBuf),
    Drop(Vec<PathBuf>, Option<PathBuf>),
    Rename(PathBuf, String),
    Details(PathBuf),
    Cut(Vec<PathBuf>),
    Paste(Option<PathBuf>),
    SaveAs(PathBuf),
}

/// Bundles the many parameters needed by GridView::show (CS10 fix).
pub struct GridContext<'a> {
    pub side: PaneSide,
    pub thumbnail_manager: &'a ThumbnailManager,
    pub thumb_tx: &'a mpsc::Sender<ThumbnailResult>,
    pub count_tx: &'a mpsc::Sender<FolderCountResult>,
    pub can_paste: bool,
    pub cut_paths: &'a HashSet<PathBuf>,
}

impl GridView {
    pub fn new() -> Self {
        Self {
            textures: std::collections::HashMap::new(),
            loading: std::collections::HashSet::new(),
            folder_counts: std::collections::HashMap::new(),
            loading_counts: std::collections::HashSet::new(),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut PaneState,
        gctx: &GridContext<'_>,
    ) -> GridAction {
        let mut action = GridAction::None;

        // Ctrl+Scroll to resize thumbnails
        if ui.rect_contains_pointer(ui.available_rect_before_wrap()) {
            let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
            if ui.input(|i| i.modifiers.command) && scroll_delta != 0.0 {
                state.thumbnail_size =
                    (state.thumbnail_size + scroll_delta * 0.1).clamp(64.0, 512.0);
            }
        }

        let thumbnail_size = state.thumbnail_size;
        let view_state = state.view_state;
        let file_count = state.files.len();

        egui::ScrollArea::vertical()
            .id_salt(ui.id().with(gctx.side).with("main_scroll"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let fill = if ui.visuals().dark_mode {
                    egui::Color32::from_gray(20)
                } else {
                    egui::Color32::from_gray(250)
                };

                let (_inner, dropped_payload) = ui.dnd_drop_zone::<Vec<PathBuf>, _>(
                    egui::Frame::NONE.fill(fill).inner_margin(8.0),
                    |ui| {
                        ui.set_min_size(ui.available_size());

                        match view_state {
                            crate::ui::pane_state::ViewState::Grid => {
                                self.show_virtualized_grid(
                                    ui, state, gctx, thumbnail_size, file_count,
                                    &mut action,
                                );
                            }
                            crate::ui::pane_state::ViewState::List => {
                                self.show_virtualized_list(
                                    ui, state, gctx, file_count, &mut action,
                                );
                            }
                        }
                    },
                );

                if let Some(payload) = dropped_payload {
                    action = GridAction::Drop((*payload).clone(), None);
                }

                // Background context menu (paste) — Improved coverage
                let bg_response = ui.interact(
                    ui.available_rect_before_wrap(),
                    ui.id().with("bg_context_interact"),
                    egui::Sense::click(),
                );

                bg_response.context_menu(|ui| {
                    if ui
                        .add_enabled(gctx.can_paste, egui::Button::new("📋 Paste"))
                        .clicked()
                    {
                        action = GridAction::Paste(None);
                        ui.close();
                    }
                });
            });

        action
    }

    // ── Virtualized Grid Rendering ────────────────────────────────────────────

    /// Renders only the visible grid items within the scroll viewport.
    fn show_virtualized_grid(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut PaneState,
        gctx: &GridContext<'_>,
        thumbnail_size: f32,
        file_count: usize,
        action: &mut GridAction,
    ) {
        let item_spacing = egui::vec2(12.0, 12.0);
        let total_size = egui::vec2(thumbnail_size, thumbnail_size + 24.0);
        let item_total = total_size + item_spacing;

        let available_width = ui.available_width();
        let columns =
            (available_width / item_total.x).floor().max(1.0) as usize;

        let row_height = item_total.y;
        let total_rows = (file_count + columns - 1) / columns.max(1);
        let total_height = (total_rows as f32 * row_height).max(0.0);

        let (_, body) =
            ui.allocate_space(egui::vec2(available_width, total_height));
        let mut ui = ui.child_ui(body, egui::Layout::default(), None);

        let scroll_offset = (ui.clip_rect().min.y - body.min.y).max(0.0);
        let visible_height = ui.clip_rect().height();
        let first_visible_row =
            (scroll_offset / row_height).floor().max(0.0) as usize;
        let last_visible_row =
            ((scroll_offset + visible_height) / row_height).ceil() as usize;

        // Add a buffer of 1 row above and below to prevent pop-in
        let start_row = first_visible_row.saturating_sub(1);
        let end_row = (last_visible_row + 1).min(total_rows);

        for row in start_row..end_row {
            let start_idx = row * columns;
            let end_idx = (start_idx + columns).min(file_count);

            if start_idx >= file_count {
                break;
            }

            let y_offset = row as f32 * row_height;

            for col in 0..(end_idx - start_idx) {
                let i = start_idx + col;
                let x_offset = col as f32 * item_total.x;

                let item_rect = egui::Rect::from_min_size(
                    body.min + egui::vec2(x_offset, y_offset),
                    total_size,
                );

                // Skip items outside the visible area
                if !ui.clip_rect().intersects(item_rect.expand(50.0)) {
                    continue;
                }

                let file = state.files[i].clone();

                let mut item_ui = ui.child_ui(
                    item_rect,
                    egui::Layout::top_down(egui::Align::Min),
                    None,
                );
                if let Some(a) = self.render_grid_item_at_rect(
                    &mut item_ui,
                    &file,
                    i,
                    thumbnail_size,
                    state,
                    gctx,
                    item_rect,
                ) {
                    *action = a;
                }
            }
        }
    }

    // ── Virtualized List Rendering ────────────────────────────────────────────

    /// Renders only the visible list items within the scroll viewport.
    fn show_virtualized_list(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut PaneState,
        gctx: &GridContext<'_>,
        file_count: usize,
        action: &mut GridAction,
    ) {
        let item_height = 48.0;
        let total_height = file_count as f32 * item_height;

        let (_, body) =
            ui.allocate_space(egui::vec2(ui.available_width(), total_height));
        let mut ui = ui.child_ui(body, egui::Layout::default(), None);

        let scroll_offset = (ui.clip_rect().min.y - body.min.y).max(0.0);
        let visible_height = ui.clip_rect().height();
        let first_visible =
            (scroll_offset / item_height).floor().max(0.0) as usize;
        let last_visible =
            ((scroll_offset + visible_height) / item_height).ceil() as usize;

        // Add a buffer of 2 items above and below
        let start = first_visible.saturating_sub(2);
        let end = (last_visible + 2).min(file_count);

        for i in start..end {
            let file = state.files[i].clone();
            let y_offset = i as f32 * item_height;
            let item_rect = egui::Rect::from_min_size(
                body.min + egui::vec2(0.0, y_offset),
                egui::vec2(ui.available_width(), item_height),
            );

            let mut item_ui = ui.child_ui(
                item_rect,
                egui::Layout::top_down(egui::Align::Min),
                None,
            );
            if let Some(a) = self.render_list_item_at_rect(
                &mut item_ui,
                &file,
                i,
                state,
                gctx,
                item_rect,
            ) {
                *action = a;
            }
        }
    }

    // ── Shared helpers (CS3 fix: extracted from grid/list item rendering) ──────

    /// Renders the right-click context menu for any file item.
    fn render_context_menu(
        ui: &mut egui::Ui,
        file: &FileInfo,
        state: &mut PaneState,
        can_paste: bool,
    ) -> Option<GridAction> {
        let mut action = None;
        if ui.button("🖼 Open in Gallery").clicked() {
            action = Some(GridAction::Open(file.path.clone()));
            ui.close();
        }
        if file.is_dir && file.name != ".." {
            if ui
                .add_enabled(can_paste, egui::Button::new("📋 Paste Into"))
                .clicked()
            {
                action = Some(GridAction::Paste(Some(file.path.clone())));
                ui.close();
            }
        }
        if ui.button("✂ Cut").clicked() {
            let paths = if state.selected_files.contains(&file.path) {
                state.selected_files.iter().cloned().collect()
            } else {
                vec![file.path.clone()]
            };
            action = Some(GridAction::Cut(paths));
            ui.close();
        }
        if file.name != ".." {
            if ui.button("✏ Rename (F2)").clicked() {
                state.renaming_path = Some(file.path.clone());
                state.rename_buffer = file.name.clone();
                ui.close();
            }
            if ui.button("💾 Save As...").clicked() {
                action = Some(GridAction::SaveAs(file.path.clone()));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("📁 Reveal in Explorer").clicked() {
            let _ = opener::reveal(&file.path);
            ui.close();
        }
        ui.separator();
        if ui.button("📋 Properties").clicked() {
            action = Some(GridAction::Details(file.path.clone()));
            ui.close();
        }
        action
    }

    /// Handles the inline rename text edit for a file item.
    fn render_rename_edit(
        ui: &mut egui::Ui,
        file: &FileInfo,
        state: &mut PaneState,
        id_salt: &str,
    ) -> Option<GridAction> {
        let mut action = None;
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.rename_buffer).id_salt(id_salt),
        );
        if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                action = Some(GridAction::Rename(
                    file.path.clone(),
                    state.rename_buffer.clone(),
                ));
            }
            state.renaming_path = None;
        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.renaming_path = None;
        }
        response.request_focus();
        action
    }

    /// Triggers async thumbnail loading if not already loaded or in-progress.
    fn trigger_thumbnail_load(
        &mut self,
        file: &FileInfo,
        size: u32,
        side: PaneSide,
        thumb_tx: &mpsc::Sender<ThumbnailResult>,
        tm: &ThumbnailManager,
        ui: &egui::Ui,
    ) {
        if !file.is_dir
            && file.name != ".."
            && !self.textures.contains_key(&file.path)
            && !self.loading.contains(&file.path)
        {
            self.loading.insert(file.path.clone());
            let path = file.path.clone();
            let tx = thumb_tx.clone();
            let tm = tm.clone();
            let ctx = ui.ctx().clone();
            let pane_side = side;
            tokio::spawn(async move {
                if let Some(image) = tm.get_thumbnail(&path, size).await {
                    let _ = tx
                        .send(ThumbnailResult {
                            path,
                            pane_side,
                            image,
                        })
                        .await;
                    ctx.request_repaint();
                }
            });
        }
    }

    /// Triggers async folder image count if not already loaded or in-progress.
    fn trigger_count_scan(
        &mut self,
        file: &FileInfo,
        count_tx: &mpsc::Sender<FolderCountResult>,
        ui: &egui::Ui,
    ) {
        if file.is_dir
            && file.name != ".."
            && !self.folder_counts.contains_key(&file.path)
            && !self.loading_counts.contains(&file.path)
        {
            self.loading_counts.insert(file.path.clone());
            let path = file.path.clone();
            let tx = count_tx.clone();
            let ctx = ui.ctx().clone();
            tokio::spawn(async move {
                let count =
                    crate::engine::gallery::GalleryScanner::count_images(&path).await;
                let _ = tx.send(FolderCountResult { path, count }).await;
                ctx.request_repaint();
            });
        }
    }

    /// Common DnD setup: computes the drag payload and initiates drag after 1.5s hold.
    fn setup_dnd(
        file: &FileInfo,
        is_selected: bool,
        state: &PaneState,
        response: &egui::Response,
        ui: &egui::Ui,
    ) -> Vec<PathBuf> {
        let drag_payload = if is_selected {
            state.selected_files.iter().cloned().collect::<Vec<_>>()
        } else {
            vec![file.path.clone()]
        };

        // DND Trigger: Hold Left Mouse for 1.5s
        if response.dragged() && ui.input(|i| i.pointer.primary_down()) {
            if let Some(press_start) = ui.input(|i| i.pointer.press_start_time()) {
                let current_time = ui.input(|i| i.time);
                if current_time - press_start >= 1.5 {
                    ui.ctx().set_dragged_id(response.id);
                }
            }
        }

        drag_payload
    }

    // ── Grid Item Rendering (allocates its own rect) ─────────────────────────

    fn render_grid_item(
        &mut self,
        ui: &mut egui::Ui,
        file: &FileInfo,
        index: usize,
        size: f32,
        state: &mut PaneState,
        gctx: &GridContext<'_>,
    ) -> Option<GridAction> {
        let total_size = egui::vec2(size, size + 24.0);
        let rect = ui.allocate_exact_size(total_size, egui::Sense::hover()).0;
        self.render_grid_item_at_rect(ui, file, index, size, state, gctx, rect)
    }

    // ── Grid Item Rendering (uses pre-allocated rect for virtualization) ─────

    fn render_grid_item_at_rect(
        &mut self,
        ui: &mut egui::Ui,
        file: &FileInfo,
        index: usize,
        size: f32,
        state: &mut PaneState,
        gctx: &GridContext<'_>,
        rect: egui::Rect,
    ) -> Option<GridAction> {
        let mut action = None;
        let item_id = ui.id().with(&file.path);
        let response =
            ui.interact(rect, item_id, egui::Sense::click_and_drag());
        self.handle_selection(file, index, state, &response, ui);

        // Context menu
        response.context_menu(|ui| {
            if let Some(a) =
                Self::render_context_menu(ui, file, state, gctx.can_paste)
            {
                action = Some(a);
            }
        });

        if response.double_clicked() {
            action = if file.is_dir {
                Some(GridAction::Navigate(file.path.clone()))
            } else {
                Some(GridAction::Open(file.path.clone()))
            };
        }

        let is_selected = state.selected_files.contains(&file.path);
        let is_cut = gctx.cut_paths.contains(&file.path);
        
        let mut visual_opacity = 1.0;
        if is_cut {
            visual_opacity = 0.4;
        }

        let bg_color = if is_selected {
            ui.visuals()
                .selection
                .bg_fill
                .gamma_multiply(0.15)
        } else if response.hovered() {
            ui.visuals()
                .widgets
                .hovered
                .bg_fill
                .gamma_multiply(0.1)
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 4.0, bg_color);

        let mut dnd_action = None;
        let drag_payload =
            Self::setup_dnd(file, is_selected, state, &response, ui);

        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            // Apply ghosting to the entire item scope
            ui.set_opacity(visual_opacity);
            
            let mut render_item = |ui: &mut egui::Ui| {
                ui.vertical_centered(|ui| {
                    let inner_size = size - 8.0;
                    let img_rect = ui
                        .allocate_exact_size(
                            egui::vec2(inner_size, inner_size),
                            egui::Sense::hover(),
                        )
                        .0;
                    if file.name == ".." {
                        let color = if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(100, 160, 255)
                        } else {
                            egui::Color32::from_rgb(40, 100, 200)
                        };
                        ui.painter().text(
                            img_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "⤴",
                            egui::FontId::proportional(inner_size * 0.8),
                            color,
                        );
                    } else if file.is_dir {
                        let color = if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(200, 180, 50)
                        } else {
                            egui::Color32::from_rgb(220, 190, 70)
                        };
                        ui.painter().text(
                            img_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "📁",
                            egui::FontId::proportional(inner_size * 0.8),
                            color,
                        );
                    } else if let Some(texture) =
                        self.textures.get(&file.path)
                    {
                        ui.painter().image(
                            texture.id(),
                            img_rect,
                            egui::Rect::from_min_max(
                                egui::pos2(0.0, 0.0),
                                egui::pos2(1.0, 1.0),
                            ),
                            egui::Color32::WHITE,
                        );
                    } else {
                        ui.painter().rect_filled(
                            img_rect,
                            2.0,
                            egui::Color32::from_gray(40),
                        );
                    }
                    ui.add_space(2.0);

                    // Inline rename or label
                    if state.renaming_path == Some(file.path.clone()) {
                        if let Some(a) = Self::render_rename_edit(
                            ui,
                            file,
                            state,
                            "rename_input",
                        ) {
                            action = Some(a);
                        }
                    } else {
                        let truncated_name =
                            if file.name.chars().count() > 18 {
                                let mut s: String =
                                    file.name.chars().take(15).collect();
                                s.push_str("...");
                                s
                            } else {
                                file.name.clone()
                            };
                        ui.label(
                            egui::RichText::new(truncated_name)
                                .size(11.0)
                                .color(
                                    ui.visuals()
                                        .text_color()
                                        .gamma_multiply(0.9),
                                ),
                        );
                    }
                });
            };

            let mut render_final = |ui: &mut egui::Ui| {
                if file.is_dir && file.name != ".." {
                    let (_inner, drop_payload) = ui
                        .dnd_drop_zone::<Vec<PathBuf>, _>(
                            egui::Frame::NONE,
                            |ui| render_item(ui),
                        );
                    if let Some(payload) = drop_payload {
                        dnd_action = Some(GridAction::Drop(
                            (*payload).clone(),
                            Some(file.path.clone()),
                        ));
                    }
                } else {
                    render_item(ui);
                }
            };

            if ui.ctx().is_being_dragged(response.id) {
                ui.dnd_drag_source(response.id, drag_payload, render_final);
            } else {
                render_final(ui);
            }
        });

        // Trigger folder count scan
        self.trigger_count_scan(file, gctx.count_tx, ui);

        // Render Overlay Badge for folder counts
        if file.is_dir && file.name != ".." {
            if let Some(&count) = self.folder_counts.get(&file.path) {
                let badge_rect = egui::Rect::from_min_size(
                    rect.max - egui::vec2(28.0, 42.0),
                    egui::vec2(20.0, 14.0),
                );
                ui.painter().rect_filled(
                    badge_rect,
                    4.0,
                    ui.visuals().selection.bg_fill,
                );
                ui.painter().text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    count.to_string(),
                    egui::FontId::proportional(10.0),
                    egui::Color32::WHITE,
                );
            }
        }

        // Selection frame
        if is_selected {
            ui.painter().rect_stroke(
                rect,
                4.0,
                (2.0, ui.visuals().selection.bg_fill),
                egui::StrokeKind::Inside,
            );
        }

        // Trigger thumbnail load
        self.trigger_thumbnail_load(
            file,
            size as u32,
            gctx.side,
            gctx.thumb_tx,
            gctx.thumbnail_manager,
            ui,
        );

        if dnd_action.is_some() {
            action = dnd_action;
        }
        action
    }

    // ── List Item Rendering (allocates its own rect) ─────────────────────────

    fn render_list_item(
        &mut self,
        ui: &mut egui::Ui,
        file: &FileInfo,
        index: usize,
        state: &mut PaneState,
        gctx: &GridContext<'_>,
    ) -> Option<GridAction> {
        let item_height = 48.0;
        let rect = ui
            .allocate_exact_size(
                egui::vec2(ui.available_width(), item_height),
                egui::Sense::click_and_drag(),
            )
            .0;
        self.render_list_item_at_rect(ui, file, index, state, gctx, rect)
    }

    // ── List Item Rendering (uses pre-allocated rect for virtualization) ─────

    fn render_list_item_at_rect(
        &mut self,
        ui: &mut egui::Ui,
        file: &FileInfo,
        index: usize,
        state: &mut PaneState,
        gctx: &GridContext<'_>,
        rect: egui::Rect,
    ) -> Option<GridAction> {
        let response =
            ui.interact(rect, ui.id().with(&file.path), egui::Sense::click_and_drag());
        let mut action = None;

        self.handle_selection(file, index, state, &response, ui);

        // Context menu (shared helper)
        response.context_menu(|ui| {
            if let Some(a) =
                Self::render_context_menu(ui, file, state, gctx.can_paste)
            {
                action = Some(a);
            }
        });

        if response.double_clicked() {
            action = if file.is_dir {
                Some(GridAction::Navigate(file.path.clone()))
            } else {
                Some(GridAction::Open(file.path.clone()))
            };
        }

        let is_selected = state.selected_files.contains(&file.path);
        let is_cut = gctx.cut_paths.contains(&file.path);

        let mut visual_opacity = 1.0;
        if is_cut {
            visual_opacity = 0.4;
        }

        let bg_color = if is_selected {
            ui.visuals()
                .selection
                .bg_fill
                .gamma_multiply(0.15)
        } else if response.hovered() {
            ui.visuals()
                .widgets
                .hovered
                .bg_fill
                .gamma_multiply(0.1)
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 2.0, bg_color);

        let mut dnd_action = None;
        let drag_payload =
            Self::setup_dnd(file, is_selected, state, &response, ui);

        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            // Apply ghosting to the entire item scope
            ui.set_opacity(visual_opacity);

            let mut render_item = |ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let thumb_rect = ui
                        .allocate_exact_size(
                            egui::vec2(40.0, 40.0),
                            egui::Sense::hover(),
                        )
                        .0;
                    if file.name == ".." {
                        let color = if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(100, 160, 255)
                        } else {
                            egui::Color32::from_rgb(40, 100, 200)
                        };
                        ui.painter().text(
                            thumb_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "⤴",
                            egui::FontId::proportional(28.0),
                            color,
                        );
                    } else if file.is_dir {
                        let color = if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(200, 180, 50)
                        } else {
                            egui::Color32::from_rgb(220, 190, 70)
                        };
                        ui.painter().text(
                            thumb_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "📁",
                            egui::FontId::proportional(28.0),
                            color,
                        );

                        // Badge overlay for list view
                        if let Some(&count) =
                            self.folder_counts.get(&file.path)
                        {
                            let badge_pos =
                                thumb_rect.max - egui::vec2(12.0, 12.0);
                            ui.painter().rect_filled(
                                egui::Rect::from_center_size(
                                    badge_pos,
                                    egui::vec2(16.0, 12.0),
                                ),
                                3.0,
                                ui.visuals().selection.bg_fill,
                            );
                            ui.painter().text(
                                badge_pos,
                                egui::Align2::CENTER_CENTER,
                                count.to_string(),
                                egui::FontId::proportional(9.0),
                                egui::Color32::WHITE,
                            );
                        }
                    } else if let Some(texture) =
                        self.textures.get(&file.path)
                    {
                        ui.painter().image(
                            texture.id(),
                            thumb_rect,
                            egui::Rect::from_min_max(
                                egui::pos2(0.0, 0.0),
                                egui::pos2(1.0, 1.0),
                            ),
                            egui::Color32::WHITE,
                        );
                    } else {
                        ui.painter().rect_filled(
                            thumb_rect,
                            2.0,
                            egui::Color32::from_gray(40),
                        );
                    }
                    ui.add_space(8.0);

                    // Inline rename or label (shared helper)
                    if state.renaming_path == Some(file.path.clone()) {
                        if let Some(a) = Self::render_rename_edit(
                            ui,
                            file,
                            state,
                            "rename_input_list",
                        ) {
                            action = Some(a);
                        }
                    } else {
                        ui.label(&file.name);
                    }

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add_space(8.0);
                            if file.name == ".." {
                                ui.label(
                                    egui::RichText::new("Parent Folder").weak(),
                                );
                            } else if !file.is_dir {
                                ui.label(format!(
                                    "{} KB",
                                    file.size / 1024
                                ));
                            } else {
                                ui.label(
                                    egui::RichText::new("Folder").weak(),
                                );
                            }
                        },
                    );
                });
            };

            let mut render_final = |ui: &mut egui::Ui| {
                if file.is_dir && file.name != ".." {
                    let (_inner, drop_payload) = ui
                        .dnd_drop_zone::<Vec<PathBuf>, _>(
                            egui::Frame::NONE,
                            |ui| render_item(ui),
                        );
                    if let Some(payload) = drop_payload {
                        dnd_action = Some(GridAction::Drop(
                            (*payload).clone(),
                            Some(file.path.clone()),
                        ));
                    }
                } else {
                    render_item(ui);
                }
            };

            if ui.ctx().is_being_dragged(response.id) {
                ui.dnd_drag_source(response.id, drag_payload, render_final);
            } else {
                render_final(ui);
            }
        });

        // Trigger folder count scan (shared helper)
        self.trigger_count_scan(file, gctx.count_tx, ui);

        // Selection frame
        if is_selected {
            ui.painter().rect_stroke(
                rect,
                2.0,
                (2.0, ui.visuals().selection.bg_fill),
                egui::StrokeKind::Inside,
            );
        }

        // Trigger thumbnail load for list view (shared helper)
        self.trigger_thumbnail_load(
            file,
            40,
            gctx.side,
            gctx.thumb_tx,
            gctx.thumbnail_manager,
            ui,
        );

        if dnd_action.is_some() {
            action = dnd_action;
        }
        action
    }

    // ── Virtual Collections (Clusters) Rendering ─────────────────────────────

    pub fn show_clusters(
        &mut self,
        ui: &mut egui::Ui,
        clusters: &[crate::messages::Cluster],
        state: &mut PaneState,
        gctx: &GridContext<'_>,
    ) -> GridAction {
        let mut action = GridAction::None;

        // Ctrl+Scroll to resize thumbnails
        if ui.rect_contains_pointer(ui.available_rect_before_wrap()) {
            let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
            if ui.input(|i| i.modifiers.command) && scroll_delta != 0.0 {
                state.thumbnail_size =
                    (state.thumbnail_size + scroll_delta * 0.1).clamp(64.0, 512.0);
            }
        }

        egui::ScrollArea::vertical()
            .id_salt(ui.id().with(gctx.side).with("clusters_scroll"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let thumbnail_size = state.thumbnail_size;

                for cluster in clusters {
                    let header_text = if cluster.id == 0 {
                        "📁 Miscellaneous".to_string()
                    } else {
                        format!("Theme {} ({} items)", cluster.id, cluster.members.len())
                    };

                    ui.add_space(8.0);
                    
                    let id = ui.id().with("cluster").with(cluster.id);
                    egui::CollapsingHeader::new(
                        egui::RichText::new(header_text).heading().strong()
                    )
                    .id_salt(id)
                    .default_open(true)
                    .show(ui, |ui| {
                        if let Some(label) = &cluster.label {
                            ui.label(egui::RichText::new(format!("💡 Reason: {}", label)).weak().italics());
                        }
                        
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                            for (idx, path) in cluster.members.iter().enumerate() {
                                let file = FileInfo {
                                    path: path.clone(),
                                    name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                    is_dir: false,
                                    size: 0,
                                    dimensions: None,
                                    modified: std::time::SystemTime::now(),
                                    phash: None,
                                };

                                if let Some(a) = self.render_grid_item(
                                    ui,
                                    &file,
                                    idx,
                                    thumbnail_size,
                                    state,
                                    gctx,
                                ) {
                                    action = a;
                                }
                            }
                        });
                        ui.add_space(12.0);
                    });
                    ui.separator();
                }
            });

        action
    }

    // ── Selection Logic ───────────────────────────────────────────────────────

    fn handle_selection(
        &self,
        file: &FileInfo,
        index: usize,
        state: &mut PaneState,
        response: &egui::Response,
        ui: &egui::Ui,
    ) {
        let is_primary_press = response.is_pointer_button_down_on()
            && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
        let modifiers = ui.input(|i| i.modifiers);

        if is_primary_press {
            if modifiers.shift {
                if let Some(last_index) = state.last_selected_index {
                    let start = last_index.min(index);
                    let end = last_index.max(index);
                    for i in start..=end {
                        if let Some(f) = state.files.get(i) {
                            if f.name != ".." {
                                state.selected_files.insert(f.path.clone());
                            }
                        }
                    }
                } else {
                    state.selected_files.insert(file.path.clone());
                }
                state.last_selected_index = Some(index);
            } else if modifiers.ctrl || modifiers.command {
                if state.selected_files.contains(&file.path) {
                    state.selected_files.remove(&file.path);
                } else {
                    state.selected_files.insert(file.path.clone());
                }
                state.last_selected_index = Some(index);
            } else {
                // Normal select on press only if NOT already selected
                // (This allows dragging a multi-selected group)
                if !state.selected_files.contains(&file.path) {
                    state.selected_files.clear();
                    state.selected_files.insert(file.path.clone());
                    state.last_selected_index = Some(index);
                }
            }
        } else if response.clicked() {
            // Finalize selection on release if no modifiers
            if !modifiers.shift && !modifiers.ctrl && !modifiers.command {
                state.selected_files.clear();
                state.selected_files.insert(file.path.clone());
                state.last_selected_index = Some(index);
            }
        }
    }
}