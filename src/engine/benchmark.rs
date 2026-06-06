use std::time::Instant;
use std::path::PathBuf;
use rayon::iter::IntoParallelIterator;
use rayon::prelude::*;
use crate::engine::auto_group::{
    ImageFeature, NormStats, OnlineClusterManager, combined_distance, select_champions
};
use crate::messages::AutoGroupConfig;

// Simple LCG for deterministic random data generation
struct Lcg { state: u64 }
impl Lcg {
    fn new(seed: u64) -> Self { Self { state: seed } }
    fn next_f32(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((self.state >> 40) as u32) as f32 / 16777216.0
    }
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.state
    }
    fn next_range(&mut self, min: f32, max: f32) -> f32 {
        min + self.next_f32() * (max - min)
    }
}

fn generate_dataset(name: &str, count: usize, seed: u64) -> Vec<ImageFeature> {
    let mut lcg = Lcg::new(seed);
    let mut features = Vec::with_capacity(count);
    
    for i in 0..count {
        let (time, l, a, b, aspect) = match name {
            "Burst" => {
                // Very tight clustering, slightly drifting
                let t = 1000.0 + (i as f32) * 1.5 + lcg.next_range(-5.0, 5.0);
                let l_val = 50.0 + lcg.next_range(-2.0, 2.0);
                let a_val = 10.0 + lcg.next_range(-1.0, 1.0);
                let b_val = -5.0 + lcg.next_range(-1.0, 1.0);
                (t, l_val, a_val, b_val, 1.5)
            },
            "Timeline" => {
                // Steady progression
                let t = (i as f32) * 86400.0; // 1 day apart
                let l_val = lcg.next_range(20.0, 80.0);
                let a_val = lcg.next_range(-20.0, 20.0);
                let b_val = lcg.next_range(-20.0, 20.0);
                (t, l_val, a_val, b_val, if lcg.next_f32() > 0.5 { 1.33 } else { 0.75 })
            },
            "Chaos" | _ => {
                // High variance
                let t = lcg.next_range(0.0, 1000000.0);
                let l_val = lcg.next_range(0.0, 100.0);
                let a_val = lcg.next_range(-50.0, 50.0);
                let b_val = lcg.next_range(-50.0, 50.0);
                let aspect = lcg.next_range(0.5, 2.0);
                (t, l_val, a_val, b_val, aspect)
            }
        };

        features.push(ImageFeature {
            path: PathBuf::from(format!("img_{}.jpg", i)),
            time,
            l, a, b,
            aspect_ratio: aspect,
            phash_bits: Some(lcg.next_u64()),
            dominant_colors: vec![[l, a, b]],
        });
    }
    features
}

pub trait ClusterAlgorithm: Send + Sync {
    fn name(&self) -> &str;
    fn run(&self, data: &[ImageFeature], config: &AutoGroupConfig) -> (usize, Vec<usize>, usize);
}

// Algo A: Batch DBSCAN
pub struct BatchDbscan;
impl ClusterAlgorithm for BatchDbscan {
    fn name(&self) -> &str { "Batch DBSCAN" }
    fn run(&self, data: &[ImageFeature], config: &AutoGroupConfig) -> (usize, Vec<usize>, usize) {
        use linfa_clustering::Dbscan;
        use ndarray::Array2;

        let mut norm = NormStats::new(0.0); // Exact variance, no clamp
        for f in data { norm.update(f); }

        let mut matrix = Array2::<f64>::zeros((data.len(), 5));
        for (i, f) in data.iter().enumerate() {
            let v = norm.normalize(f);
            for j in 0..5 { matrix[[i, j]] = v[j]; }
        }

        // Approx memory: $N \times 5 \times 8$ bytes for data, plus distance matrix $N^2 \times 8$
        let approx_mem = (data.len() * 5 * 8) + (data.len() * data.len() * 8);

        // using L2 on scaled coordinates (simplified for linfa compatibility)
        use linfa::traits::Transformer;
        let dataset = linfa::DatasetBase::from(matrix);
        let clusters = Dbscan::params(config.min_samples).tolerance(config.eps as f64).transform(dataset).unwrap();
        
        let mut num_clusters = 0;
        let mut assignments = Vec::with_capacity(data.len());
        for c in clusters.targets().iter() {
            let id = match c {
                None => 0,
                Some(val) => *val + 1,
            };
            if id > num_clusters { num_clusters = id; }
            assignments.push(id);
        }

        (num_clusters, assignments, approx_mem)
    }
}

