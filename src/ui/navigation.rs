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

    pub fn show(&mut self, ui: &mut egui::Ui, can_paste: bool) -> crate::messages::NavAction {
        use crate::messages::NavAction;
        let mut action = NavAction::None;

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
                    let response = ui.selectable_label(is_selected, label);
                    
                    if response.clicked() {
                        self.selected_path = Some(path.clone());
                        action = NavAction::Navigate(path.clone());
                    }

                    response.context_menu(|ui| {
                        if ui.add_enabled(can_paste, egui::Button::new("📋 Paste Into")).clicked() {
                            action = NavAction::PasteInto(path.clone());
                            ui.close();
                        }
                    });
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
                        let response = ui.selectable_label(is_selected, label);

                        if response.clicked() {
                            self.selected_path = Some(fav.path.clone());
                            action = NavAction::Navigate(fav.path.clone());
                        }

                        response.context_menu(|ui| {
                            if ui.add_enabled(can_paste, egui::Button::new("📋 Paste Into")).clicked() {
                                action = NavAction::PasteInto(fav.path.clone());
                                ui.close();
                            }
                        });
                    }
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);
                
                ui.heading("Local Drives");
                ui.add_space(4.0);
                
                for (label, path, count) in self.local_drives.clone() {
                    let nav_label = format!("💾 {} ({})", label, count);
                    if let Some(a) = self.render_tree(ui, &path, &nav_label, can_paste) {
                        action = a;
                    }
                }
            });

        action
    }

    fn render_tree(&mut self, ui: &mut egui::Ui, path: &Path, label: &str, can_paste: bool) -> Option<crate::messages::NavAction> {
        use crate::messages::NavAction;
        let mut action = None;
        let is_selected = self.selected_path.as_deref() == Some(path);
        
        let id = egui::Id::new(path);
        let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);

        state.show_header(ui, |ui| {
            let response = ui.selectable_label(is_selected, label);
            if response.clicked() {
                self.selected_path = Some(path.to_path_buf());
                action = Some(NavAction::Navigate(path.to_path_buf()));
            }

            response.context_menu(|ui| {
                if ui.add_enabled(can_paste, egui::Button::new("📋 Paste Into")).clicked() {
                    action = Some(NavAction::PasteInto(path.to_path_buf()));
                    ui.close();
                }
            });
        }).body(|ui| {
            // Lazy load children
            let children = self.expanded_dirs.entry(path.to_path_buf()).or_insert_with(|| {
                crate::library::scanner::get_child_directories(path)
            });

            for child in children.clone() {
                let child_name = child.file_name().unwrap_or_default().to_string_lossy();
                let count = crate::library::scanner::count_supported_files(&child);
                if let Some(a) = self.render_tree(ui, &child, &format!("📁 {} ({})", child_name, count), can_paste) {
                    action = Some(a);
                }
            }
        });

        action
    }
}
