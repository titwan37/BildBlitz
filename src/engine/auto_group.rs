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
    // --- New Rendering Profile Scores ---
    pub sketch_score: f32,
    pub binary_score: f32,
    pub raytrace_score: f32,
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
    pub(crate) sketch: WelfordStat,
    pub(crate) binary: WelfordStat,
    pub(crate) raytrace: WelfordStat,
}

impl NormStats {
    pub(crate) fn new(min_var: f64) -> Self {
        Self {
            time: WelfordStat { min_variance: min_var, ..Default::default() },
            l: WelfordStat { min_variance: min_var, ..Default::default() },
            a: WelfordStat { min_variance: min_var, ..Default::default() },
            b: WelfordStat { min_variance: min_var, ..Default::default() },
            aspect_ratio: WelfordStat { min_variance: min_var, ..Default::default() },
            sketch: WelfordStat { min_variance: min_var, ..Default::default() },
            binary: WelfordStat { min_variance: min_var, ..Default::default() },
            raytrace: WelfordStat { min_variance: min_var, ..Default::default() },
        }
    }

    pub(crate) fn update(&mut self, f: &ImageFeature) {
        self.time.update(f.time as f64);
        self.l.update(f.l as f64);
        self.a.update(f.a as f64);
        self.b.update(f.b as f64);
        self.aspect_ratio.update(f.aspect_ratio as f64);
        self.sketch.update(f.sketch_score as f64);
        self.binary.update(f.binary_score as f64);
        self.raytrace.update(f.raytrace_score as f64);
    }

    /// Returns a 8-element normalized feature vector [time, l, a, b, aspect, sketch, binary, raytrace].
    pub(crate) fn normalize(&self, f: &ImageFeature) -> [f64; 8] {
        [
            self.time.z_score(f.time as f64),
            self.l.z_score(f.l as f64),
            self.a.z_score(f.a as f64),
            self.b.z_score(f.b as f64),
            self.aspect_ratio.z_score(f.aspect_ratio as f64),
            self.sketch.z_score(f.sketch_score as f64),
            self.binary.z_score(f.binary_score as f64),
            self.raytrace.z_score(f.raytrace_score as f64),
        ]
    }

    /// Returns a 8-element normalized vector from a raw continuous array.
    pub(crate) fn normalize_raw(&self, raw: &[f64; 8]) -> [f64; 8] {
        [
            self.time.z_score(raw[0]),
            self.l.z_score(raw[1]),
            self.a.z_score(raw[2]),
            self.b.z_score(raw[3]),
            self.aspect_ratio.z_score(raw[4]),
            self.sketch.z_score(raw[5]),
            self.binary.z_score(raw[6]),
            self.raytrace.z_score(raw[7]),
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
    sum_sketch: f64,
    sum_binary: f64,
    sum_raytrace: f64,
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
            sum_sketch: feat.sketch_score as f64,
            sum_binary: feat.binary_score as f64,
            sum_raytrace: feat.raytrace_score as f64,
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
        self.sum_sketch += feat.sketch_score as f64;
        self.sum_binary += feat.binary_score as f64;
        self.sum_raytrace += feat.raytrace_score as f64;
        self.count += 1;
        self.members.push(feat.path.clone());
        if feat.time < self.min_time { self.min_time = feat.time; }
        if feat.time > self.max_time { self.max_time = feat.time; }
    }

    /// Returns the raw centroid vector.
    fn raw_centroid(&self) -> [f64; 8] {
        let n = self.count as f64;
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
    v1_norm: &[f64; 8],
    v2_norm: &[f64; 8],
    ph1: Option<u64>,
    ph2: Option<u64>,
    pal1: &[[f32; 3]],
    pal2: &[[f32; 3]],
    w_color: f64,
    w_time: f64,
    _w_name: f64,
    w_sketch: f64,
    w_binary: f64,
    w_raytrace: f64,
    phash_weight: f64,
    palette_weight: f64,
) -> f64 {
    // Weighted Euclidean: dim 0 = time, dims 1-3 = color, dim 4 = aspect, dim 5-7 = rendering
    let weights = [w_time, w_color, w_color, w_color, 0.2, w_sketch, w_binary, w_raytrace]; 
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
    w_sketch: f64,
    w_binary: f64,
    w_raytrace: f64,
    phash_weight: f64,
    palette_weight: f64,
    
    // Mini-batch Tensor Acceleration State
    tensor_engine: crate::engine::tensor_backend::TensorEngine,
    batch_buffer: Vec<ImageFeature>,
    batch_size: usize,
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
            w_sketch: config.weight_sketch as f64,
            w_binary: config.weight_binary as f64,
            w_raytrace: config.weight_raytrace as f64,
            phash_weight: config.eps as f64 * 0.4 * (config.weight_name as f64).max(0.1),
            palette_weight: config.eps as f64 * 0.6 * (config.weight_color as f64).max(0.1),
            tensor_engine: crate::engine::tensor_backend::TensorEngine::init(),
            batch_buffer: Vec::with_capacity(512),
            batch_size: 512,
        }
    }

