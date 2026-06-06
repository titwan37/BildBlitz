use std::path::PathBuf;
use tokio::sync::mpsc;
use rayon::prelude::*;
use palette::IntoColor;

use crate::messages::{AutoGroupConfig, AutoGroupProgress, AutoGroupResult, Cluster};
use crate::engine::gallery::GalleryScanner;

// ── Constants ─────────────────────────────────────────────────────────────────

const DOMINANT_COLOR_COUNT: usize = 8;
const STREAM_EMIT_EVERY: usize = 4; // Emit even more frequently for "immediate" feel

// ── Feature Vector ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ImageFeature {
    pub path: PathBuf,
    pub time: f32,
    pub l: f32,
    pub a: f32,
    pub b: f32,
    pub aspect_ratio: f32,
    /// Raw 64-bit perceptual hash for Hamming distance comparison.
    pub phash_bits: Option<u64>,
    /// 8 main colors in Lab space, sorted by luminance.
    pub dominant_colors: Vec<[f32; 3]>,
}

// ── Welford Running Statistics (Online Z-Score normalization) ─────────────────

// Helper: select three champion images to bootstrap statistics
pub(crate) fn select_champions(paths: &[PathBuf]) -> Vec<PathBuf> {
    if paths.is_empty() {
        return vec![];
    }
    // Sort by modification time (fallback to filename order)
    let mut sorted = paths.to_vec();
    sorted.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default())
    });
    let oldest = sorted.first().cloned().unwrap();
    let newest = sorted.last().cloned().unwrap();
    let median = sorted.get(sorted.len() / 2).cloned().unwrap_or_else(|| oldest.clone());
    vec![oldest, newest, median]
}

#[derive(Clone, Default)]
pub(crate) struct WelfordStat {
    pub(crate) count: f64,
    pub(crate) mean: f64,
    pub(crate) m2: f64,
    pub(crate) min_variance: f64, // clamp to avoid zero std dev
}

impl WelfordStat {
    fn update(&mut self, x: f64) {
        self.count += 1.0;
        let delta = x - self.mean;
        self.mean += delta / self.count;
        let delta2 = x - self.mean;
        self.m2 += delta * delta2;
    }

    pub(crate) fn std_dev(&self) -> f64 {
        if self.count < 2.0 {
            1.0
        } else {
            let raw = (self.m2 / self.count).sqrt();
            raw.max(self.min_variance.max(1e-9))
        }
    }

    fn z_score(&self, x: f64) -> f64 {
        (x - self.mean) / self.std_dev()
    }
}

#[derive(Clone, Default)]
pub(crate) struct NormStats {
    pub(crate) time: WelfordStat,
    pub(crate) l: WelfordStat,
    pub(crate) a: WelfordStat,
    pub(crate) b: WelfordStat,
    pub(crate) aspect_ratio: WelfordStat,
}

impl NormStats {
    pub(crate) fn new(min_var: f64) -> Self {
        Self {
            time: WelfordStat { min_variance: min_var, ..Default::default() },
            l: WelfordStat { min_variance: min_var, ..Default::default() },
            a: WelfordStat { min_variance: min_var, ..Default::default() },
            b: WelfordStat { min_variance: min_var, ..Default::default() },
            aspect_ratio: WelfordStat { min_variance: min_var, ..Default::default() },
        }
    }

    pub(crate) fn update(&mut self, f: &ImageFeature) {
        self.time.update(f.time as f64);
        self.l.update(f.l as f64);
        self.a.update(f.a as f64);
        self.b.update(f.b as f64);
        self.aspect_ratio.update(f.aspect_ratio as f64);
    }

    /// Returns a 5-element normalized feature vector [time, l, a, b, aspect].
    pub(crate) fn normalize(&self, f: &ImageFeature) -> [f64; 5] {
        [
            self.time.z_score(f.time as f64),
            self.l.z_score(f.l as f64),
            self.a.z_score(f.a as f64),
            self.b.z_score(f.b as f64),
            self.aspect_ratio.z_score(f.aspect_ratio as f64),
        ]
    }
}