// Algo B: Naive Streaming Leader
pub struct NaiveStreaming;
impl ClusterAlgorithm for NaiveStreaming {
    fn name(&self) -> &str { "Naive Streaming Leader" }
    fn run(&self, data: &[ImageFeature], config: &AutoGroupConfig) -> (usize, Vec<usize>, usize) {
        let mut norm = NormStats::new(0.0); // Unclamped
        let mut manager = OnlineClusterManager::new(config);
        
        let mut assignments = Vec::with_capacity(data.len());
        for f in data {
            norm.update(f);
            let cluster_id = manager.ingest(f, &norm);
            assignments.push(cluster_id);
        }
        
        let clusters = manager.finalize();
        let approx_mem = clusters.len() * 128; // K * cluster struct size
        (clusters.len(), assignments, approx_mem)
    }
}

// Algo C: Stabilized Streaming Leader (Champions Trick)
pub struct ChampionStreaming;
impl ClusterAlgorithm for ChampionStreaming {
    fn name(&self) -> &str { "Champion-Initialized Streaming" }
    fn run(&self, data: &[ImageFeature], config: &AutoGroupConfig) -> (usize, Vec<usize>, usize) {
        let mut norm = NormStats::new(0.1); // Clamped
        let mut manager = OnlineClusterManager::new(config);
        
        let all_paths: Vec<PathBuf> = data.iter().map(|f| f.path.clone()).collect();
        let champs = select_champions(&all_paths);
        
        // Feed champions first
        for cp in &champs {
            if let Some(f) = data.iter().find(|x| x.path == *cp) {
                norm.update(f);
                manager.ingest(f, &norm);
            }
        }
        
        let mut assignments = Vec::with_capacity(data.len());
        for f in data {
            // Already ingested champions? Skip or re-ingest? In real code we remove them from the stream.
            // For simplicity we will just let them stream through again or skip. Let's skip.
            if champs.contains(&f.path) {
                assignments.push(0); // placeholder
                continue;
            }
            norm.update(f);
            let cluster_id = manager.ingest(f, &norm);
            assignments.push(cluster_id);
        }

        // fix champion placeholders
        for i in 0..data.len() {
            if assignments[i] == 0 {
                // rough assignment for metric consistency
                assignments[i] = manager.ingest(&data[i], &norm); 
            }
        }
        
        let clusters = manager.finalize();
        let approx_mem = clusters.len() * 128;
        (clusters.len(), assignments, approx_mem)
    }
}

// ── Metrics ──────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct BenchmarkRecord {
    #[serde(rename = "Dataset Name")]
    dataset_name: String,
    #[serde(rename = "Image Count")]
    image_count: usize,
    #[serde(rename = "Heterogeneity Rate")]
    heterogeneity_rate: f64,
    #[serde(rename = "Algorithm")]
    algorithm: String,
    #[serde(rename = "Time (ms)")]
    time_ms: u128,
    #[serde(rename = "Peak Mem (MB)")]
    peak_mem_mb: f64,
    #[serde(rename = "Cluster Count")]
    cluster_count: usize,
    #[serde(rename = "Silhouette Score")]
    silhouette_score: f64,
}

fn calculate_silhouette(data: &[ImageFeature], assignments: &[usize], config: &AutoGroupConfig) -> f64 {
    let n = data.len();
    if n < 2 { return 0.0; }

    let mut norm = NormStats::new(0.0);
    for f in data { norm.update(f); }

    let vecs: Vec<[f64; 5]> = data.iter().map(|f| norm.normalize(f)).collect();

    let mut s_total = 0.0;
    
    for i in 0..n {
        let c_i = assignments[i];
        
        let mut a_sum = 0.0;
        let mut a_count = 0;
        
        let mut b_min = f64::MAX;
        let mut b_sums = std::collections::HashMap::new();
        let mut b_counts = std::collections::HashMap::new();

        for j in 0..n {
            if i == j { continue; }
            let phash_weight = config.eps as f64 * 0.4 * (config.weight_name as f64).max(0.1);
            let palette_weight = config.eps as f64 * 0.6 * (config.weight_color as f64).max(0.1);
            let d = combined_distance(
                &vecs[i], &vecs[j], 
                data[i].phash_bits, data[j].phash_bits, 
                &data[i].dominant_colors, &data[j].dominant_colors,
                config.weight_color as f64, 
                config.weight_time as f64,
                config.weight_name as f64,
                phash_weight, 
                palette_weight
            );
            
            let c_j = assignments[j];
            if c_i == c_j {
                a_sum += d;
                a_count += 1;
            } else {
                *b_sums.entry(c_j).or_insert(0.0) += d;
                *b_counts.entry(c_j).or_insert(0) += 1;
            }
        }

        let a = if a_count > 0 { a_sum / a_count as f64 } else { 0.0 };
        
        for (c_j, sum) in b_sums {
            let count = b_counts[&c_j];
            let avg = sum / count as f64;
            if avg < b_min { b_min = avg; }
        }
        
        if b_min == f64::MAX { b_min = 0.0; }

        let max_ab = a.max(b_min);
        let s = if max_ab > 0.0 { (b_min - a) / max_ab } else { 0.0 };
        s_total += s;
    }

    s_total / n as f64
}

