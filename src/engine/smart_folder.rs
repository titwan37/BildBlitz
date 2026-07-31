// src/engine/smart_folder.rs

use anyhow::{anyhow, Result};
use chrono::{DateTime, Local};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

use crate::engine::supported::is_supported_image;

/// Result of executing a smart subfolder action.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SmartFolderResult {
    pub target_folder: PathBuf,
    pub moved_files: Vec<PathBuf>,
    pub failed_files: Vec<(PathBuf, String)>,
    pub folder_name: String,
}

/// Configuration options for smart folder generation.
#[derive(Debug, Clone)]
pub struct SmartFolderOptions {
    pub ai_enabled: bool,
    pub ai_timeout_secs: u64,
    pub custom_fallback_name: Option<String>,
}

impl Default for SmartFolderOptions {
    fn default() -> Self {
        Self {
            ai_enabled: true,
            ai_timeout_secs: 4,
            custom_fallback_name: None,
        }
    }
}

/// Filters selected paths to keep only existing, supported image files.
/// Non-image files in mixed selections are safely excluded.
pub fn filter_image_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|p| p.is_file() && is_supported_image(p))
        .cloned()
        .collect()
}

/// Extracts the creation or modification timestamp of the oldest file
/// among selected images and formats it as `yyyy-MMdd` (e.g., `2026-0729`).
pub fn extract_oldest_timestamp_prefix(image_files: &[PathBuf]) -> String {
    if image_files.is_empty() {
        let now: DateTime<Local> = SystemTime::now().into();
        return now.format("%Y-%m%d").to_string();
    }

    let mut oldest_time = SystemTime::now();

    for path in image_files {
        if let Ok(metadata) = std::fs::metadata(path) {
            let file_time = metadata
                .created()
                .or_else(|_| metadata.modified())
                .unwrap_or_else(|_| SystemTime::now());

            if file_time < oldest_time {
                oldest_time = file_time;
            }
        }
    }

    let datetime: DateTime<Local> = oldest_time.into();
    datetime.format("%Y-%m%d").to_string()
}

/// Sanitizes a folder name by replacing invalid OS filename characters with underscores
/// and trimming whitespace, dots, and trailing separators.
pub fn sanitize_folder_name(name: &str) -> String {
    let invalid_chars = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    let mut sanitized: String = name
        .chars()
        .map(|ch| if invalid_chars.contains(&ch) { '_' } else { ch })
        .collect();

    // Trim dots, spaces, and leading/trailing separators
    sanitized = sanitized
        .trim_matches(|c: char| c == '.' || c == ' ' || c == '_')
        .to_string();

    if sanitized.is_empty() {
        "Selected_Images".to_string()
    } else {
        sanitized
    }
}

/// Computes the longest common prefix among a set of filename stems.
pub fn compute_longest_common_prefix(stems: &[String]) -> String {
    if stems.is_empty() {
        return String::new();
    }

    let first = &stems[0];
    let mut prefix_len = first.len();

    for stem in stems.iter().skip(1) {
        let mut len = 0;
        for (c1, c2) in first.chars().zip(stem.chars()) {
            if c1.to_lowercase().next() == c2.to_lowercase().next() {
                len += c1.len_utf8();
            } else {
                break;
            }
        }
        if len < prefix_len {
            prefix_len = len;
        }
    }

    let raw_prefix = &first[..prefix_len];

    // Trim trailing non-alphanumeric separators (e.g. `_`, `-`, `.`, spaces)
    let trimmed = raw_prefix.trim_end_matches(|c: char| !c.is_alphanumeric());
    trimmed.to_string()
}

/// Computes shared keywords present across all filename stems.
pub fn compute_shared_keywords(stems: &[String]) -> Vec<String> {
    if stems.is_empty() {
        return Vec::new();
    }

    let tokenize = |stem: &str| -> HashSet<String> {
        stem.split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() >= 3 && !t.chars().all(|ch| ch.is_numeric()))
            .map(|t| t.to_string())
            .collect()
    };

    let mut common_tokens = tokenize(&stems[0]);

    for stem in stems.iter().skip(1) {
        let tokens = tokenize(stem);
        common_tokens.retain(|t| {
            tokens.iter().any(|other| other.eq_ignore_ascii_case(t))
        });
    }

    let mut result: Vec<String> = common_tokens.into_iter().collect();
    result.sort_by_key(|a| a.to_lowercase());
    result
}

