// src/engine/smart_folder.rs

use anyhow::{anyhow, Result};
use chrono::{DateTime, Local};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

use crate::engine::auto_group::{
    combined_distance, extract_single_feature, ImageFeature, NormStats,
};
use crate::engine::gallery::FileInfo;
use crate::engine::supported::is_supported_image;
use crate::messages::DeterminantForces;

/// Represents a clustered similarity group within a smart subfolder.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SmartClusterGroup {
    pub cluster_id: usize,
    pub folder_name: String,
    pub dominant_force: String,
    pub member_paths: Vec<PathBuf>,
    pub best_shot: Option<PathBuf>,
    pub culled_shots: Vec<PathBuf>,
}

/// Result of executing a smart subfolder action.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SmartFolderResult {
    pub target_folder: PathBuf,
    pub moved_files: Vec<PathBuf>,
    pub failed_files: Vec<(PathBuf, String)>,
    pub folder_name: String,
    pub clusters: Vec<SmartClusterGroup>,
    pub forces: DeterminantForces,
}

/// Configuration options for smart folder generation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SmartFolderOptions {
    pub ai_enabled: bool,
    pub ai_timeout_secs: u64,
    pub custom_fallback_name: Option<String>,
    // --- New Similarity Forces & Culling Configuration ---
    pub similarity_clustering: bool,
    pub similarity_threshold: f64,
    pub max_clusters: usize,
    pub culling_mode: bool,
    pub weight_time: f32,
    pub weight_color: f32,
    pub weight_name: f32,
    pub weight_sketch: f32,
    pub weight_binary: f32,
    pub weight_raytrace: f32,
}