fn calculate_heterogeneity(data: &[ImageFeature]) -> f64 {
    if data.is_empty() { return 0.0; }
    let mut norm = NormStats::new(0.0);
    for f in data { norm.update(f); }
    
    // Normalize raw standard deviations roughly to [0..1] ranges
    let time_std_days = norm.time.std_dev() / 86400.0;
    let l_std = norm.l.std_dev() / 100.0;
    let a_std = norm.a.std_dev() / 128.0;
    let b_std = norm.b.std_dev() / 128.0;
    let aspect_std = norm.aspect_ratio.std_dev() / 2.0;
    
    let rate = (time_std_days * 0.1) + l_std + a_std + b_std + aspect_std;
    rate
}

pub async fn run_benchmark_suite() {
    println!("Starting BildBlitz Benchmarking Suite...");
    
    let config = AutoGroupConfig {
        source_path: PathBuf::new(),
        eps: 1.5,
        min_samples: 4,
        weight_time: 1.0,
        weight_color: 1.0,
        weight_name: 0.0,
        create_physical: false,
    };

    let datasets = vec![
        ("Burst", generate_dataset("Burst", 200, 42)),
        ("Timeline", generate_dataset("Timeline", 500, 43)),
        ("Chaos", generate_dataset("Chaos", 1000, 44)),
    ];

    let algos: Vec<Box<dyn ClusterAlgorithm>> = vec![
        Box::new(BatchDbscan),
        Box::new(NaiveStreaming),
        Box::new(ChampionStreaming),
    ];

    let mut csv_wtr = csv::Writer::from_path("benchmark_results.csv").unwrap();
    csv_wtr.write_record(&[
        "Dataset Name", "Image Count", "Heterogeneity Rate", 
        "Algorithm", "Time (ms)", "Peak Mem (MB)", 
        "Cluster Count", "Silhouette Score"
    ]).unwrap();

    let mut json_records = Vec::new();
    
    let mut md_report = String::new();
    md_report.push_str("# BildBlitz Benchmarking Results\n\n");
    md_report.push_str("| Dataset | N | Heterogeneity | Algorithm | Time (ms) | Peak Mem (MB) | Clusters | Silhouette |\n");
    md_report.push_str("|---|---|---|---|---|---|---|---|\n");


    for (ds_name, ds_data) in datasets {
        let count = ds_data.len();
        let heterogeneity = calculate_heterogeneity(&ds_data);
        println!("Evaluating Dataset: {} (N={}, Heterogeneity: {:.3})", ds_name, count, heterogeneity);
        
        for algo in &algos {
            print!("  Running {}... ", algo.name());
            
            let start = Instant::now();
            let (num_clusters, assignments, mem_bytes) = algo.run(&ds_data, &config);
            let duration = start.elapsed().as_millis();
            
            let silhouette = calculate_silhouette(&ds_data, &assignments, &config);
            let mem_mb = mem_bytes as f64 / 1_048_576.0;

            let record = BenchmarkRecord {
                dataset_name: ds_name.to_string(),
                image_count: count,
                heterogeneity_rate: heterogeneity,
                algorithm: algo.name().to_string(),
                time_ms: duration,
                peak_mem_mb: (mem_mb * 1000.0).round() / 1000.0,
                cluster_count: num_clusters,
                silhouette_score: (silhouette * 10000.0).round() / 10000.0,
            };

            csv_wtr.write_record(&[
                &record.dataset_name,
                &record.image_count.to_string(),
                &format!("{:.4}", record.heterogeneity_rate),
                &record.algorithm,
                &record.time_ms.to_string(),
                &format!("{:.3}", record.peak_mem_mb),
                &record.cluster_count.to_string(),
                &format!("{:.4}", record.silhouette_score),
            ]).unwrap();

            json_records.push(record);
            
            md_report.push_str(&format!(
                "| {} | {} | {:.4} | {} | {} | {:.3} | {} | {:.4} |\n",
                ds_name,
                count,
                heterogeneity,
                algo.name(),
                duration,
                mem_mb,
                num_clusters,
                silhouette
            ));
            
            println!("Done! ({} ms, {:.3} MB, {} clusters, Silhouette: {:.4})", duration, mem_mb, num_clusters, silhouette);
        }
    }
    
    csv_wtr.flush().unwrap();
    
    // Write JSON report
    let json_report = serde_json::to_string_pretty(&json_records).unwrap();
    std::fs::write("benchmark_results.json", json_report).expect("Unable to write JSON report");

    // Write Markdown report
    std::fs::write("benchmark_results.md", md_report).expect("Unable to write Markdown report");

    println!("Benchmarking complete. Results saved to .csv, .json, and .md files.");
}