// ── Online Cluster ────────────────────────────────────────────────────────────

pub(crate) struct OnlineCluster {
    id: usize,
    /// Raw sums for calculating raw centroid.
    sum_time: f64,
    sum_l: f64,
    sum_a: f64,
    sum_b: f64,
    sum_ar: f64,
    count: usize,
    /// Representative PHash (from first member).
    rep_phash: Option<u64>,
    /// Representative palette (from first member).
    rep_palette: Vec<[f32; 3]>,
    members: Vec<PathBuf>,
    min_time: f32,
    max_time: f32,
}

impl OnlineCluster {
    fn new(id: usize, feat: &ImageFeature) -> Self {
        Self {
            id,
            sum_time: feat.time as f64,
            sum_l: feat.l as f64,
            sum_a: feat.a as f64,
            sum_b: feat.b as f64,
            sum_ar: feat.aspect_ratio as f64,
            count: 1,
            rep_phash: feat.phash_bits,
            rep_palette: feat.dominant_colors.clone(),
            members: vec![feat.path.clone()],
            min_time: feat.time,
            max_time: feat.time,
        }
    }

    fn absorb(&mut self, feat: &ImageFeature) {
        self.sum_time += feat.time as f64;
        self.sum_l += feat.l as f64;
        self.sum_a += feat.a as f64;
        self.sum_b += feat.b as f64;
        self.sum_ar += feat.aspect_ratio as f64;
        self.count += 1;
        self.members.push(feat.path.clone());
        if feat.time < self.min_time { self.min_time = feat.time; }
        if feat.time > self.max_time { self.max_time = feat.time; }
    }

    /// Returns the raw centroid vector.
    fn raw_centroid(&self) -> [f64; 5] {
        let n = self.count as f64;
        [
            self.sum_time / n,
            self.sum_l / n,
            self.sum_a / n,
            self.sum_b / n,
            self.sum_ar / n,
        ]
    }
}

// ── Distance Metric ───────────────────────────────────────────────────────────

/// Normalized Hamming distance between two 64-bit pHashes: [0.0 .. 1.0].
fn hamming_dist_norm(a: u64, b: u64) -> f64 {
    (a ^ b).count_ones() as f64 / 64.0
}

fn palette_distance(p1: &[[f32; 3]], p2: &[[f32; 3]]) -> f64 {
    if p1.is_empty() || p2.is_empty() { return 1.0; }
    let mut sum = 0.0;
    for (c1, c2) in p1.iter().zip(p2.iter()) {
        let d = (c1[0]-c2[0]).powi(2) + (c1[1]-c2[1]).powi(2) + (c1[2]-c2[2]).powi(2);
        sum += d.sqrt();
    }
    (sum / p1.len() as f32) as f64 / 100.0 // Normalize Lab dist
}

/// Combined distance: weighted Euclidean on continuous features + phash penalty.
/// `phash_weight` is how much a full hash mismatch counts relative to epsilon.
pub(crate) fn combined_distance(
    v1_norm: &[f64; 5],
    v2_norm: &[f64; 5],
    ph1: Option<u64>,
    ph2: Option<u64>,
    pal1: &[[f32; 3]],
    pal2: &[[f32; 3]],
    w_color: f64,
    w_time: f64,
    _w_name: f64,
    phash_weight: f64,
    palette_weight: f64,
) -> f64 {
    // Weighted Euclidean: dim 0 = time, dims 1-3 = color, dim 4 = aspect
    let weights = [w_time, w_color, w_color, w_color, 0.2]; 
    let sq_sum: f64 = v1_norm.iter().zip(v2_norm.iter()).zip(weights.iter())
        .map(|((a, b), w)| w * (a - b).powi(2))
        .sum();
    let euclidean = sq_sum.sqrt();

    // Hamming penalty (optional boost when both hashes available)
    let phash_penalty = match (ph1, ph2) {
        (Some(h1), Some(h2)) => hamming_dist_norm(h1, h2) * phash_weight,
        _ => 0.0,
    };

    let pal_dist = palette_distance(pal1, pal2) * palette_weight;

    euclidean + phash_penalty + pal_dist
}

