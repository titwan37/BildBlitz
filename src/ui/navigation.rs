use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::library::scanner;
use crate::library::config::Favorite;

pub struct NavigationPane {
    quick_access: Vec<(String, PathBuf, usize)>,
    favorites: Vec<(Favorite, usize)>,
    local_drives: Vec<(String, PathBuf, usize)>,
    selected_path: Option<PathBuf>,
    expanded_dirs: HashMap<PathBuf, Vec<PathBuf>>,
}

impl NavigationPane {
    pub fn new(favorites: Vec<Favorite>) -> Self {
        let quick_access = scanner::get_quick_access_folders()
            .into_iter()
            .map(|(name, path)| {
                let count = scanner::count_supported_files(&path);
                (name, path, count)
            })
            .collect();

        let favorites_with_counts = favorites
            .into_iter()
            .map(|fav| {
                let count = scanner::count_supported_files(&fav.path);
                (fav, count)
            })
            .collect();

        let local_drives = scanner::get_local_drives()
            .into_iter()
            .map(|(name, path)| {
                let count = scanner::count_supported_files(&path);
                (name, path, count)
            })
            .collect();

        Self {
            quick_access,
            favorites: favorites_with_counts,
            local_drives,
            selected_path: None,
            expanded_dirs: HashMap::new(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        let mut clicked_path = None;

        ui.add_space(8.0);
        ui.heading("Quick Access");
        ui.add_space(4.0);
        
        egui::ScrollArea::vertical()
            .id_salt("nav_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (name, path, count) in &self.quick_access {
                    let is_selected = self.selected_path.as_deref() == Some(path);
                    let label = format!("⭐ {} ({})", name, count);
                    if ui.selectable_label(is_selected, label).clicked() {
                        self.selected_path = Some(path.clone());
                        clicked_path = Some(path.clone());
                    }
                }

                if !self.favorites.is_empty() {
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(12.0);
                    ui.heading("Favorites");
                    ui.add_space(4.0);

                    for (fav, count) in &self.favorites {
                        let is_selected = self.selected_path.as_deref() == Some(&fav.path);
                        let label = format!("📌 {} ({})", fav.name, count);
                        if ui.selectable_label(is_selected, label).clicked() {
                            self.selected_path = Some(fav.path.clone());
                            clicked_path = Some(fav.path.clone());
                        }
                    }
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);
                
                ui.heading("Local Drives");
                ui.add_space(4.0);
                
                for (label, path, count) in self.local_drives.clone() {
                    let nav_label = format!("💾 {} ({})", label, count);
                    if let Some(p) = self.render_tree(ui, &path, &nav_label) {
                        clicked_path = Some(p);
                    }
                }
            });

        clicked_path
    }

    fn render_tree(&mut self, ui: &mut egui::Ui, path: &Path, label: &str) -> Option<PathBuf> {
        let mut clicked = None;
        let is_selected = self.selected_path.as_deref() == Some(path);
        
        let id = egui::Id::new(path);
        let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);

        state.show_header(ui, |ui| {
            if ui.selectable_label(is_selected, label).clicked() {
                self.selected_path = Some(path.to_path_buf());
                clicked = Some(path.to_path_buf());
            }
        }).body(|ui| {
            // Lazy load children
            let children = self.expanded_dirs.entry(path.to_path_buf()).or_insert_with(|| {
                scanner::get_child_directories(path)
            });

            for child in children.clone() {
                let child_name = child.file_name().unwrap_or_default().to_string_lossy();
                let count = scanner::count_supported_files(&child);
                if let Some(p) = self.render_tree(ui, &child, &format!("📁 {} ({})", child_name, count)) {
                    clicked = Some(p);
                }
            }
        });

        clicked
    }
}
