use std::collections::VecDeque;
use chrono::{DateTime, Local};
use egui::Color32;

#[derive(Clone, Debug)]
pub enum AuditStatus {
    Pending,
    Success,
    Failure(String),
}

#[derive(Clone, Debug)]
pub struct AuditItem {
    pub name: String,
    pub status: AuditStatus,
    pub timestamp: DateTime<Local>,
}

pub struct SystemAudit {
    items: VecDeque<AuditItem>,
    max_items: usize,
}

impl SystemAudit {
    pub fn new() -> Self {
        Self {
            items: VecDeque::new(),
            max_items: 50,
        }
    }

    pub fn push(&mut self, name: &str, status: AuditStatus) {
        self.items.push_front(AuditItem {
            name: name.to_string(),
            status,
            timestamp: Local::now(),
        });
        if self.items.len() > self.max_items {
            self.items.pop_back();
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.heading("Live System Audit");
            ui.add_space(8.0);

            egui::ScrollArea::vertical().show(ui, |ui| {
                for item in &self.items {
                    ui.horizontal(|ui| {
                        let (icon, color) = match &item.status {
                            AuditStatus::Pending => ("⏳", Color32::GRAY),
                            AuditStatus::Success => ("✅", Color32::GREEN),
                            AuditStatus::Failure(_) => ("❌", Color32::RED),
                        };

                        ui.label(egui::RichText::new(icon).color(color));
                        ui.label(egui::RichText::new(&item.name).strong());
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(item.timestamp.format("%H:%M:%S").to_string()).weak());
                        });
                    });

                    if let AuditStatus::Failure(msg) = &item.status {
                        ui.indent("error_msg", |ui| {
                            ui.label(egui::RichText::new(msg).color(Color32::RED).size(10.0));
                        });
                    }
                    ui.separator();
                }
            });
        });
    }
}