// ── Online Clustering Manager ─────────────────────────────────────────────────

pub(crate) struct OnlineClusterManager {
    clusters: Vec<OnlineCluster>,
    next_id: usize,
    eps: f64,
    w_color: f64,
    w_time: f64,
    w_name: f64,
    phash_weight: f64,
    palette_weight: f64,
}

impl OnlineClusterManager {
    pub(crate) fn new(config: &AutoGroupConfig) -> Self {
        Self {
            clusters: Vec::new(),
            next_id: 1,
            eps: config.eps as f64,
            w_color: config.weight_color as f64,
            w_time: config.weight_time as f64,
            w_name: config.weight_name as f64,
            phash_weight: config.eps as f64 * 0.4 * (config.weight_name as f64).max(0.1),
            palette_weight: config.eps as f64 * 0.6 * (config.weight_color as f64).max(0.1),
        }
    }

    pub(crate) fn ingest(&mut self, feat: &ImageFeature, norm: &NormStats) -> usize {
        let feat_norm = norm.normalize(feat);
        
        let mut best_idx: Option<usize> = None;
        let mut best_dist = f64::MAX;

        for (i, cluster) in self.clusters.iter().enumerate() {
            // Normalize cluster raw centroid on the fly
            let c_raw = cluster.raw_centroid();
            let c_norm = [
                norm.time.z_score(c_raw[0]),
                norm.l.z_score(c_raw[1]),
                norm.a.z_score(c_raw[2]),
                norm.b.z_score(c_raw[3]),
                norm.aspect_ratio.z_score(c_raw[4]),
            ];

            let d = combined_distance(
                &feat_norm,
                &c_norm,
                feat.phash_bits,
                cluster.rep_phash,
                &feat.dominant_colors,
                &cluster.rep_palette,
                self.w_color,
                self.w_time,
                self.w_name,
                self.phash_weight,
                self.palette_weight,
            );
            if d < best_dist {
                best_dist = d;
                best_idx = Some(i);
            }
        }

        if best_dist <= self.eps {
            let idx = best_idx.unwrap();
            self.clusters[idx].absorb(feat);
            self.try_merge(idx, norm);
            idx
        } else {
            let id = self.next_id;
            self.next_id += 1;
            self.clusters.push(OnlineCluster::new(id, feat));
            self.clusters.len() - 1
        }
    }

    fn try_merge(&mut self, idx: usize, norm: &NormStats) {
        let merge_eps = self.eps * 1.2;
        let mut to_merge: Option<usize> = None;

        let c1_raw = self.clusters[idx].raw_centroid();
        let c1_norm = [
            norm.time.z_score(c1_raw[0]),
            norm.l.z_score(c1_raw[1]),
            norm.a.z_score(c1_raw[2]),
            norm.b.z_score(c1_raw[3]),
            norm.aspect_ratio.z_score(c1_raw[4]),
        ];

        for j in 0..self.clusters.len() {
            if j == idx { continue; }
            let c2_raw = self.clusters[j].raw_centroid();
            let c2_norm = [
                norm.time.z_score(c2_raw[0]),
                norm.l.z_score(c2_raw[1]),
                norm.a.z_score(c2_raw[2]),
                norm.b.z_score(c2_raw[3]),
                norm.aspect_ratio.z_score(c2_raw[4]),
            ];

            let d = combined_distance(
                &c1_norm,
                &c2_norm,
                self.clusters[idx].rep_phash,
                self.clusters[j].rep_phash,
                &self.clusters[idx].rep_palette,
                &self.clusters[j].rep_palette,
                self.w_color,
                self.w_time,
                self.w_name,
                self.phash_weight,
                self.palette_weight,
            );
            if d <= merge_eps {
                to_merge = Some(j);
                break;
            }
        }

        if let Some(j) = to_merge {
            let j_cluster = self.clusters.remove(j);
            let idx_adj = if j < idx { idx - 1 } else { idx };
            
            self.clusters[idx_adj].sum_time += j_cluster.sum_time;
            self.clusters[idx_adj].sum_l += j_cluster.sum_l;
            self.clusters[idx_adj].sum_a += j_cluster.sum_a;
            self.clusters[idx_adj].sum_b += j_cluster.sum_b;
            self.clusters[idx_adj].sum_ar += j_cluster.sum_ar;
            self.clusters[idx_adj].count += j_cluster.count;
            self.clusters[idx_adj].members.extend(j_cluster.members);
            if j_cluster.min_time < self.clusters[idx_adj].min_time { self.clusters[idx_adj].min_time = j_cluster.min_time; }
            if j_cluster.max_time > self.clusters[idx_adj].max_time { self.clusters[idx_adj].max_time = j_cluster.max_time; }
        }
    }

