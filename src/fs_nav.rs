use std::path::{Path, PathBuf};
use sysinfo::Disks;

/// Resolves standard media folders for the current user.
pub fn get_quick_access_folders() -> Vec<(String, PathBuf)> {
    let mut folders = Vec::new();
    
    if let Some(downloads_dir) = dirs::download_dir() {
        folders.push(("Downloads".to_string(), downloads_dir));
    }
    
    if let Some(pictures_dir) = dirs::picture_dir() {
        folders.push(("Pictures".to_string(), pictures_dir));
    }
    
    if let Some(documents_dir) = dirs::document_dir() {
        folders.push(("Documents".to_string(), documents_dir));
    }
    
    folders
}

/// Queries the OS to return a list of all available root drives.
pub fn get_local_drives() -> Vec<(String, PathBuf)> {
    let mut drives = Vec::new();
    let disks = Disks::new_with_refreshed_list();
    
    for disk in disks.list() {
        let name = disk.name().to_string_lossy().to_string();
        let mount_point = disk.mount_point().to_path_buf();
        let label = if name.is_empty() {
            format!("Local Disk ({})", mount_point.display())
        } else {
            format!("{} ({})", name, mount_point.display())
        };
        drives.push((label, mount_point));
    }
    
    drives
}

/// Fetches the immediate child directories of a given path.
pub fn get_child_directories(path: &Path) -> Vec<PathBuf> {
    let mut children = Vec::new();
    
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.filter_map(Result::ok) {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    children.push(entry.path());
                }
            }
        }
    }
    
    // Sort directories alphabetically
    children.sort();
    children
}