pub async fn run_study_on_folder(
    config: AutoGroupConfig,
    progress_tx: tokio::sync::mpsc::Sender<crate::messages::AutoGroupProgress>,
) -> anyhow::Result<()> {
    use crate::engine::auto_group::extract_single_feature;
    use crate::engine::gallery::GalleryScanner;
    use chrono::Local;

    let files = GalleryScanner::scan_directory(&config.source_path).await;
    let images: Vec<_> = files.into_iter().filter(|f| !f.is_dir).collect();
    let total = images.len();
    if total == 0 {
        return Err(anyhow::anyhow!("No images found in the selected folder"));
    }

    let _ = progress_tx.send(crate::messages::AutoGroupProgress::Extracted { done: 0, total }).await;

    // 1. Extract features for all images
    let features: Vec<ImageFeature> = images.into_par_iter()
        .filter_map(|file| {
            // We don't need the DB for the study, just the features
            extract_single_feature(file.path.clone(), file).map(|(f, _, _)| f)
        })
        .collect();
    
    let _ = progress_tx.send(crate::messages::AutoGroupProgress::Extracted { done: total, total }).await;
    let _ = progress_tx.send(crate::messages::AutoGroupProgress::Clustering { percent: 0.0 }).await;

    let algos: Vec<Box<dyn ClusterAlgorithm>> = vec![
        Box::new(BatchDbscan),
        Box::new(NaiveStreaming),
        Box::new(ChampionStreaming),
    ];

    let mut records = Vec::new();
    let now = Local::now();
    let timestamp = now.format("%Y%m%d_%H%M%S").to_string();

    for (i, algo) in algos.iter().enumerate() {
        let pct = (i as f32 / algos.len() as f32) * 100.0;
        let _ = progress_tx.send(crate::messages::AutoGroupProgress::Clustering { percent: pct }).await;

        let start = Instant::now();
        let (num_clusters, assignments, mem_bytes) = algo.run(&features, &config);
        let duration = start.elapsed().as_millis();
        
        let silhouette = calculate_silhouette(&features, &assignments, &config);
        let mem_mb = mem_bytes as f64 / 1_048_576.0;

        records.push(BenchmarkRecord {
            dataset_name: config.source_path.file_name().unwrap_or_default().to_string_lossy().to_string(),
            image_count: total,
            heterogeneity_rate: calculate_heterogeneity(&features),
            algorithm: algo.name().to_string(),
            time_ms: duration,
            peak_mem_mb: (mem_mb * 1000.0).round() / 1000.0,
            cluster_count: num_clusters,
            silhouette_score: (silhouette * 10000.0).round() / 10000.0,
        });
    }

    let _ = progress_tx.send(crate::messages::AutoGroupProgress::Clustering { percent: 100.0 }).await;

    // 2. Determine output directory
    let mut output_dir = dirs::config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config dir"))?;
    output_dir.push("BildBlitz");
    output_dir.push("result_folder");
    std::fs::create_dir_all(&output_dir)?;

    // 3. Save results
    let filename = format!("{}_study_results.json", timestamp);
    let path = output_dir.join(filename);
    let json = serde_json::to_string_pretty(&records)?;
    std::fs::write(&path, json)?;

    // Also save a CSV version
    let csv_filename = format!("{}_study_results.csv", timestamp);
    let csv_path = output_dir.join(csv_filename);
    let mut wtr = csv::Writer::from_path(csv_path)?;
    wtr.write_record(&[
        "Algorithm", "Time (ms)", "Peak Mem (MB)", "Cluster Count", "Silhouette Score"
    ])?;
    for r in records {
        wtr.write_record(&[
            &r.algorithm,
            &r.time_ms.to_string(),
            &format!("{:.3}", r.peak_mem_mb),
            &r.cluster_count.to_string(),
            &format!("{:.4}", r.silhouette_score),
        ])?;
    }
    wtr.flush()?;

    Ok(())
}