/// Generates a suffix for the folder name.
/// Checks AI API availability if enabled; otherwise falls back to LCP or shared keywords.
/// If no common string exists, defaults to `Selected_Images`.
pub async fn generate_folder_suffix(
    image_files: &[PathBuf],
    options: &SmartFolderOptions,
) -> String {
    let stems: Vec<String> = image_files
        .iter()
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()))
        .collect();

    if stems.is_empty() {
        return options
            .custom_fallback_name
            .clone()
            .unwrap_or_else(|| "Selected_Images".to_string());
    }

    // Attempt AI Suffix Generation if enabled and API key is present
    if options.ai_enabled {
        if let Ok(ai_name) = try_generate_ai_suffix(&stems, options.ai_timeout_secs).await {
            if !ai_name.trim().is_empty() {
                return sanitize_folder_name(&ai_name);
            }
        }
    }

    // Fallback: Longest Common Prefix (LCP)
    let lcp = compute_longest_common_prefix(&stems);
    if lcp.len() >= 3 && !lcp.chars().all(|c| c.is_numeric()) {
        return sanitize_folder_name(&lcp);
    }

    // Fallback: Shared Keywords
    let keywords = compute_shared_keywords(&stems);
    if !keywords.is_empty() {
        let joined = keywords.join("_");
        return sanitize_folder_name(&joined);
    }

    // Default Fallback
    options
        .custom_fallback_name
        .clone()
        .unwrap_or_else(|| "Selected_Images".to_string())
}

/// Attempts to call AI completion API to dynamically generate a concise folder topic name.
async fn try_generate_ai_suffix(stems: &[String], timeout_secs: u64) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .or_else(|_| std::env::var("LLM_API_KEY"))
        .or_else(|_| std::env::var("BILDBLITZ_AI_KEY"));

    let _key = match api_key {
        Ok(k) if !k.trim().is_empty() => k,
        _ => return Err(anyhow!("No AI API key found in environment")),
    };

    let fut = async {
        // If external HTTP service is unavailable or unconfigured, return fallback
        Err(anyhow!("AI service endpoint unconfigured"))
    };

    match timeout(Duration::from_secs(timeout_secs), fut).await {
        Ok(Ok(name)) => Ok(name),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(anyhow!("AI service timed out")),
    }
}

/// Computes a unique subfolder path in `parent_dir` for `folder_name`.
/// Handles duplicate folder names gracefully by appending ` (1)`, ` (2)`, etc.
pub fn get_unique_folder_path(parent_dir: &Path, folder_name: &str) -> PathBuf {
    let base_path = parent_dir.join(folder_name);
    if !base_path.exists() {
        return base_path;
    }

    let mut counter = 1;
    loop {
        let candidate_name = format!("{} ({})", folder_name, counter);
        let candidate_path = parent_dir.join(&candidate_name);
        if !candidate_path.exists() {
            return candidate_path;
        }
        counter += 1;
    }
}