    pub(crate) fn finalize(self) -> Vec<Cluster> {
        let mut clusters: Vec<Cluster> = self.clusters.into_iter().map(|oc| {
            let count = oc.count as f32;
            let avg_l = (oc.sum_l / count as f64) as f32;
            let avg_a = (oc.sum_a / count as f64) as f32;
            let avg_b = (oc.sum_b / count as f64) as f32;
            let avg_ar = (oc.sum_ar / count as f64) as f32;
            let time_span_mins = (oc.max_time - oc.min_time) / 60.0;

            let label = if time_span_mins < 10.0 {
                "Burst / Moment"
            } else if time_span_mins < 120.0 {
                "Event"
            } else if avg_ar < 0.85 && avg_a > 5.0 && avg_b > 5.0 {
                "Portraits"
            } else if avg_ar > 1.2 && (avg_a < -5.0 || avg_b < -5.0) {
                "Landscapes"
            } else if avg_l > 80.0 {
                "Bright Scenes"
            } else if avg_l < 20.0 {
                "Dark Scenes"
            } else {
                "Visual Harmony"
            };

            Cluster { id: oc.id, members: oc.members, label: Some(label.to_string()) }
        }).collect();
        clusters.sort_by_key(|c| c.id);
        clusters
    }

    fn snapshot(&self) -> Vec<Cluster> {
        let mut out: Vec<Cluster> = self.clusters.iter().map(|oc| {
            Cluster {
                id: oc.id,
                members: oc.members.clone(),
                label: Some(format!("{} images", oc.members.len())),
            }
        }).collect();
        out.sort_by_key(|c| c.id);
        out
    }
}

// ── Color Quantization ────────────────────────────────────────────────────────

fn extract_dominant_colors(pixels: &[[f32; 3]], k: usize) -> Vec<[f32; 3]> {
    if pixels.is_empty() { return vec![[0.0, 0.0, 0.0]; k]; }
    
    // Initialize centroids by sampling
    let mut centroids: Vec<[f32; 3]> = pixels.iter().step_by((pixels.len() / k).max(1)).take(k).cloned().collect();
    while centroids.len() < k {
        centroids.push(pixels[0]);
    }

    for _ in 0..5 {
        let mut sums = vec![[0.0, 0.0, 0.0]; k];
        let mut counts = vec![0usize; k];

        for &p in pixels {
            let mut best_dist = f32::MAX;
            let mut best_idx = 0;
            for (i, &c) in centroids.iter().enumerate() {
                let d = (p[0]-c[0]).powi(2) + (p[1]-c[1]).powi(2) + (p[2]-c[2]).powi(2);
                if d < best_dist {
                    best_dist = d;
                    best_idx = i;
                }
            }
            sums[best_idx][0] += p[0];
            sums[best_idx][1] += p[1];
            sums[best_idx][2] += p[2];
            counts[best_idx] += 1;
        }

        for i in 0..k {
            if counts[i] > 0 {
                centroids[i][0] = sums[i][0] / counts[i] as f32;
                centroids[i][1] = sums[i][1] / counts[i] as f32;
                centroids[i][2] = sums[i][2] / counts[i] as f32;
            }
        }
    }
    
    centroids.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    centroids
}

