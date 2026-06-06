use std::path::PathBuf;

use crate::messages::{AutoGroupConfig, AutoGroupProgress, AutoGroupResult};

#[derive(Default)]
pub struct AutoGroupState {
    pub is_open: bool,
    pub weight_color: f32,
    pub weight_time: f32,
    pub weight_name: f32,
    pub eps: f32,
    pub min_samples: usize,
    pub create_physical: bool,
    pub is_docked: bool,
    pub is_running: bool,
    pub auto_tune_running: bool,
    pub progress: Option<AutoGroupProgress>,
    pub result: Option<AutoGroupResult>,
}

impl AutoGroupState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            weight_color: 0.2,
            weight_time: 0.1,
            weight_name: 0.1,
            eps: 0.6,
            min_samples: 5,
            create_physical: false,
            is_docked: true, // Default to docked as it's cleaner
            is_running: false,
            auto_tune_running: false,
            progress: None,
            result: None,
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        current_path: Option<PathBuf>,
        tx: tokio::sync::mpsc::Sender<crate::messages::BackendMsg>,
    ) {
        if !self.is_open {
            return;
        }

        if self.is_docked {
            egui::SidePanel::right("auto_group_panel")
                .resizable(true)
                .default_width(320.0)
                .show(ctx, |ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.heading("Auto-Group by Theme");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✕").on_hover_text("Close").clicked() {
                                self.is_open = false;
                            }
                            if ui.button("🗗").on_hover_text("Undock").clicked() {
                                self.is_docked = false;
                            }
                        });
                    });
                    ui.separator();
                    
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.render_content(ui, current_path, tx);
                    });
                });
        } else {
            let mut is_open = self.is_open;
            egui::Window::new("Auto-Group by Theme")
                .open(&mut is_open)
                .collapsible(false)
                .resizable(true)
                .default_width(400.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Floating Window");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("🗖").on_hover_text("Dock to side").clicked() {
                                self.is_docked = true;
                            }
                        });
                    });
                    ui.separator();
                    self.render_content(ui, current_path, tx);
                });
            self.is_open = is_open;
        }
    }

    fn render_content(
        &mut self,
        ui: &mut egui::Ui,
        current_path: Option<PathBuf>,
        tx: tokio::sync::mpsc::Sender<crate::messages::BackendMsg>,
    ) {
        if self.is_running {
            ui.label("Processing...");
            match &self.progress {
                Some(AutoGroupProgress::Extracted { done, total }) => {
                    let pct = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                    ui.add(egui::ProgressBar::new(pct).text(format!("Extracting features: {}/{}", done, total)));
                }
                Some(AutoGroupProgress::Clustering { percent }) => {
                    ui.add(egui::ProgressBar::new(percent / 100.0).text(format!("Clustering: {:.1}%", percent)));
                }
                Some(AutoGroupProgress::Moving { done, total }) => {
                    let pct = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                    ui.add(egui::ProgressBar::new(pct).text(format!("Moving files: {}/{}", done, total)));
                }
                // VirtualClustersUpdated is consumed by app.rs; show live discovery label
                Some(AutoGroupProgress::VirtualClustersUpdated { clusters }) => {
                    let n: usize = clusters.iter().map(|c| c.members.len()).sum();
                    ui.add(egui::ProgressBar::new(1.0).animate(true)
                        .text(format!("Streaming… {} images in {} groups", n, clusters.len())));
                }
                None => {
                    ui.spinner();
                }
            }
            return;
        }

        let mut commit_req = None;
        let mut close_req = false;

        if let Some(res) = &self.result {
            ui.heading("Results (Virtual Collections)");

            ui.add_space(6.0);
            ui.label(format!("Generated {} clusters.", res.clusters.len()));
            ui.add_space(8.0);

            // ── Determinant Force Feedback ─────────────────────────────────────
            let (pt, pc, ppal) = res.forces;
            let dominant = if pt >= pc && pt >= ppal { "Time" }
                           else if pc >= pt && pc >= ppal { "Color" }
                           else { "Composition" };

            egui::Frame::group(ui.style())
                .corner_radius(6.0)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("⚙ Determinant Forces").strong().size(13.0));
                    ui.label(egui::RichText::new("What drove this clustering?").small().italics()
                        .color(ui.visuals().weak_text_color()));
                    ui.add_space(6.0);

                    // Time bar
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("⏱ Time ").monospace().size(11.0));
                        ui.add(egui::ProgressBar::new(pt / 100.0)
                            .text(format!("{:.1}%", pt))
                            .fill(egui::Color32::from_rgb(80, 140, 220)));
                    });
                    // Color bar
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("🎨 Color").monospace().size(11.0));
                        ui.add(egui::ProgressBar::new(pc / 100.0)
                            .text(format!("{:.1}%", pc))
                            .fill(egui::Color32::from_rgb(200, 100, 160)));
                    });
                    // Composition bar
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("📐 Comp.").monospace().size(11.0));
                        ui.add(egui::ProgressBar::new(ppal / 100.0)
                            .text(format!("{:.1}%", ppal))
                            .fill(egui::Color32::from_rgb(80, 190, 140)));
                    });

                    ui.add_space(6.0);
                    let tip = format!(
                        "💡 {} is currently driving cluster formation. \
                         {}",
                        dominant,
                        match dominant {
                            "Time" => "Lower 'Time Weight' if you want more color-based groups.",
                            "Color" => "Lower 'Color Weight' if timestamps matter more.",
                            _ => "Adjust Epsilon to control cluster granularity.",
                        }
                    );
                    ui.label(egui::RichText::new(tip).small().italics()
                        .color(ui.visuals().warn_fg_color));
                });

            ui.add_space(8.0);
            ui.label(egui::RichText::new("Review clusters in the main view via the 'Virtual Collections' tab.").small());
            ui.add_space(6.0);

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("✅ Commit to Physical Folders").clicked() {
                    commit_req = Some(res.clone());
                }
                if ui.button("✕ Close").clicked() {
                    close_req = true;
                }
            });
        }

        if let Some(res) = commit_req {
            if let Some(path) = current_path.clone() {
                let _ = tx.try_send(crate::messages::BackendMsg::AutoGroupCommit {
                    result: res,
                    source_path: path,
                });
                self.is_running = true;
                self.result = None;
            }
        }
        
        if close_req {
            self.result = None;
            self.is_open = false;
        }

        if self.result.is_some() {
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Adjust parameters to re-run:").italics());
        }

        // Configuration UI
        ui.heading("Feature Weights");
        ui.add(egui::Slider::new(&mut self.weight_time, 0.0..=1.0).text("Time Weight"));
        ui.add(egui::Slider::new(&mut self.weight_color, 0.0..=1.0).text("Color Weight"));
        ui.add(egui::Slider::new(&mut self.weight_name, 0.0..=1.0).text("Filename Weight"));

        ui.separator();
        ui.heading("Advanced Settings (DBSCAN)");
        
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut self.eps, 0.01..=1.0).text("Epsilon"));
            
            if ui.add_enabled(!self.auto_tune_running && !self.is_running, egui::Button::new("Auto-Tune")).clicked() {
                if let Some(path) = current_path.clone() {
                        let config = AutoGroupConfig {
                            weight_color: self.weight_color,
                            weight_time: self.weight_time,
                            weight_name: self.weight_name,
                            eps: self.eps,
                            min_samples: self.min_samples,
                            create_physical: false,
                            source_path: path,
                        };
                        let _ = tx.try_send(crate::messages::BackendMsg::AutoGroupTuneEpsilon(config));
                        self.auto_tune_running = true;
                        self.is_running = true;
                        self.result = None;
                }
            }
        });

        if self.auto_tune_running {
            ui.label(egui::RichText::new("Calculating optimal epsilon...").italics().color(egui::Color32::KHAKI));
        }

        ui.add(egui::Slider::new(&mut self.min_samples, 1..=20).text("Min Samples"));

        ui.separator();
        ui.checkbox(&mut self.create_physical, "Automatically create physical folders");

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Run Auto-Group").clicked() {
                if let Some(path) = current_path.clone() {
                    let config = AutoGroupConfig {
                        weight_color: self.weight_color,
                        weight_time: self.weight_time,
                        weight_name: self.weight_name,
                        eps: self.eps,
                        min_samples: self.min_samples,
                        create_physical: self.create_physical,
                        source_path: path,
                    };
                    
                    let _ = tx.try_send(crate::messages::BackendMsg::AutoGroupStart(config));
                    self.is_running = true;
                    self.result = None;
                }
            }

            if ui.button("🔬 Run Research Study (3 Algos)").on_hover_text("Compare 3 clustering algorithms and save metrics to result_folder").clicked() {
                if let Some(path) = current_path {
                    let config = AutoGroupConfig {
                        weight_color: self.weight_color,
                        weight_time: self.weight_time,
                        weight_name: self.weight_name,
                        eps: self.eps,
                        min_samples: self.min_samples,
                        create_physical: false, // Don't create folders for study
                        source_path: path,
                    };
                    
                    let _ = tx.try_send(crate::messages::BackendMsg::AutoGroupRunStudy(config));
                    self.is_running = true;
                    self.result = None;
                }
            }
        });
    }
}
