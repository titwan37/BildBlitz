use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewState {
    Grid,
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabMode {
    FolderView,
    Collections,
    Duplicates,
}

use crate::engine::gallery::FileInfo;

pub struct PaneState {
    pub current_path: Option<PathBuf>,
    pub view_state: ViewState,
    pub thumbnail_size: f32,
    pub selected_files: HashSet<PathBuf>,
    pub files: Vec<FileInfo>,
    pub fullscreen_path: Option<PathBuf>,
    pub renaming_path: Option<PathBuf>,
    pub rename_buffer: String,
    pub last_selected_index: Option<usize>,
    pub tab_mode: TabMode,
}

impl PaneState {
    pub fn new() -> Self {
        Self {
            current_path: None,
            view_state: ViewState::Grid,
            thumbnail_size: 160.0,
            selected_files: HashSet::new(),
            files: Vec::new(),
            fullscreen_path: None,
            renaming_path: None,
            rename_buffer: String::new(),
            last_selected_index: None,
            tab_mode: TabMode::FolderView,
        }
    }
}
