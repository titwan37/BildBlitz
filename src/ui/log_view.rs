use egui::ScrollArea;
use std::collections::VecDeque;
use std::path::PathBuf;
use crate::os::logging::LogStream;

const MAX_LOG_ENTRIES: usize = 500;

pub struct LogView {
    pub entries: VecDeque<String>,
    pub is_open: bool,
    pub log_dir: PathBuf,
    pub receiver: tokio::sync::mpsc::UnboundedReceiver<String>,
    autoscroll: bool,
}

impl LogView {
    pub fn new(log_stream: LogStream, log_dir: PathBuf) -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            is_open: false,
            log_dir,
            receiver: log_stream.receiver,
            autoscroll: true,
        }
    }

    pub fn update(&mut self) {
        // Drain all available log entries
        while let Ok(msg) = self.receiver.try_recv() {
            self.entries.push_back(msg);
            // Keep only the last N entries — O(1) via VecDeque::pop_front (B13 fix)
            if self.entries.len() > MAX_LOG_ENTRIES {
                self.entries.pop_front();
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.is_open {
            return;
        }

        egui::TopBottomPanel::bottom("log_panel")
            .resizable(true)
            .default_height(150.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Live Logs");
                    ui.separator();

                    if ui
                        .button("📂 Open Log Folder")
                        .on_hover_text(
                            "Open the folder containing log files in Explorer",
                        )
                        .clicked()
                    {
                        crate::os::logging::open_log_folder(&self.log_dir);
                    }

                    if ui.button("🗑 Clear").clicked() {
                        self.entries.clear();
                    }

                    ui.checkbox(&mut self.autoscroll, "Autoscroll");

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.button("❌ Close").clicked() {
                                self.is_open = false;
                            }
                        },
                    );
                });

                ui.separator();

                let scroll_area = ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(self.autoscroll);

                scroll_area.show(ui, |ui| {
                    ui.vertical(|ui| {
                        for entry in &self.entries {
                            let color = if entry.contains("ERROR") {
                                egui::Color32::from_rgb(255, 100, 100)
                            } else if entry.contains("WARN") {
                                egui::Color32::from_rgb(255, 200, 100)
                            } else if entry.contains("DEBUG") {
                                egui::Color32::from_rgb(150, 150, 150)
                            } else {
                                ui.visuals().text_color()
                            };

                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(entry)
                                        .monospace()
                                        .size(12.0)
                                        .color(color),
                                )
                                .truncate(),
                            );
                        }
                    });
                });
            });
    }
}
