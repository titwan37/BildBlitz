use eframe::egui;
use crate::messages::ToolbarAction;
use std::path::PathBuf;

pub fn toolbar(
    ui: &mut egui::Ui,
    active_nav_folder: Option<PathBuf>,
    active_left_grid: Option<PathBuf>,
    active_right_grid: Option<PathBuf>,
) -> ToolbarAction {
    let mut action = ToolbarAction::None;
    
    ui.horizontal(|ui| {
        ui.label("Transform:");
        if ui.button("⟲ 90°").on_hover_text("Rotate Left").clicked() {
            action = ToolbarAction::Rotate(270);
        }
        if ui.button("⟳ 90°").on_hover_text("Rotate Right").clicked() {
            action = ToolbarAction::Rotate(90);
        }
        if ui.button("↕ Flip V").on_hover_text("Flip Vertical").clicked() {
            action = ToolbarAction::FlipV;
        }
        if ui.button("↔ Flip H").on_hover_text("Flip Horizontal").clicked() {
            action = ToolbarAction::FlipH;
        }

        ui.separator();

        let mut num_selected = 0;
        if active_nav_folder.is_some() { num_selected += 1; }
        if active_left_grid.is_some() { num_selected += 1; }
        if active_right_grid.is_some() { num_selected += 1; }
        
        let enabled = num_selected == 1;
        
        if ui.add_enabled(enabled, egui::Button::new("✏ Rename")).on_hover_text("Rename selected folder").clicked() {
            if let Some(path) = active_nav_folder {
                action = ToolbarAction::InitiateRenameNav(path);
            } else if let Some(path) = active_left_grid {
                action = ToolbarAction::InitiateRenameGrid(path, crate::ui::pane_state::PaneSide::Left);
            } else if let Some(path) = active_right_grid {
                action = ToolbarAction::InitiateRenameGrid(path, crate::ui::pane_state::PaneSide::Right);
            }
        }
    });

    action
}