// ── PHash Helper ──────────────────────────────────────────────────────────────

fn phash_to_bits(hash_b64: &str) -> Option<u64> {
    let bytes = base64_decode(hash_b64)?;
    if bytes.len() < 8 { return None; }
    Some(u64::from_be_bytes(bytes[..8].try_into().ok()?))
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [0u8; 256];
    for (i, &c) in alphabet.iter().enumerate() { lookup[c as usize] = i as u8; }
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        buf = (buf << 6) | (lookup[c as usize] as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8 & 0xFF);
        }
    }
    Some(out)
}

// ── Feature extraction helper ─────────────────────────────────────────────────

pub(crate) fn extract_single_feature(
    path: PathBuf,
    file: crate::engine::gallery::FileInfo,
) -> Option<(ImageFeature, String, crate::engine::gallery::FileInfo)> {
    let meta = crate::library::metadata::MetadataParser::extract_metadata(&path).ok()?;
    let time = meta.modified.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs_f32();

    let mut l_avg = 0.0f32;
    let mut a_avg = 0.0f32;
    let mut b_avg = 0.0f32;
    let mut aspect_ratio = 1.0f32;
    let mut all_pixels = Vec::with_capacity(32*32);

    if let Ok(img) = image::open(&path) {
        use image::GenericImageView;
        let (w, h) = img.dimensions();
        aspect_ratio = w as f32 / h.max(1) as f32;
        let thumb = img.resize_exact(32, 32, image::imageops::FilterType::Nearest);
        let rgb = thumb.to_rgb8();
        for pixel in rgb.pixels() {
            let srgb = palette::Srgb::new(pixel[0] as f32 / 255.0, pixel[1] as f32 / 255.0, pixel[2] as f32 / 255.0).into_linear();
            let lab: palette::Lab = srgb.into_color();
            l_avg += lab.l; a_avg += lab.a; b_avg += lab.b;
            all_pixels.push([lab.l, lab.a, lab.b]);
        }
        if !all_pixels.is_empty() {
            let n = all_pixels.len() as f32;
            l_avg /= n; a_avg /= n; b_avg /= n;
        }
    }

    let dominant_colors = extract_dominant_colors(&all_pixels, DOMINANT_COLOR_COUNT);
    let phash_b64 = crate::library::hash::compute_hash(&path).unwrap_or_default();
    let phash_bits = phash_to_bits(&phash_b64);

    Some((
        ImageFeature { path, time, l: l_avg, a: a_avg, b: b_avg, aspect_ratio, phash_bits, dominant_colors },
        phash_b64,
        file
    ))
}

// ── Public Streaming Entry Point ──────────────────────────────────────────────

