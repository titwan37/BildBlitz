use eframe::egui;
use std::path::PathBuf;
use crate::ui::pane_state::PaneState;
use crate::engine::gallery::ThumbnailManager;
use tokio::sync::mpsc;

pub struct GridView {
    pub textures: std::collections::HashMap<std::path::PathBuf, egui::TextureHandle>,
    pub loading: std::collections::HashSet<std::path::PathBuf>,
    pub folder_counts: std::collections::HashMap<std::path::PathBuf, usize>,
    pub loading_counts: std::collections::HashSet<std::path::PathBuf>,
}

pub enum GridAction {
    None,
    Open(PathBuf), // For images
    Navigate(PathBuf), // For folders
    Drop(Vec<PathBuf>, Option<PathBuf>),
    Rename(PathBuf, String),
    Details(PathBuf),
    Cut(Vec<PathBuf>),
    Paste(Option<PathBuf>),
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

    pub fn show(&mut self, ui: &mut egui::Ui, state: &mut PaneState, side: crate::ui::pane_state::PaneSide, thumbnail_manager: &ThumbnailManager, thumb_tx: &mpsc::Sender<crate::app::ThumbnailResult>, count_tx: &mpsc::Sender<crate::app::FolderCountResult>, can_paste: bool) -> GridAction {
        let mut action = GridAction::None;

        if ui.rect_contains_pointer(ui.available_rect_before_wrap()) {
            let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
            if ui.input(|i| i.modifiers.command) && scroll_delta != 0.0 {
                state.thumbnail_size = (state.thumbnail_size + scroll_delta * 0.1).clamp(64.0, 512.0);
            }
        }

        egui::ScrollArea::vertical()
            .id_salt(ui.id().with(side).with("main_scroll"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let fill = if ui.visuals().dark_mode { egui::Color32::from_gray(20) } else { egui::Color32::from_gray(250) };
                
                let (_inner, dropped_payload) = ui.dnd_drop_zone::<Vec<PathBuf>, _>(egui::Frame::NONE.fill(fill).inner_margin(8.0), |ui| {
                    ui.set_min_size(ui.available_size());
                    
                    let files = state.files.clone();
                    let thumbnail_size = state.thumbnail_size;
                    let view_state = state.view_state;

                    match view_state {
                        crate::ui::pane_state::ViewState::Grid => {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                                for (i, file) in files.iter().enumerate() {
                                    if let Some(a) = self.render_grid_item(ui, file, i, thumbnail_size, state, thumbnail_manager, thumb_tx, count_tx, can_paste) {
                                        action = a;
                                    }
                                }
                            });
                        }
                        crate::ui::pane_state::ViewState::List => {
                            ui.vertical(|ui| {
                                for (i, file) in files.iter().enumerate() {
                                    if let Some(a) = self.render_list_item(ui, file, i, state, thumbnail_manager, thumb_tx, count_tx, can_paste) {
                                        action = a;
                                    }
                                }
                            });
                        }
                    }
                });

                if let Some(payload) = dropped_payload {
                    action = GridAction::Drop((*payload).clone(), None);
                }
                
                ui.interact(ui.available_rect_before_wrap(), ui.id().with("bg_interact"), egui::Sense::click()).context_menu(|ui| {
                    if ui.add_enabled(can_paste, egui::Button::new("📋 Paste")).clicked() {
                        action = GridAction::Paste(None);
                        ui.close();
                    }
                });
            });

        action
    }

    fn render_grid_item(&mut self, ui: &mut egui::Ui, file: &crate::engine::gallery::FileInfo, index: usize, size: f32, state: &mut PaneState, thumbnail_manager: &ThumbnailManager, thumb_tx: &mpsc::Sender<crate::app::ThumbnailResult>, count_tx: &mpsc::Sender<crate::app::FolderCountResult>, can_paste: bool) -> Option<GridAction> {
        let total_size = egui::vec2(size, size + 24.0);
        let mut action = None;
        let item_id = ui.id().with(&file.path);
        let rect = ui.allocate_exact_size(total_size, egui::Sense::hover()).0;
        let response = ui.interact(rect, item_id, egui::Sense::click_and_drag());
        self.handle_selection(file, index, state, &response, ui);
        
        response.context_menu(|ui| {
            if ui.button("🖼 Open in Gallery").clicked() {
                action = Some(GridAction::Open(file.path.clone()));
                ui.close();
            }
            if file.is_dir && file.name != ".." {
                if ui.add_enabled(can_paste, egui::Button::new("📋 Paste Into")).clicked() {
                    action = Some(GridAction::Paste(Some(file.path.clone())));
                    ui.close();
                }
            }
            if ui.button("✂ Cut").clicked() {
                let paths = if state.selected_files.contains(&file.path) { state.selected_files.iter().cloned().collect() } else { vec![file.path.clone()] };
                action = Some(GridAction::Cut(paths));
                ui.close();
            }
            if file.name != ".." {
                if ui.button("✏ Rename (F2)").clicked() {
                    state.renaming_path = Some(file.path.clone());
                    state.rename_buffer = file.name.clone();
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
        });

        if response.double_clicked() {
            action = if file.is_dir { Some(GridAction::Navigate(file.path.clone())) } else { Some(GridAction::Open(file.path.clone())) };
        }

        let is_selected = state.selected_files.contains(&file.path);
        let bg_color = if is_selected { ui.visuals().selection.bg_fill.gamma_multiply(0.15) } 
                        else if response.hovered() { ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.1) } 
                        else { egui::Color32::TRANSPARENT };
        ui.painter().rect_filled(rect, 4.0, bg_color);

        let mut dnd_action = None;
        let drag_payload = if is_selected { state.selected_files.iter().cloned().collect::<Vec<_>>() } 
                            else { vec![file.path.clone()] };
        // DND Trigger: Hold Left Mouse for 1.5s
        if response.dragged() && ui.input(|i| i.pointer.primary_down()) {
            if let Some(press_start) = ui.input(|i| i.pointer.press_start_time()) {
                let current_time = ui.input(|i| i.time);
                if current_time - press_start >= 1.5 {
                    ui.ctx().set_dragged_id(response.id);
                }
            }
        }

        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            let mut render_item = |ui: &mut egui::Ui| {
                ui.vertical_centered(|ui| {
                    let inner_size = size - 8.0;
                    let img_rect = ui.allocate_exact_size(egui::vec2(inner_size, inner_size), egui::Sense::hover()).0;
                    if file.name == ".." {
                        let color = if ui.visuals().dark_mode { egui::Color32::from_rgb(100, 160, 255) } else { egui::Color32::from_rgb(40, 100, 200) };
                        ui.painter().text(img_rect.center(), egui::Align2::CENTER_CENTER, "⤴", egui::FontId::proportional(inner_size * 0.8), color);
                    } else if file.is_dir {
                        let color = if ui.visuals().dark_mode { egui::Color32::from_rgb(200, 180, 50) } else { egui::Color32::from_rgb(220, 190, 70) };
                        ui.painter().text(img_rect.center(), egui::Align2::CENTER_CENTER, "📁", egui::FontId::proportional(inner_size * 0.8), color);
                    } else {
                        if let Some(texture) = self.textures.get(&file.path) {
                            ui.painter().image(texture.id(), img_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        } else {
                            ui.painter().rect_filled(img_rect, 2.0, egui::Color32::from_gray(40));
                        }
                    }
                    ui.add_space(2.0);
                    if state.renaming_path == Some(file.path.clone()) {
                        let response = ui.add(egui::TextEdit::singleline(&mut state.rename_buffer).id_salt("rename_input"));
                        if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                action = Some(GridAction::Rename(file.path.clone(), state.rename_buffer.clone()));
                            }
                            state.renaming_path = None;
                        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            state.renaming_path = None;
                        }
                        response.request_focus();
                    } else {
                        let truncated_name = if file.name.chars().count() > 18 {
                            let mut s: String = file.name.chars().take(15).collect();
                            s.push_str("...");
                            s
                        } else { file.name.clone() };
                        ui.label(egui::RichText::new(truncated_name).size(11.0).color(ui.visuals().text_color().gamma_multiply(0.9)));
                    }
                });
            };

            let mut render_final = |ui: &mut egui::Ui| {
                if file.is_dir && file.name != ".." { 
                    let (_inner, drop_payload) = ui.dnd_drop_zone::<Vec<PathBuf>, _>(egui::Frame::NONE, |ui| render_item(ui));
                    if let Some(payload) = drop_payload { dnd_action = Some(GridAction::Drop((*payload).clone(), Some(file.path.clone()))); }
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
        if file.is_dir && file.name != ".." && !self.folder_counts.contains_key(&file.path) && !self.loading_counts.contains(&file.path) {
            self.loading_counts.insert(file.path.clone());
            let path = file.path.clone();
            let tx = count_tx.clone();
            let ctx = ui.ctx().clone();
            tokio::spawn(async move {
                let count = crate::engine::gallery::GalleryScanner::count_images(&path).await;
                let _ = tx.send(crate::app::FolderCountResult { path, count }).await;
                ctx.request_repaint();
            });
        }

        // Render Overlay Badge for folder counts
        if file.is_dir && file.name != ".." {
            if let Some(&count) = self.folder_counts.get(&file.path) {
                let badge_rect = egui::Rect::from_min_size(
                    rect.max - egui::vec2(28.0, 42.0),
                    egui::vec2(20.0, 14.0)
                );
                ui.painter().rect_filled(badge_rect, 4.0, ui.visuals().selection.bg_fill);
                ui.painter().text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    count.to_string(),
                    egui::FontId::proportional(10.0),
                    egui::Color32::WHITE
                );
            }
        }

        // Render Selection frame ON TOP of the icon (but as a stroke)
        if is_selected {
            ui.painter().rect_stroke(rect, 4.0, (2.0, ui.visuals().selection.bg_fill), egui::StrokeKind::Inside);
        }

        if !file.is_dir && file.name != ".." && !self.textures.contains_key(&file.path) && !self.loading.contains(&file.path) {
            self.loading.insert(file.path.clone());
            let path = file.path.clone();
            let tx = thumb_tx.clone();
            let tm = thumbnail_manager.clone();
            let ctx = ui.ctx().clone();
            tokio::spawn(async move {
                if let Some(image) = tm.get_thumbnail(&path, size as u32).await {
                    let _ = tx.send(crate::app::ThumbnailResult { path, image }).await;
                    ctx.request_repaint();
                }
            });
        }

        if dnd_action.is_some() { action = dnd_action; }
        action
    }

    fn render_list_item(&mut self, ui: &mut egui::Ui, file: &crate::engine::gallery::FileInfo, index: usize, state: &mut PaneState, thumbnail_manager: &ThumbnailManager, thumb_tx: &mpsc::Sender<crate::app::ThumbnailResult>, count_tx: &mpsc::Sender<crate::app::FolderCountResult>, can_paste: bool) -> Option<GridAction> {
        let item_height = 48.0;
        let rect = ui.allocate_exact_size(egui::vec2(ui.available_width(), item_height), egui::Sense::click_and_drag()).0;
        let response = ui.interact(rect, ui.id().with(&file.path), egui::Sense::click_and_drag());
        let _item_id = response.id;
        let mut action = None;
        
        self.handle_selection(file, index, state, &response, ui);
        response.context_menu(|ui| {
            if ui.button("🖼 Open in Gallery").clicked() { action = Some(GridAction::Open(file.path.clone())); ui.close(); }
            if file.is_dir && file.name != ".." {
                if ui.add_enabled(can_paste, egui::Button::new("📋 Paste Into")).clicked() { action = Some(GridAction::Paste(Some(file.path.clone()))); ui.close(); }
            }
            if ui.button("✂ Cut").clicked() {
                let paths = if state.selected_files.contains(&file.path) { state.selected_files.iter().cloned().collect() } else { vec![file.path.clone()] };
                action = Some(GridAction::Cut(paths)); ui.close();
            }
            if file.name != ".." {
                if ui.button("✏ Rename (F2)").clicked() {
                    state.renaming_path = Some(file.path.clone());
                    state.rename_buffer = file.name.clone();
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("📁 Reveal in Explorer").clicked() { let _ = opener::reveal(&file.path); ui.close(); }
            ui.separator();
            if ui.button("📋 Properties").clicked() { action = Some(GridAction::Details(file.path.clone())); ui.close(); }
        });
        if response.double_clicked() { action = if file.is_dir { Some(GridAction::Navigate(file.path.clone())) } else { Some(GridAction::Open(file.path.clone())) }; }
        let is_selected = state.selected_files.contains(&file.path);
        let bg_color = if is_selected { ui.visuals().selection.bg_fill.gamma_multiply(0.15) } else if response.hovered() { ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.1) } else { egui::Color32::TRANSPARENT };
        ui.painter().rect_filled(rect, 2.0, bg_color);

        let mut dnd_action = None;
        let drag_payload = if is_selected { state.selected_files.iter().cloned().collect::<Vec<_>>() } else { vec![file.path.clone()] };
        // DND Trigger: Hold Left Mouse for 1.5s
        if response.dragged() && ui.input(|i| i.pointer.primary_down()) {
            if let Some(press_start) = ui.input(|i| i.pointer.press_start_time()) {
                let current_time = ui.input(|i| i.time);
                if current_time - press_start >= 1.5 {
                    ui.ctx().set_dragged_id(response.id);
                }
            }
        }

        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            let mut render_item = |ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let thumb_rect = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover()).0;
                    if file.name == ".." {
                        let color = if ui.visuals().dark_mode { egui::Color32::from_rgb(100, 160, 255) } else { egui::Color32::from_rgb(40, 100, 200) };
                        ui.painter().text(thumb_rect.center(), egui::Align2::CENTER_CENTER, "⤴", egui::FontId::proportional(28.0), color);
                    } else if file.is_dir {
                        let color = if ui.visuals().dark_mode { egui::Color32::from_rgb(200, 180, 50) } else { egui::Color32::from_rgb(220, 190, 70) };
                        ui.painter().text(thumb_rect.center(), egui::Align2::CENTER_CENTER, "📁", egui::FontId::proportional(28.0), color);
                        
                        // Badge overlay for list view
                        if let Some(&count) = self.folder_counts.get(&file.path) {
                            let badge_pos = thumb_rect.max - egui::vec2(12.0, 12.0);
                            ui.painter().rect_filled(egui::Rect::from_center_size(badge_pos, egui::vec2(16.0, 12.0)), 3.0, ui.visuals().selection.bg_fill);
                            ui.painter().text(badge_pos, egui::Align2::CENTER_CENTER, count.to_string(), egui::FontId::proportional(9.0), egui::Color32::WHITE);
                        }
                    } else if let Some(texture) = self.textures.get(&file.path) {
                        ui.painter().image(texture.id(), thumb_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                    } else {
                        ui.painter().rect_filled(thumb_rect, 2.0, egui::Color32::from_gray(40));
                    }
                    ui.add_space(8.0);
                    if state.renaming_path == Some(file.path.clone()) {
                        let response = ui.add(egui::TextEdit::singleline(&mut state.rename_buffer).id_salt("rename_input_list"));
                        if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                action = Some(GridAction::Rename(file.path.clone(), state.rename_buffer.clone()));
                            }
                            state.renaming_path = None;
                        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            state.renaming_path = None;
                        }
                        response.request_focus();
                    } else {
                        ui.label(&file.name);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        if file.name == ".." { ui.label(egui::RichText::new("Parent Folder").weak()); }
                        else if !file.is_dir { ui.label(format!("{} KB", file.size / 1024)); }
                        else { ui.label(egui::RichText::new("Folder").weak()); }
                    });
                });
            };

            let mut render_final = |ui: &mut egui::Ui| {
                if file.is_dir && file.name != ".." { 
                    let (_inner, drop_payload) = ui.dnd_drop_zone::<Vec<PathBuf>, _>(egui::Frame::NONE, |ui| render_item(ui));
                    if let Some(payload) = drop_payload { dnd_action = Some(GridAction::Drop((*payload).clone(), Some(file.path.clone()))); }
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
        if file.is_dir && file.name != ".." && !self.folder_counts.contains_key(&file.path) && !self.loading_counts.contains(&file.path) {
            self.loading_counts.insert(file.path.clone());
            let path = file.path.clone();
            let tx = count_tx.clone();
            let ctx = ui.ctx().clone();
            tokio::spawn(async move {
                let count = crate::engine::gallery::GalleryScanner::count_images(&path).await;
                let _ = tx.send(crate::app::FolderCountResult { path, count }).await;
                ctx.request_repaint();
            });
        }

        // Render Selection frame ON TOP of the icon (but as a stroke)
        if is_selected {
            ui.painter().rect_stroke(rect, 2.0, (2.0, ui.visuals().selection.bg_fill), egui::StrokeKind::Inside);
        }

        if !file.is_dir && file.name != ".." && !self.textures.contains_key(&file.path) && !self.loading.contains(&file.path) {
            self.loading.insert(file.path.clone());
            let path = file.path.clone();
            let tx = thumb_tx.clone();
            let tm = thumbnail_manager.clone();
            let ctx = ui.ctx().clone();
            tokio::spawn(async move {
                if let Some(image) = tm.get_thumbnail(&path, 40).await {
                    let _ = tx.send(crate::app::ThumbnailResult { path, image }).await;
                    ctx.request_repaint();
                }
            });
        }

        if dnd_action.is_some() { action = dnd_action; }
        action
    }

    fn handle_selection(&self, file: &crate::engine::gallery::FileInfo, index: usize, state: &mut PaneState, response: &egui::Response, ui: &egui::Ui) {
        let is_primary_press = response.is_pointer_button_down_on() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
        let modifiers = ui.input(|i| i.modifiers);
        
        if is_primary_press {
            // Handle Press (Button Down) - Instant feedback for range and toggle
            if modifiers.shift {
                if let Some(last_index) = state.last_selected_index {
                    let start = last_index.min(index);
                    let end = last_index.max(index);
                    for i in start..=end {
                        if let Some(f) = state.files.get(i) {
                            if f.name != ".." { state.selected_files.insert(f.path.clone()); }
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
            // Handle Click (Button Up) - Finalize selection if no modifiers
            if !modifiers.shift && !modifiers.ctrl && !modifiers.command {
                // Solely select on release (if we didn't drag)
                state.selected_files.clear();
                state.selected_files.insert(file.path.clone());
                state.last_selected_index = Some(index);
            }
        }
    }
}