    /// Adds an image feature to the mini-batch buffer. If the buffer is full, 
    /// processes the batch using the TensorEngine's fast matrix distance.
    pub(crate) fn ingest_batch(&mut self, feat: &ImageFeature, norm: &NormStats) -> Option<Vec<usize>> {
        self.batch_buffer.push(feat.clone());
        if self.batch_buffer.len() >= self.batch_size {
            return Some(self.flush_batch(norm));
        }
        None
    }

    /// Flushes the current batch buffer, computing GEMM distances to all centroids.
    pub(crate) fn flush_batch(&mut self, norm: &NormStats) -> Vec<usize> {
        if self.batch_buffer.is_empty() { return vec![]; }
        if self.clusters.is_empty() {
            // First item edge case
            let mut ids = vec![];
            let items = std::mem::take(&mut self.batch_buffer);
            for f in items {
                ids.push(self.ingest(&f, norm));
            }
            return ids;
        }

        let num_items = self.batch_buffer.len();
        let num_centroids = self.clusters.len();
        let dim = 8;

        // A Matrix: [N, D]
        let mut features_matrix = Vec::with_capacity(num_items * dim);
        for feat in &self.batch_buffer {
            let n = norm.normalize(feat);
            features_matrix.extend_from_slice(&[
                n[0] as f32, n[1] as f32, n[2] as f32, n[3] as f32, 
                n[4] as f32, n[5] as f32, n[6] as f32, n[7] as f32
            ]);
        }

        // B Matrix: [K, D]
        let mut centroids_matrix = Vec::with_capacity(num_centroids * dim);
        for cluster in &self.clusters {
            let c = cluster.raw_centroid();
            centroids_matrix.extend_from_slice(&[
                norm.time.z_score(c[0]) as f32,
                norm.l.z_score(c[1]) as f32,
                norm.a.z_score(c[2]) as f32,
                norm.b.z_score(c[3]) as f32,
                norm.aspect_ratio.z_score(c[4]) as f32,
                norm.sketch.z_score(c[5]) as f32,
                norm.binary.z_score(c[6]) as f32,
                norm.raytrace.z_score(c[7]) as f32,
            ]);
        }

        // C Matrix: [N, K] - Fast GEMM Cosine Distances
        let _distances = self.tensor_engine.compute_pairwise_distances(
            &features_matrix, num_items,
            &centroids_matrix, num_centroids,
            dim
        );

        // NOTE: In full implementation, we process the C matrix to apply the pHash/Palette penalty.
        // For this milestone, we fallback to sequential routing since the matrices are calculated.
        let mut ids = vec![];
        let items = std::mem::take(&mut self.batch_buffer);
        for f in items {
            ids.push(self.ingest(&f, norm));
        }
        ids
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
                norm.sketch.z_score(c_raw[5]),
                norm.binary.z_score(c_raw[6]),
                norm.raytrace.z_score(c_raw[7]),
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
                self.w_sketch,
                self.w_binary,
                self.w_raytrace,
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
            norm.sketch.z_score(c1_raw[5]),
            norm.binary.z_score(c1_raw[6]),
            norm.raytrace.z_score(c1_raw[7]),
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
                norm.sketch.z_score(c2_raw[5]),
                norm.binary.z_score(c2_raw[6]),
                norm.raytrace.z_score(c2_raw[7]),
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
                self.w_sketch,
                self.w_binary,
                self.w_raytrace,
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

pub fn phash_to_bits_pub(hash_b64: &str) -> Option<u64> {
    phash_to_bits(hash_b64)
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

// ── Performance Telemetry Accumulator ─────────────────────────────────────────

#[derive(Default, Clone)]
pub struct TimingsAccumulator {
    pub decode_us: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub color_us: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub phash_us: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub sketch_us: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub binary_us: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub raytrace_us: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

// ── Feature extraction helper ─────────────────────────────────────────────────

pub(crate) fn extract_single_feature(
    path: PathBuf,
    file: crate::engine::gallery::FileInfo,
    do_sketch: bool,
    do_binary: bool,
    do_raytrace: bool,
    timings: Option<&TimingsAccumulator>,
) -> Option<(ImageFeature, String, crate::engine::gallery::FileInfo)> {
    use std::sync::atomic::Ordering;

    let time = file.modified.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs_f32();

    let mut l_avg = 0.0f32;
    let mut a_avg = 0.0f32;
    let mut b_avg = 0.0f32;
    let mut aspect_ratio = 1.0f32;
    let mut all_pixels = Vec::with_capacity(32*32);
    
    let mut sketch_score = 0.0;
    let mut binary_score = 0.0;
    let mut raytrace_score = 0.0;

    // ── Tier 1 & 2 Fast Decoder (EXIF Thumb / SIMD zune-jpeg / Buffered Fallback) ─
    let t_dec = std::time::Instant::now();
    let fast_decoded = crate::library::fast_decode::FastImageDecoder::decode_fast(&path);
    if let Some(t) = timings {
        t.decode_us.fetch_add(t_dec.elapsed().as_micros() as u64, Ordering::Relaxed);
    }

    if let Ok(decoded) = fast_decoded {
        use image::GenericImageView;
        let img = decoded.image;
        let (w, h) = img.dimensions();
        aspect_ratio = w as f32 / h.max(1) as f32;
        
        // 1. 32x32 Thumbnail for fast color grouping
        let t_col = std::time::Instant::now();
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
        if let Some(t) = timings {
            t.color_us.fetch_add(t_col.elapsed().as_micros() as u64, Ordering::Relaxed);
        }

        // 2. 128x128 Thumbnail for Geometric & Rendering Profile (short-circuited if disabled)
        if do_sketch || do_binary || do_raytrace {
            let render_thumb = img.resize_exact(128, 128, image::imageops::FilterType::Triangle);
            let render_rgb = render_thumb.to_rgb8();
            let raw_pixels = render_rgb.into_raw();

            if do_sketch {
                let t_s = std::time::Instant::now();
                sketch_score = compute_sketch_score(&raw_pixels, 128, 128) as f32;
                if let Some(t) = timings {
                    t.sketch_us.fetch_add(t_s.elapsed().as_micros() as u64, Ordering::Relaxed);
                }
            }
            if do_binary {
                let t_b = std::time::Instant::now();
                binary_score = compute_binary_score(&raw_pixels, 128, 128) as f32;
                if let Some(t) = timings {
                    t.binary_us.fetch_add(t_b.elapsed().as_micros() as u64, Ordering::Relaxed);
                }
            }
            if do_raytrace {
                let t_r = std::time::Instant::now();
                raytrace_score = compute_raytrace_score(&raw_pixels, 128, 128) as f32;
                if let Some(t) = timings {
                    t.raytrace_us.fetch_add(t_r.elapsed().as_micros() as u64, Ordering::Relaxed);
                }
            }
        }
    }

    let dominant_colors = extract_dominant_colors(&all_pixels, DOMINANT_COLOR_COUNT);
    
    let t_ph = std::time::Instant::now();
    let phash_b64 = crate::library::hash::compute_hash(&path).unwrap_or_default();
    if let Some(t) = timings {
        t.phash_us.fetch_add(t_ph.elapsed().as_micros() as u64, Ordering::Relaxed);
    }
    let phash_bits = phash_to_bits(&phash_b64);

    Some((
        ImageFeature { 
            path, time, l: l_avg, a: a_avg, b: b_avg, aspect_ratio, 
            phash_bits, dominant_colors,
            sketch_score, binary_score, raytrace_score
        },
        phash_b64,
        file
    ))
}

// ── Public Streaming Entry Point ──────────────────────────────────────────────

pub async fn run_auto_group(
    config: AutoGroupConfig,
    progress_tx: mpsc::Sender<AutoGroupProgress>,
) -> anyhow::Result<AutoGroupResult> {
    let start_overall = std::time::Instant::now();

    // Scan directory and collect image files
    let files = GalleryScanner::scan_directory(&config.source_path).await;
    let images: Vec<_> = files.into_iter().filter(|f| !f.is_dir).collect();
    let total = images.len();
    if total == 0 {
        return Ok(AutoGroupResult { clusters: vec![], forces: Default::default(), perf: None });
    }

    let _ = progress_tx.send(AutoGroupProgress::Extracted { done: 0, total }).await;

    let do_sketch = config.weight_sketch > 0.001;
    let do_binary = config.weight_binary > 0.001;
    let do_raytrace = config.weight_raytrace > 0.001;

    let timings_acc = TimingsAccumulator::default();

    // Database connection (used for metadata & feature cache persistence)
    let db = crate::library::db::DatabaseManager::new().await?;
    let mut norm = NormStats::new(0.1);
    let mut manager = OnlineClusterManager::new(&config);
    let mut done = 0usize;

    // ── Tier 0: Check SQLite Cached Features First ────────────────────────────
    let mut uncached_images = Vec::with_capacity(total);
    for file in images {
        let mod_ts = file.modified.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        if let Ok(Some((cached_feat, _cached_phash))) = db.get_cached_feature(&file.path, mod_ts).await {
            norm.update(&cached_feat);
            manager.ingest(&cached_feat, &norm);
            done += 1;
            let _ = progress_tx.send(AutoGroupProgress::Extracted { done, total }).await;
        } else {
            uncached_images.push(file);
        }
    }

    // ── Extract Remaining Uncached Images in Parallel ──────────────────────────
    if !uncached_images.is_empty() {
        let (tx, mut rx) = mpsc::channel(32);
        let rt_handle = tokio::runtime::Handle::current();

        let timings_clone = timings_acc.clone();
        rayon::spawn(move || {
            uncached_images.into_par_iter().for_each(|file| {
                let path = file.path.clone();
                if let Some(res) = extract_single_feature(path, file, do_sketch, do_binary, do_raytrace, Some(&timings_clone)) {
                    let _ = rt_handle.block_on(tx.send(res));
                }
            });
        });

        // Process streamed feature results and persist full cache
        while let Some((feat, phash_b64, file)) = rx.recv().await {
            let db_clone = db.clone();
            let feat_clone = feat.clone();
            let hash_clone = phash_b64.clone();
            tokio::spawn(async move {
                let _ = db_clone.insert_full_feature(&file, &feat_clone, &hash_clone).await;
            });

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
    }

    let start_clustering = std::time::Instant::now();
    let clustering_ms = start_clustering.elapsed().as_secs_f64() * 1000.0;
    let final_clusters = manager.finalize();

    // ── Determinant Force Calculation (6 Dimensions) ──────────────────────────
    let force_time     = config.weight_time as f64;
    let force_color    = config.weight_color as f64;
    let force_comp     = 0.2 + (config.weight_name as f64 * 0.4);
    let force_sketch   = config.weight_sketch as f64;
    let force_binary   = config.weight_binary as f64;
    let force_raytrace = config.weight_raytrace as f64;

    let total_force = force_time + force_color + force_comp + force_sketch + force_binary + force_raytrace;
    let forces = if total_force > 1e-9 {
        crate::messages::DeterminantForces {
            time: (force_time / total_force * 100.0) as f32,
            color: (force_color / total_force * 100.0) as f32,
            composition: (force_comp / total_force * 100.0) as f32,
            sketch: (force_sketch / total_force * 100.0) as f32,
            binary: (force_binary / total_force * 100.0) as f32,
            raytrace: (force_raytrace / total_force * 100.0) as f32,
        }
    } else {
        crate::messages::DeterminantForces {
            time: 16.6, color: 16.6, composition: 16.6,
            sketch: 16.6, binary: 16.6, raytrace: 16.6,
        }
    };

    let total_elapsed_ms = start_overall.elapsed().as_secs_f64() * 1000.0;
    let perf = Some(crate::messages::PerformanceProfile {
        total_elapsed_ms,
        total_images: total,
        images_per_sec: total as f64 / (total_elapsed_ms / 1000.0).max(0.001),
        decode_ms: timings_acc.decode_us.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0,
        color_extract_ms: timings_acc.color_us.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0,
        phash_ms: timings_acc.phash_us.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0,
        sketch_ms: timings_acc.sketch_us.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0,
        binary_ms: timings_acc.binary_us.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0,
        raytrace_ms: timings_acc.raytrace_us.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0,
        clustering_ms,
    });

    Ok(AutoGroupResult { clusters: final_clusters, forces, perf })
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

    let do_sketch = config.weight_sketch > 0.001;
    let do_binary = config.weight_binary > 0.001;
    let do_raytrace = config.weight_raytrace > 0.001;

    // For tuning, we still collect all features first to build a global distance matrix
    let features: Vec<ImageFeature> = images.into_par_iter()
        .filter_map(|file| extract_single_feature(file.path.clone(), file, do_sketch, do_binary, do_raytrace, None).map(|(f, _, _)| f))
        .collect();

    let mut norm = NormStats::default();
    for f in &features { norm.update(f); }

    let vecs: Vec<[f64; 8]> = features.iter().map(|f| norm.normalize(f)).collect();
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
                config.weight_sketch as f64,
                config.weight_binary as f64,
                config.weight_raytrace as f64,
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

// ── Rendering & Geometric Profile Algorithms ────────────────────────────────

/// Calcule le score de croquis basé sur la variance du gradient de Sobel
/// et la saturation moyenne dans l'espace colorimétrique.
pub fn compute_sketch_score(rgb_pixels: &[u8], width: usize, height: usize) -> f64 {
    if width < 3 || height < 3 {
        return 0.0;
    }

    let mut gray = vec![0u8; width * height];
    let mut total_saturation = 0.0;

    for i in 0..(width * height) {
        let r = rgb_pixels[i * 3] as f64;
        let g = rgb_pixels[i * 3 + 1] as f64;
        let b = rgb_pixels[i * 3 + 2] as f64;

        gray[i] = (0.299 * r + 0.587 * g + 0.114 * b) as u8;

        let max_val = r.max(g).max(b);
        let min_val = r.min(g).min(b);
        if max_val > 0.0 {
            total_saturation += (max_val - min_val) / max_val;
        }
    }

    let mean_saturation = total_saturation / (width * height) as f64;

    let mut magnitudes = Vec::with_capacity((width - 2) * (height - 2));
    let mut sum_magnitude = 0.0;

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let idx = |dx: isize, dy: isize| -> f64 {
                let px = (x as isize + dx) as usize;
                let py = (y as isize + dy) as usize;
                gray[py * width + px] as f64
            };

            let gx = -1.0 * idx(-1, -1) + 1.0 * idx(1, -1)
                   - 2.0 * idx(-1,  0) + 2.0 * idx(1,  0)
                   - 1.0 * idx(-1,  1) + 1.0 * idx(1,  1);

            let gy = -1.0 * idx(-1, -1) - 2.0 * idx(0, -1) - 1.0 * idx(1, -1)
                   + 1.0 * idx(-1,  1) + 2.0 * idx(0,  1) + 1.0 * idx(1,  1);

            let magnitude = (gx * gx + gy * gy).sqrt();
            magnitudes.push(magnitude);
            sum_magnitude += magnitude;
        }
    }

    let total_elements = magnitudes.len() as f64;
    let mean_magnitude = sum_magnitude / total_elements;

    let mut sum_variance = 0.0;
    for mag in &magnitudes {
        let diff = mag - mean_magnitude;
        sum_variance += diff * diff;
    }
    let edge_variance = sum_variance / total_elements;

    edge_variance / (mean_saturation + 0.001)
}

/// Calcule le score de binarité basé sur le critère de variance inter-classe d'Otsu.
pub fn compute_binary_score(rgb_pixels: &[u8], width: usize, height: usize) -> f64 {
    let total_pixels = (width * height) as f64;
    if total_pixels == 0.0 {
        return 0.0;
    }

    let mut hist = [0u32; 256];
    let mut sum_total_intensity = 0.0;

    for i in 0..(width * height) {
        let r = rgb_pixels[i * 3] as f64;
        let g = rgb_pixels[i * 3 + 1] as f64;
        let b = rgb_pixels[i * 3 + 2] as f64;

        let luma = (0.299 * r + 0.587 * g + 0.114 * b) as usize;
        let clamped_luma = luma.min(255);
        
        hist[clamped_luma] += 1;
        sum_total_intensity += clamped_luma as f64;
    }

    let mean_global = sum_total_intensity / total_pixels;
    let mut global_variance = 0.0;
    for i in 0..256 {
        let count = hist[i] as f64;
        if count > 0.0 {
            global_variance += count * (i as f64 - mean_global).powi(2);
        }
    }
    global_variance /= total_pixels;

    if global_variance == 0.0 {
        return 0.0;
    }

    let mut weight_background = 0.0;
    let mut sum_background = 0.0;
    let mut max_between_variance = 0.0;

    for t in 0..256 {
        let count = hist[t] as f64;
        weight_background += count;
        if weight_background == 0.0 {
            continue;
        }

        let weight_foreground = total_pixels - weight_background;
        if weight_foreground == 0.0 {
            break; 
        }

        sum_background += (t as f64) * count;

        let mean_background = sum_background / weight_background;
        let mean_foreground = (sum_total_intensity - sum_background) / weight_foreground;

        let between_variance = weight_background 
            * weight_foreground 
            * (mean_background - mean_foreground).powi(2);

        if between_variance > max_between_variance {
            max_between_variance = between_variance;
        }
    }

    let final_between_variance = max_between_variance / (total_pixels * total_pixels);
    let binary_score = final_between_variance / global_variance;

    binary_score.clamp(0.0, 1.0)
}

/// Calcule le score de rendu 3D / Raytrace en analysant la perfection mathématique
/// des micro-gradients et la présence de bruit de calcul haute fréquence localisé.
pub fn compute_raytrace_score(rgb_pixels: &[u8], width: usize, height: usize) -> f64 {
    if width < 4 || height < 4 {
        return 0.0;
    }

    let mut gray = vec![0u8; width * height];
    for i in 0..(width * height) {
        let r = rgb_pixels[i * 3] as f64;
        let g = rgb_pixels[i * 3 + 1] as f64;
        let b = rgb_pixels[i * 3 + 2] as f64;
        gray[i] = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
    }

    let mut perfect_gradient_sequences = 0.0;
    let mut high_frequency_noise_blocks = 0.0;
    let mut total_blocks_analyzed = 0.0;

    for y in (0..(height - 4)).step_by(4) {
        for x in (0..(width - 4)).step_by(4) {
            total_blocks_analyzed += 1.0;

            let mut is_perfectly_linear = true;
            let mut local_variances = Vec::with_capacity(4);

            for block_y in 0..4 {
                let row_idx = (y + block_y) * width + x;
                
                let g1 = gray[row_idx + 1] as f64 - gray[row_idx] as f64;
                let g2 = gray[row_idx + 2] as f64 - gray[row_idx + 1] as f64;
                let g3 = gray[row_idx + 3] as f64 - gray[row_idx + 2] as f64;

                if (g1 - g2).abs() > 0.5 || (g2 - g3).abs() > 0.5 {
                    is_perfectly_linear = false;
                }

                let mean = (gray[row_idx] as f64 + gray[row_idx + 1] as f64 + gray[row_idx + 2] as f64 + gray[row_idx + 3] as f64) / 4.0;
                let var = ((gray[row_idx] as f64 - mean).powi(2)
                    + (gray[row_idx + 1] as f64 - mean).powi(2)
                    + (gray[row_idx + 2] as f64 - mean).powi(2)
                    + (gray[row_idx + 3] as f64 - mean).powi(2)) / 4.0;
                local_variances.push(var);
            }

            if is_perfectly_linear {
                perfect_gradient_sequences += 1.0;
            }

            let mut variance_of_variances = 0.0;
            let mean_var = (local_variances[0] + local_variances[1] + local_variances[2] + local_variances[3]) / 4.0;
             for v in local_variances {
                variance_of_variances += (v - mean_var).powi(2);
            }
            variance_of_variances /= 4.0;

            if variance_of_variances > 150.0 && mean_var > 10.0 {
                high_frequency_noise_blocks += 1.0;
            }
        }
    }

    if total_blocks_analyzed == 0.0 {
        return 0.0;
    }

    let linearity_ratio = perfect_gradient_sequences / total_blocks_analyzed;
    let noise_ratio = high_frequency_noise_blocks / total_blocks_analyzed;

    let raytrace_score = ((linearity_ratio * 0.6) + (noise_ratio * 0.4)) as f64;

    raytrace_score.clamp(0.0, 1.0)
}
