use eframe::egui;
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub enum ViewerAction {
    None,
    Next,
    Prev,
    Close,
    JumpToIndex(usize),
    SaveAs(PathBuf),
}

pub struct ImageViewer {
    pub is_open: bool,
    pub current_index: usize,
}

impl ImageViewer {
    pub fn new() -> Self {
        Self {
            is_open: false,
            current_index: 0,
        }
    }

    /// Shows the fullscreen gallery viewer overlay.
    /// Accepts slices instead of `&Vec` (CS11/CS12 clippy fix).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        files: &[crate::engine::gallery::FileInfo],
        textures: &std::collections::HashMap<PathBuf, egui::TextureHandle>,
        hd_textures: &std::collections::HashMap<PathBuf, egui::TextureHandle>,
    ) -> ViewerAction {
        if !self.is_open || files.is_empty() {
            return ViewerAction::None;
        }

        let mut action = ViewerAction::None;

        // Ensure index is valid
        if self.current_index >= files.len() {
            self.current_index = 0;
        }

        egui::Area::new(egui::Id::new("gallery_view"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let screen_rect = ui.ctx().content_rect();

                // Dark Backdrop
                ui.painter()
                    .rect_filled(screen_rect, 0.0, egui::Color32::BLACK);

                // Handle Global Input
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    action = ViewerAction::Close;
                }
                if ui.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                    action = ViewerAction::Next;
                }
                if ui.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                    action = ViewerAction::Prev;
                }

                // Main Image Display
                let current_file = &files[self.current_index];
                if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
                    action = ViewerAction::SaveAs(current_file.path.clone());
                }

                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(screen_rect),
                    |ui| {
                        ui.centered_and_justified(|ui| {
                            if let Some(texture) = hd_textures
                                .get(&current_file.path)
                                .or_else(|| textures.get(&current_file.path))
                            {
                                let available_size = ui.available_size() * 0.95;
                                let tex_size = texture.size_vec2();
                                let ratio = (available_size.x / tex_size.x)
                                    .min(available_size.y / tex_size.y)
                                    .min(1.0);
                                let final_size = tex_size * ratio;

                                ui.image((texture.id(), final_size));

                                // Spinner overlay if still loading HD but showing thumbnail
                                if !hd_textures.contains_key(&current_file.path) {
                                    ui.with_layout(
                                        egui::Layout::bottom_up(egui::Align::Center),
                                        |ui| {
                                            ui.add_space(20.0);
                                            ui.add(egui::Spinner::new().size(24.0));
                                            ui.label(
                                                egui::RichText::new("Loading High-Res...")
                                                    .color(egui::Color32::WHITE)
                                                    .size(12.0),
                                            );
                                        },
                                    );
                                }
                            } else {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(screen_rect.height() / 2.0 - 20.0);
                                    ui.add(egui::Spinner::new().size(40.0));
                                    ui.label(
                                        egui::RichText::new("Loading High-Res Image...")
                                            .color(egui::Color32::WHITE)
                                            .size(16.0),
                                    );
                                });
                            }
                        });
                    },
                );

                // Top Bar
                let top_rect = egui::Rect::from_min_max(
                    screen_rect.left_top(),
                    screen_rect.right_top() + egui::vec2(0.0, 64.0),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(top_rect), |ui| {
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add_space(20.0);
                            let close_btn = egui::Button::new(
                                egui::RichText::new("✕").size(24.0).strong(),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE);

                            if ui
                                .add(close_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                action = ViewerAction::Close;
                            }

                            ui.add_space(12.0);
                            let save_as_btn = egui::Button::new(
                                egui::RichText::new("💾 Save As").size(14.0)
                            )
                            .fill(egui::Color32::from_white_alpha(30))
                            .stroke(egui::Stroke::NONE);

                            if ui
                                .add(save_as_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                action = ViewerAction::SaveAs(current_file.path.clone());
                            }

                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add_space(20.0);
                                    ui.label(
                                        egui::RichText::new(&current_file.name)
                                            .color(egui::Color32::WHITE)
                                            .size(14.0),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "({} / {})",
                                            self.current_index + 1,
                                            files.len()
                                        ))
                                        .color(egui::Color32::GRAY)
                                        .size(12.0),
                                    );
                                },
                            );
                        },
                    );
                });

                // Navigation Arrows (B9 fix: saturating_sub to prevent underflow)
                let arrow_margin = 32.0;
                let arrow_size = 48.0;

                if self.current_index > 0 {
                    let left_rect = egui::Rect::from_center_size(
                        screen_rect.left_center() + egui::vec2(arrow_margin, 0.0),
                        egui::vec2(arrow_size, arrow_size),
                    );
                    if self.draw_nav_button(ui, left_rect, "⏴") {
                        action = ViewerAction::Prev;
                    }
                }

                if !files.is_empty()
                    && self.current_index < files.len().saturating_sub(1)
                {
                    let right_rect = egui::Rect::from_center_size(
                        screen_rect.right_center()
                            - egui::vec2(arrow_margin, 0.0),
                        egui::vec2(arrow_size, arrow_size),
                    );
                    if self.draw_nav_button(ui, right_rect, "⏵") {
                        action = ViewerAction::Next;
                    }
                }

                // Thumbnail Ribbon
                self.show_ribbon(ui, screen_rect, files, textures, &mut action);
            });

        action
    }

    fn draw_nav_button(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        text: &str,
    ) -> bool {
        let response = ui.interact(rect, ui.id().with(text), egui::Sense::click());
        let color = if response.hovered() {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_gray(120).gamma_multiply(0.5)
        };

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(32.0),
            color,
        );

        if response.hovered() {
            ui.painter().circle_filled(
                rect.center(),
                28.0,
                egui::Color32::from_white_alpha(20),
            );
        }

        response.clicked()
    }

    fn show_ribbon(
        &self,
        ui: &mut egui::Ui,
        screen_rect: egui::Rect,
        files: &[crate::engine::gallery::FileInfo],
        textures: &std::collections::HashMap<PathBuf, egui::TextureHandle>,
        action: &mut ViewerAction,
    ) {
        let ribbon_height = 84.0;
        let ribbon_rect = egui::Rect::from_min_max(
            screen_rect.left_bottom() - egui::vec2(0.0, ribbon_height + 16.0),
            screen_rect.right_bottom(),
        );

        // Detect hover in bottom area
        let hover_area = egui::Rect::from_min_max(
            screen_rect.left_bottom() - egui::vec2(0.0, 140.0),
            screen_rect.right_bottom(),
        );
        let is_hovering = ui.rect_contains_pointer(hover_area);

        if is_hovering {
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(ribbon_rect),
                |ui| {
                    egui::Frame::NONE
                        .fill(egui::Color32::from_black_alpha(180))
                        .corner_radius(8.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            egui::ScrollArea::horizontal()
                                .id_salt("ribbon_scroll")
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        for (i, file) in files.iter().enumerate()
                                        {
                                            if file.is_dir {
                                                continue;
                                            }

                                            let thumb_size =
                                                egui::vec2(60.0, 60.0);
                                            let (rect, response) = ui
                                                .allocate_exact_size(
                                                    thumb_size,
                                                    egui::Sense::click(),
                                                );

                                            if let Some(texture) =
                                                textures.get(&file.path)
                                            {
                                                ui.painter().image(
                                                    texture.id(),
                                                    rect,
                                                    egui::Rect::from_min_max(
                                                        egui::pos2(0.0, 0.0),
                                                        egui::pos2(1.0, 1.0),
                                                    ),
                                                    egui::Color32::WHITE,
                                                );
                                            } else {
                                                ui.painter().rect_filled(
                                                    rect,
                                                    4.0,
                                                    egui::Color32::from_gray(30),
                                                );
                                            }

                                            if i == self.current_index {
                                                ui.painter().rect_stroke(
                                                    rect.expand(2.0),
                                                    4.0,
                                                    (2.0, egui::Color32::WHITE),
                                                    egui::StrokeKind::Outside,
                                                );
                                            } else if response.hovered() {
                                                ui.painter().rect_stroke(
                                                    rect.expand(2.0),
                                                    4.0,
                                                    (
                                                        1.0,
                                                        egui::Color32::LIGHT_GRAY,
                                                    ),
                                                    egui::StrokeKind::Outside,
                                                );
                                            }

                                            if response.clicked() {
                                                *action =
                                                    ViewerAction::JumpToIndex(i);
                                            }
                                        }
                                    });
                                });
                        });
                },
            );
        }
    }
}
