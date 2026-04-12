use std::path::{Path, PathBuf};
use sysinfo::Disks;
use tracing::{error, warn};

use crate::engine::supported::is_supported_image;

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

    match std::fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries.filter_map(Result::ok) {
                match entry.file_type() {
                    Ok(file_type) => {
                        if file_type.is_dir() {
                            children.push(entry.path());
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get file type for {:?}: {}", entry.path(), e)
                    }
                }
            }
        }
        Err(e) => error!("Failed to read directory {:?}: {}", path, e),
    }

    // Sort directories alphabetically
    children.sort();
    children
}

/// Quickly counts the number of supported image files in a directory (non-recursive).
/// Uses the centralized extension list from `engine::supported`.
pub fn count_supported_files(path: &Path) -> usize {
    match std::fs::read_dir(path) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file() && is_supported_image(&e.path()))
            .count(),
        Err(_) => 0,
    }
}
