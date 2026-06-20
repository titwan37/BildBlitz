use eframe::egui;
use crate::messages::ToolbarAction;

pub fn toolbar(ui: &mut egui::Ui) -> ToolbarAction {
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
    });

    action
}