pub async fn run_auto_group(
    config: AutoGroupConfig,
    progress_tx: mpsc::Sender<AutoGroupProgress>,
) -> anyhow::Result<AutoGroupResult> {
    // Scan directory and collect image files
    let files = GalleryScanner::scan_directory(&config.source_path).await;
    let mut images: Vec<_> = files.into_iter().filter(|f| !f.is_dir).collect();
    let total = images.len();
    if total == 0 {
        return Ok(AutoGroupResult { clusters: vec![], forces: (33.3, 33.3, 33.3) });
    }

    let _ = progress_tx.send(AutoGroupProgress::Extracted { done: 0, total }).await;

    // Database connection (used for metadata persistence)
    let db = crate::library::db::DatabaseManager::new().await?;
    let mut norm = NormStats::new(0.1);
    let mut manager = OnlineClusterManager::new(&config);
    // -------- Champion seeding --------
    // Build a list of all image paths for champion selection
    let all_paths: Vec<PathBuf> = images.iter().map(|f| f.path.clone()).collect();
    let champion_paths = select_champions(&all_paths);
    let mut done = 0usize;

    // Extract champion features synchronously and seed statistics / initial clusters
    for champ_path in champion_paths {
        if let Some(idx) = images.iter().position(|f| f.path == champ_path) {
            let champ_file = images.remove(idx);
            if let Some((feat, phash_b64, file)) = extract_single_feature(champ_path.clone(), champ_file) {
                // Persistent metadata for champions too
                let db_clone = db.clone();
                let hash_clone = if phash_b64.is_empty() { None } else { Some(phash_b64) };
                tokio::spawn(async move { let _ = db_clone.insert_image_metadata(file, hash_clone).await; });

                norm.update(&feat);
                manager.ingest(&feat, &norm);
                done += 1;
                let _ = progress_tx.send(AutoGroupProgress::Extracted { done, total }).await;
            }
        }
    }

    // Database connection (used for metadata persistence)
    let db = crate::library::db::DatabaseManager::new().await?;
    let (tx, mut rx) = mpsc::channel(32);
    let rt_handle = tokio::runtime::Handle::current();

    // Spawn parallel extraction for the remaining images
    rayon::spawn(move || {
        images.into_par_iter().for_each(|file| {
            let path = file.path.clone();
            if let Some(res) = extract_single_feature(path, file) {
                let _ = rt_handle.block_on(tx.send(res));
            }
        });
    });

    // Process streamed feature results
    while let Some((feat, phash_b64, file)) = rx.recv().await {
        let db_clone = db.clone();
        let hash_clone = if phash_b64.is_empty() { None } else { Some(phash_b64) };
        tokio::spawn(async move { let _ = db_clone.insert_image_metadata(file, hash_clone).await; });

        norm.update(&feat);
        manager.ingest(&feat, &norm);
        done += 1;

        let _ = progress_tx.send(AutoGroupProgress::Extracted { done, total }).await;

        if done % STREAM_EMIT_EVERY == 0 || done == total {
            let pct = done as f32 / total as f32 * 100.0;
            let _ = progress_tx.send(AutoGroupProgress::Clustering { percent: pct }).await;
            let snapshot = manager.snapshot();
            let _ = progress_tx.send(AutoGroupProgress::VirtualClustersUpdated { clusters: snapshot }).await;
        }
    }

    let final_clusters = manager.finalize();

    // ── Determinant Force Calculation ─────────────────────────────────────────
    // Variance of each Welford stat = m2 / count (population variance).
    // We map this to the three user-facing dimensions:
    //   time         → norm.time variance
    //   color        → sum of L, A, B variances (Lab channels)
    //   palette/phash→ aspect_ratio variance (proxy for composition)
    let var_time = if norm.time.count > 1.0 { norm.time.m2 / norm.time.count } else { 0.0 };
    let var_l    = if norm.l.count > 1.0    { norm.l.m2    / norm.l.count    } else { 0.0 };
    let var_a    = if norm.a.count > 1.0    { norm.a.m2    / norm.a.count    } else { 0.0 };
    let var_b    = if norm.b.count > 1.0    { norm.b.m2    / norm.b.count    } else { 0.0 };
    let _var_ar   = if norm.aspect_ratio.count > 1.0 { norm.aspect_ratio.m2 / norm.aspect_ratio.count } else { 0.0 };

    // The UI 'Determinant Forces' should represent the influence of the weights
    // adjusted for the fact that features are normalized to unit variance during clustering.
    let force_time    = config.weight_time as f64;
    let force_color   = config.weight_color as f64;
    let force_palette = 0.2; // Constant background weight for composition

    let total_force = force_time + force_color + force_palette;
    let forces = if total_force > 1e-9 {
        (
            (force_time    / total_force * 100.0) as f32,
            (force_color   / total_force * 100.0) as f32,
            (force_palette / total_force * 100.0) as f32,
        )
    } else {
        (33.3, 33.3, 33.3)
    };

    Ok(AutoGroupResult { clusters: final_clusters, forces })
}

// ── Auto-Tune ────────────────────────────────────────────────────────────────