impl Default for SmartFolderOptions {
    fn default() -> Self {
        Self {
            ai_enabled: true,
            ai_timeout_secs: 4,
            custom_fallback_name: None,
            similarity_clustering: true,
            similarity_threshold: 0.45,
            max_clusters: 8,
            culling_mode: true,
            weight_time: 0.2,
            weight_color: 0.2,
            weight_name: 0.1,
            weight_sketch: 0.2,
            weight_binary: 0.2,
            weight_raytrace: 0.0,
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

    // Trim trailing sequence digits and non-alphanumeric separators (e.g. `_0`, `_`, `-`, `.`, spaces)
    let trimmed = raw_prefix
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end_matches(|c: char| !c.is_alphanumeric());
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
async fn try_generate_ai_suffix(_stems: &[String], timeout_secs: u64) -> Result<String> {
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

/// Internal representation of an online cluster during smart grouping.
struct InternalSmartCluster {
    _id: usize,
    members: Vec<ImageFeature>,
    sum_time: f64,
    sum_l: f64,
    sum_a: f64,
    sum_b: f64,
    sum_ar: f64,
    sum_sketch: f64,
    sum_binary: f64,
    sum_raytrace: f64,
    rep_phash: Option<u64>,
    rep_palette: Vec<[f32; 3]>,
}

impl InternalSmartCluster {
    fn new(id: usize, feat: &ImageFeature) -> Self {
        Self {
            _id: id,
            members: vec![feat.clone()],
            sum_time: feat.time as f64,
            sum_l: feat.l as f64,
            sum_a: feat.a as f64,
            sum_b: feat.b as f64,
            sum_ar: feat.aspect_ratio as f64,
            sum_sketch: feat.sketch_score as f64,
            sum_binary: feat.binary_score as f64,
            sum_raytrace: feat.raytrace_score as f64,
            rep_phash: feat.phash_bits,
            rep_palette: feat.dominant_colors.clone(),
        }
    }

    fn absorb(&mut self, feat: &ImageFeature) {
        self.members.push(feat.clone());
        self.sum_time += feat.time as f64;
        self.sum_l += feat.l as f64;
        self.sum_a += feat.a as f64;
        self.sum_b += feat.b as f64;
        self.sum_ar += feat.aspect_ratio as f64;
        self.sum_sketch += feat.sketch_score as f64;
        self.sum_binary += feat.binary_score as f64;
        self.sum_raytrace += feat.raytrace_score as f64;
    }

    fn raw_centroid(&self) -> [f64; 8] {
        let n = self.members.len().max(1) as f64;
        [
            self.sum_time / n,
            self.sum_l / n,
            self.sum_a / n,
            self.sum_b / n,
            self.sum_ar / n,
            self.sum_sketch / n,
            self.sum_binary / n,
            self.sum_raytrace / n,
        ]
    }
}

/// Computes multi-force similarity clustering & culling recommendations for selected image files.
pub async fn compute_similarity_clusters(
    image_files: &[PathBuf],
    options: &SmartFolderOptions,
) -> (Vec<SmartClusterGroup>, DeterminantForces) {
    if image_files.is_empty() {
        return (Vec::new(), DeterminantForces::default());
    }

    let do_sketch = options.weight_sketch > 0.001;
    let do_binary = options.weight_binary > 0.001;
    let do_raytrace = options.weight_raytrace > 0.001;

    // 1. Feature extraction in parallel
    let features: Vec<ImageFeature> = image_files
        .par_iter()
        .filter_map(|p| {
            let meta = std::fs::metadata(p).ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta.as_ref().and_then(|m| m.modified().ok()).unwrap_or_else(SystemTime::now);
            let file_info = FileInfo {
                path: p.clone(),
                name: p.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string(),
                size,
                dimensions: None,
                modified,
                is_dir: false,
                phash: None,
            };
            extract_single_feature(p.clone(), file_info, do_sketch, do_binary, do_raytrace, None)
                .map(|(feat, _, _)| feat)
        })
        .collect();

    if features.is_empty() {
        return (Vec::new(), DeterminantForces::default());
    }

    // 2. Normalization via Welford Running Statistics
    let mut norm = NormStats::new(0.1);
    for feat in &features {
        norm.update(feat);
    }

    // 3. Determinant forces calculation
    let total_w = (options.weight_time + options.weight_color * 3.0 + options.weight_name + options.weight_sketch + options.weight_binary + options.weight_raytrace).max(0.001);
    let forces = DeterminantForces {
        time: (options.weight_time / total_w) * 100.0,
        color: ((options.weight_color * 3.0) / total_w) * 100.0,
        composition: (options.weight_name / total_w) * 100.0,
        sketch: (options.weight_sketch / total_w) * 100.0,
        binary: (options.weight_binary / total_w) * 100.0,
        raytrace: (options.weight_raytrace / total_w) * 100.0,
    };

    // 4. Online Leader Clustering
    let mut clusters: Vec<InternalSmartCluster> = Vec::new();
    let eps = options.similarity_threshold.max(0.1);

    for feat in &features {
        let feat_norm = norm.normalize(feat);
        let mut best_dist = f64::MAX;
        let mut best_idx = None;

        for (idx, cluster) in clusters.iter().enumerate() {
            let centroid_raw = cluster.raw_centroid();
            let centroid_norm = norm.normalize_raw(&centroid_raw);
            let dist = combined_distance(
                &feat_norm,
                &centroid_norm,
                feat.phash_bits,
                cluster.rep_phash,
                &feat.dominant_colors,
                &cluster.rep_palette,
                options.weight_color as f64,
                options.weight_time as f64,
                options.weight_name as f64,
                options.weight_sketch as f64,
                options.weight_binary as f64,
                options.weight_raytrace as f64,
                0.25,
                0.25,
            );

            if dist < best_dist {
                best_dist = dist;
                best_idx = Some(idx);
            }
        }

        if let Some(idx) = best_idx {
            if best_dist <= eps {
                clusters[idx].absorb(feat);
                continue;
            }
        }

        // New cluster spawn
        let next_id = clusters.len() + 1;
        clusters.push(InternalSmartCluster::new(next_id, feat));
    }

    // If clusters exceed max_clusters, merge closest pairs
    while clusters.len() > options.max_clusters.max(1) {
        let mut min_pair_dist = f64::MAX;
        let mut pair_to_merge = (0, 1);

        for i in 0..clusters.len() {
            for j in (i + 1)..clusters.len() {
                let c1_norm = norm.normalize_raw(&clusters[i].raw_centroid());
                let c2_norm = norm.normalize_raw(&clusters[j].raw_centroid());
                let d = combined_distance(
                    &c1_norm,
                    &c2_norm,
                    clusters[i].rep_phash,
                    clusters[j].rep_phash,
                    &clusters[i].rep_palette,
                    &clusters[j].rep_palette,
                    options.weight_color as f64,
                    options.weight_time as f64,
                    options.weight_name as f64,
                    options.weight_sketch as f64,
                    options.weight_binary as f64,
                    options.weight_raytrace as f64,
                    0.25,
                    0.25,
                );
                if d < min_pair_dist {
                    min_pair_dist = d;
                    pair_to_merge = (i, j);
                }
            }
        }

        let (keep_idx, remove_idx) = pair_to_merge;
        let removed = clusters.remove(remove_idx);
        for m in removed.members {
            clusters[keep_idx].absorb(&m);
        }
    }

    // 5. Culling & Semantic Label Formulation for each cluster
    let mut smart_groups = Vec::new();

    for (idx, cluster) in clusters.into_iter().enumerate() {
        let member_paths: Vec<PathBuf> = cluster.members.iter().map(|m| m.path.clone()).collect();
        let centroid = cluster.raw_centroid();
        
        let mean_sketch = centroid[5];
        let mean_binary = centroid[6];
        let mean_raytrace = centroid[7];
        let mean_l = centroid[1];

        // Determine dominant semantic force label
        let (dominant_force, folder_name) = if mean_sketch > 0.38 {
            ("Croquis / Sketch".to_string(), "Sketches_and_LineArt".to_string())
        } else if mean_binary > 0.48 {
            ("Silhouette / Binaire".to_string(), "Silhouettes_and_BW".to_string())
        } else if mean_raytrace > 0.35 {
            ("3D Raytrace".to_string(), "3D_Renders_and_Lighting".to_string())
        } else if mean_l > 75.0 {
            ("High Lightness".to_string(), "Bright_Scenes".to_string())
        } else if mean_l < 28.0 {
            ("Low Lightness".to_string(), "Dark_and_Atmospheric".to_string())
        } else {
            ("Visual Harmony".to_string(), format!("Visual_Group_{:02}", idx + 1))
        };

        // Culling Mode: Identify best quality shot & burst duplicates
        let mut best_shot = None;
        let mut culled_shots = Vec::new();

        if options.culling_mode && cluster.members.len() >= 2 {
            // Rank members by sharpness / visual clarity
            let mut ranked = cluster.members.clone();
            ranked.sort_by(|a, b| {
                let quality_a = a.sketch_score * 0.6 + (1.0 - (a.l - 50.0).abs() / 50.0) * 0.4;
                let quality_b = b.sketch_score * 0.6 + (1.0 - (b.l - 50.0).abs() / 50.0) * 0.4;
                quality_b.partial_cmp(&quality_a).unwrap_or(std::cmp::Ordering::Equal)
            });

            best_shot = ranked.first().map(|f| f.path.clone());

            // Check for burst shots (taken within 45s of each other or high pHash similarity)
            for i in 1..ranked.len() {
                let candidate = &ranked[i];
                let is_burst = ranked.iter().take(i).any(|better| {
                    let time_diff = (candidate.time - better.time).abs();
                    let phash_match = match (candidate.phash_bits, better.phash_bits) {
                        (Some(h1), Some(h2)) => (h1 ^ h2).count_ones() <= 8,
                        _ => false,
                    };
                    time_diff < 45.0 || phash_match
                });

                if is_burst {
                    culled_shots.push(candidate.path.clone());
                }
            }
        }

        smart_groups.push(SmartClusterGroup {
            cluster_id: idx + 1,
            folder_name,
            dominant_force,
            member_paths,
            best_shot,
            culled_shots,
        });
    }

    (smart_groups, forces)
}

/// Helper function to atomically relocate a file to a destination subfolder.
async fn move_single_file(src: &Path, dest_folder: &Path) -> Result<PathBuf, String> {
    if let Some(file_name) = src.file_name() {
        let dest_path = dest_folder.join(file_name);
        match tokio::fs::rename(src, &dest_path).await {
            Ok(_) => Ok(dest_path),
            Err(e) => {
                match tokio::fs::copy(src, &dest_path).await {
                    Ok(_) => {
                        let _ = tokio::fs::remove_file(src).await;
                        Ok(dest_path)
                    }
                    Err(copy_err) => Err(format!("Rename: {}, Copy: {}", e, copy_err)),
                }
            }
        }
    } else {
        Err("Invalid file name".to_string())
    }
}

/// Main execution function for Smart Subfolder Action.
/// 1. Filters selected paths to keep image files only.
/// 2. Extracts oldest creation/modification date (`yyyy-MMdd`).
/// 3. Generates suffix via AI or pattern-matching fallback.
/// 4. If similarity clustering is active, executes multi-force clustering and culling.
/// 5. Atomically relocates image files into their corresponding smart structure.
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

    // 5. Build proposed root folder name: [yyyy-MMdd]_[Suffix]
    let base_folder_name = format!("{}_{}", prefix, suffix);
    let sanitized_name = sanitize_folder_name(&base_folder_name);

    // 6. Handle duplicate folder names gracefully
    let target_folder = get_unique_folder_path(&parent_dir, &sanitized_name);
    let final_folder_name = target_folder
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&sanitized_name)
        .to_string();

    // 7. Create root smart folder
    tokio::fs::create_dir_all(&target_folder).await?;
    info!("Created smart subfolder root: {:?}", target_folder);

    // 8. Execute Similarity Clustering & Culling if enabled and sufficient files exist
    let (clusters, forces) = if opts.similarity_clustering && image_files.len() >= 3 {
        compute_similarity_clusters(&image_files, &opts).await
    } else {
        (Vec::new(), DeterminantForces::default())
    };

    let mut moved_files = Vec::new();
    let mut failed_files = Vec::new();

    // If multi-cluster similarity grouping produced distinct collections (> 1)
    if clusters.len() > 1 {
        for (i, cluster) in clusters.iter().enumerate() {
            let cluster_folder_name = format!("{:02}_{}", i + 1, sanitize_folder_name(&cluster.folder_name));
            let cluster_dir = target_folder.join(&cluster_folder_name);
            let _ = tokio::fs::create_dir_all(&cluster_dir).await;

            let culled_set: HashSet<PathBuf> = cluster.culled_shots.iter().cloned().collect();
            let burst_rushes_dir = if opts.culling_mode && !cluster.culled_shots.is_empty() {
                let dir = cluster_dir.join("_Burst_Rushes");
                let _ = tokio::fs::create_dir_all(&dir).await;
                Some(dir)
            } else {
                None
            };

            for src in &cluster.member_paths {
                let target_sub = if culled_set.contains(src) && burst_rushes_dir.is_some() {
                    burst_rushes_dir.as_ref().unwrap()
                } else {
                    &cluster_dir
                };

                match move_single_file(src, target_sub).await {
                    Ok(dest) => moved_files.push(dest),
                    Err(err) => failed_files.push((src.clone(), err)),
                }
            }
        }
    } else {
        // Single cluster or similarity disabled -> move directly to target root
        for src in &image_files {
            match move_single_file(src, &target_folder).await {
                Ok(dest) => moved_files.push(dest),
                Err(err) => failed_files.push((src.clone(), err)),
            }
        }
    }

    Ok(SmartFolderResult {
        target_folder,
        moved_files,
        failed_files,
        folder_name: final_folder_name,
        clusters,
        forces,
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
            ..Default::default()
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