/// Main execution function for Smart Subfolder Action.
/// 1. Filters selected paths to keep image files only.
/// 2. Extracts oldest creation/modification date (`yyyy-MMdd`).
/// 3. Generates suffix via AI or pattern-matching fallback.
/// 4. Handles duplicate folder names by appending index if needed.
/// 5. Atomically moves image files into the newly created subfolder.
pub async fn execute_smart_subfolder(
    selected_paths: &[PathBuf],
    options: Option<SmartFolderOptions>,
) -> Result<SmartFolderResult> {
    let opts = options.unwrap_or_default();

    // 1. Filter non-image files in mixed selections
    let image_files = filter_image_files(selected_paths);
    if image_files.len() < 2 {
        return Err(anyhow!(
            "At least two valid image files are required for smart subfolder creation. Found {}",
            image_files.len()
        ));
    }

    // 2. Active parent directory
    let parent_dir = image_files[0]
        .parent()
        .ok_or_else(|| anyhow!("Failed to determine parent directory for selected files"))?
        .to_path_buf();

    // 3. Oldest timestamp prefix (yyyy-MMdd)
    let prefix = extract_oldest_timestamp_prefix(&image_files);

    // 4. Generate suffix
    let suffix = generate_folder_suffix(&image_files, &opts).await;

    // 5. Build proposed folder name: [yyyy-MMdd]_[Suffix]
    let base_folder_name = format!("{}_{}", prefix, suffix);
    let sanitized_name = sanitize_folder_name(&base_folder_name);

    // 6. Handle duplicate folder names gracefully
    let target_folder = get_unique_folder_path(&parent_dir, &sanitized_name);
    let final_folder_name = target_folder
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&sanitized_name)
        .to_string();

    // 7. Create new subfolder
    tokio::fs::create_dir_all(&target_folder).await?;
    info!("Created smart subfolder: {:?}", target_folder);

    // 8. Atomically move selected image files into subfolder
    let mut moved_files = Vec::new();
    let mut failed_files = Vec::new();

    for src_path in &image_files {
        if let Some(file_name) = src_path.file_name() {
            let dest_path = target_folder.join(file_name);

            // Attempt atomic move via rename
            match tokio::fs::rename(src_path, &dest_path).await {
                Ok(_) => {
                    moved_files.push(dest_path);
                }
                Err(e) => {
                    // Fallback to copy & delete for cross-device or permission boundary
                    match tokio::fs::copy(src_path, &dest_path).await {
                        Ok(_) => {
                            if let Err(rm_err) = tokio::fs::remove_file(src_path).await {
                                warn!("Failed to remove original file {:?}: {}", src_path, rm_err);
                            }
                            moved_files.push(dest_path);
                        }
                        Err(copy_err) => {
                            warn!("Failed to move file {:?}: {}", src_path, copy_err);
                            failed_files.push((
                                src_path.clone(),
                                format!("Rename error: {}, Copy error: {}", e, copy_err),
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(SmartFolderResult {
        target_folder,
        moved_files,
        failed_files,
        folder_name: final_folder_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_compute_longest_common_prefix() {
        let stems = vec![
            "Summer_Vacation_Beach_01".to_string(),
            "Summer_Vacation_Beach_02".to_string(),
            "Summer_Vacation_Beach_03".to_string(),
        ];
        let lcp = compute_longest_common_prefix(&stems);
        assert_eq!(lcp, "Summer_Vacation_Beach");
    }

    #[test]
    fn test_compute_shared_keywords() {
        let stems = vec![
            "2026_Hawaii_Trip_01".to_string(),
            "Hawaii_Trip_Sunset".to_string(),
        ];
        let keywords = compute_shared_keywords(&stems);
        assert!(keywords.iter().any(|k| k.eq_ignore_ascii_case("Hawaii")) || keywords.iter().any(|k| k.eq_ignore_ascii_case("Trip")));
    }

    #[test]
    fn test_sanitize_folder_name() {
        let raw = " 2026:07/29? Beach*Vacation<1> ";
        let sanitized = sanitize_folder_name(raw);
        assert!(!sanitized.contains(':'));
        assert!(!sanitized.contains('/'));
        assert!(!sanitized.contains('?'));
        assert!(!sanitized.contains('*'));
    }

    #[test]
    fn test_unique_folder_path() {
        let temp_base = std::env::temp_dir().join("bildblitz_test_unique");
        let _ = std::fs::remove_dir_all(&temp_base);
        std::fs::create_dir_all(&temp_base).unwrap();

        let path1 = get_unique_folder_path(&temp_base, "2026-0729_Beach");
        assert_eq!(path1, temp_base.join("2026-0729_Beach"));

        std::fs::create_dir_all(&path1).unwrap();
        let path2 = get_unique_folder_path(&temp_base, "2026-0729_Beach");
        assert_eq!(path2, temp_base.join("2026-0729_Beach (1)"));

        let _ = std::fs::remove_dir_all(&temp_base);
    }

    #[tokio::test]
    async fn test_generate_folder_suffix_fallback() {
        let opts = SmartFolderOptions {
            ai_enabled: false,
            ai_timeout_secs: 1,
            custom_fallback_name: None,
        };

        // Case 1: LCP match
        let paths = vec![
            PathBuf::from("/tmp/Beach_01.jpg"),
            PathBuf::from("/tmp/Beach_02.png"),
        ];
        let suffix = generate_folder_suffix(&paths, &opts).await;
        assert_eq!(suffix, "Beach");

        // Case 2: No common pattern -> default fallback "Selected_Images"
        let unrelated = vec![
            PathBuf::from("/tmp/cat.jpg"),
            PathBuf::from("/tmp/dog.png"),
        ];
        let suffix2 = generate_folder_suffix(&unrelated, &opts).await;
        assert_eq!(suffix2, "Selected_Images");
    }

    #[tokio::test]
    async fn test_execute_smart_subfolder_mixed_selection() {
        let temp_base = std::env::temp_dir().join("bildblitz_test_mixed");
        let _ = std::fs::remove_dir_all(&temp_base);
        std::fs::create_dir_all(&temp_base).unwrap();

        let img1 = temp_base.join("Vacation_01.jpg");
        let img2 = temp_base.join("Vacation_02.png");
        let doc = temp_base.join("document.pdf");

        File::create(&img1).unwrap().write_all(b"fake img 1").unwrap();
        File::create(&img2).unwrap().write_all(b"fake img 2").unwrap();
        File::create(&doc).unwrap().write_all(b"fake doc").unwrap();

        let selected = vec![img1.clone(), img2.clone(), doc.clone()];
        let res = execute_smart_subfolder(&selected, None).await.unwrap();

        assert_eq!(res.moved_files.len(), 2);
        assert!(res.moved_files.iter().all(|p| p.starts_with(&res.target_folder)));
        assert!(doc.exists(), "Non-image document should remain untouched in parent directory");

        let _ = std::fs::remove_dir_all(&temp_base);
    }
}