pub async fn run_auto_tune_epsilon(
    config: AutoGroupConfig,
    progress_tx: mpsc::Sender<AutoGroupProgress>,
) -> anyhow::Result<crate::messages::AutoGroupTuneResult> {
    let files = GalleryScanner::scan_directory(&config.source_path).await;
    let images: Vec<_> = files.into_iter().filter(|f| !f.is_dir).collect();
    let total = images.len();
    if total < config.min_samples + 1 { return Err(anyhow::anyhow!("Not enough images")); }

    let _ = progress_tx.send(AutoGroupProgress::Extracted { done: 0, total }).await;

    // For tuning, we still collect all features first to build a global distance matrix
    let features: Vec<ImageFeature> = images.into_par_iter()
        .filter_map(|file| extract_single_feature(file.path.clone(), file).map(|(f, _, _)| f))
        .collect();

    let mut norm = NormStats::default();
    for f in &features { norm.update(f); }

    let vecs: Vec<[f64; 5]> = features.iter().map(|f| norm.normalize(f)).collect();
    let mut k_dists: Vec<f64> = Vec::with_capacity(features.len());

    for (i, v1) in vecs.iter().enumerate() {
        let mut dists: Vec<f64> = vecs.iter().enumerate().filter(|(j, _)| *j != i).map(|(j, v2)| {
            let phash_weight = config.eps as f64 * 0.4 * (config.weight_name as f64).max(0.1);
            let palette_weight = config.eps as f64 * 0.6 * (config.weight_color as f64).max(0.1);
            combined_distance(
                v1, v2, 
                features[i].phash_bits, features[j].phash_bits, 
                &features[i].dominant_colors, &features[j].dominant_colors,
                config.weight_color as f64, 
                config.weight_time as f64, 
                config.weight_name as f64,
                phash_weight, 
                palette_weight
            )
        }).collect();
        dists.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if let Some(&kd) = dists.get(config.min_samples.saturating_sub(1)) { k_dists.push(kd); }
    }

    k_dists.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let dn = k_dists.len();
    let optimal_eps = if dn < 3 { 0.1 } else {
        let (x1, y1) = (0.0, k_dists[0]);
        let (x2, y2) = ((dn-1) as f64, k_dists[dn-1]);
        let (mut max_d, mut elbow) = (-1.0, 0);
        for i in 0..dn {
            let (x0, y0) = (i as f64, k_dists[i]);
            let d = ((y2 - y1) * x0 - (x2 - x1) * y0 + x2 * y1 - y2 * x1).abs() / ((y2 - y1).powi(2) + (x2 - x1).powi(2)).sqrt();
            if d > max_d { max_d = d; elbow = i; }
        }
        k_dists[elbow]
    };

    Ok(crate::messages::AutoGroupTuneResult { optimal_eps: optimal_eps as f32 })
}

// ── Commit ───────────────────────────────────────────────────────────────────

pub async fn commit_auto_group(
    result: AutoGroupResult,
    source_path: std::path::PathBuf,
    progress_tx: mpsc::Sender<AutoGroupProgress>,
) -> anyhow::Result<()> {
    // Compute total files considering only clusters that meet the minimum sample size (4 images).
    let min_cluster_size = 4usize;
    let filtered_clusters: Vec<_> = result.clusters
        .into_iter()
        .filter(|c| c.members.len() >= min_cluster_size)
        .collect();
    let total_files: usize = filtered_clusters.iter().map(|c| c.members.len()).sum();
    let mut moved = 0usize;

    for cluster in filtered_clusters {
        // Use existing naming scheme (0 = Uncategorized, others = Theme_<id>)
        let folder_name = if cluster.id == 0 {
            "Uncategorized".to_string()
        } else {
            format!("Theme_{}", cluster.id)
        };
        let target_dir = source_path.join(&folder_name);
        if !target_dir.exists() {
            tokio::fs::create_dir_all(&target_dir).await?;
        }
        for path in cluster.members {
            if let Some(file_name) = path.file_name() {
                let dest = target_dir.join(file_name);
                if tokio::fs::rename(&path, &dest).await.is_ok() {
                    moved += 1;
                    let _ = progress_tx
                        .send(AutoGroupProgress::Moving { done: moved, total: total_files })
                        .await;
                }
            }
        }
    }
    Ok(())
}
