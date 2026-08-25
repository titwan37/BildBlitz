# BildBlitz ⚡

**BildBlitz** is a blazing-fast, lightweight image browser, intelligent clustering engine, and library manager designed natively for Windows 10/11. Built from the ground up in Rust using `egui` and `eframe`, it delivers an instant, zero-latency user experience—even when navigating massive directories, network-attached storage, or performing complex multi-dimensional similarity organization.

Designed as a modern spiritual successor to classic image viewers like IrfanView and FastStone, BildBlitz prioritizes extreme performance, a minimal memory footprint, a non-blocking multi-threaded architecture, and optional hardware acceleration via **CUDA, DirectML, and WGPU Compute Shaders**.

---

## ✨ Key Features

* **✨ Agile 8D Streaming Clustering:** Revolutionary thematic organization using **Champion-Seeded Online Leader Clustering** in an 8-dimensional continuous vector space. Group images across **Time**, **Color (CIELAB + 8-Color Palette)**, **Composition (pHash & Aspect Ratio)**, **Sketch / Croquis**, **Binary / Silhouette**, and **3D Raytracing**.
* **⚙️ 6 Determinant Forces & Explainable AI:** A "White Box" clustering approach. BildBlitz visualizes exactly which features (Time, Color, Composition, Sketch, Silhouette, 3D Raytrace) drove the grouping, empowering users to tune slider weights with precision.
* **⏱️ Lock-Free Performance Profiling Telemetry:** Real-time diagnostics card displaying microsecond calculation costs per force (Image Decoding, Color Lab, pHash, Sobel, Otsu, 4x4 Raytrace) and overall ingestion throughput (e.g. `⚡ 480 img/s`).
* **🗂️ Smart Folders & Automated Culling Engine:** Intelligently organize messy directories into structured thematic subfolders with automated burst-duplicate detection, sequence index prefix trimming, and isolated `_Burst_Rushes` storage.
* **🏎️ Fast Multi-Tier Decoding Pipeline:** Instant 3-Tier ingestion:
  * **Tier 0:** SQLite persistent feature cache (**0.0 ms** hit).
  * **Tier 1:** EXIF header embedded thumbnail extractor (**< 0.5 ms** decode).
  * **Tier 2:** 128KB buffered stream decoder with conditional zero-cost short-circuiting (**30–50% throughput gain** when disabling unneeded sliders).
* **⚡ Multi-Backend Hardware Acceleration (TensorEngine):** Batch GEMM distance matrix computations offloaded to **NVIDIA CUDA / cuBLAS**, **DirectML (Intel Arc / AMD)**, **WGPU Compute Shaders (WGSL)**, or multi-threaded **SIMD CPU (Rayon)**.
* **📂 Virtual Collections & Duplicates:** Dedicated tabbed interfaces to preview clustered themes or identify visually identical files across your library via **64-bit DCT pHash fingerprints**.
* **⚓ Dockable Side Panel:** A modern, resizable interface for the Auto-Grouping engine. Tweak parameters (Epsilon, Weights) on-the-fly and re-run clustering instantly.
* **📐 Lossless Transform Toolbar:** Native, high-performance buttons for **Rotate 90°**, **180°**, and **Horizontal/Vertical Flip** with instant thumbnail cache invalidation and database synchronization.
* **🖼️ Dual-Pane File Management:** A classic, highly efficient dual-pane interface enabling rapid file operations, drag-and-drop, and side-by-side folder comparison.
* **⚡ Zero-Latency Architecture:** The main UI thread is completely decoupled from I/O. All feature extraction and transformations are offloaded to background `tokio` and `rayon` pools via bounded `mpsc` channels.
* **🧠 Intelligent Memory Management:** Utilizes a highly concurrent LRU cache (`moka` with TinyLFU policy) to manage decoded thumbnails and full-resolution images.
* **🔭 Immersive Gallery Viewer:** Distraction-free viewer featuring predictive pre-fetching ($N-1$ and $N+1$) and hardware acceleration for fluid scaling and panning.
* **🔭 Live Task Auditing:** Real-time audit panel tracking every background operation (Transformations, Clusters, Thumbnail Generation, Smart Folders) with success/failure reporting and telemetry.

---

## 🧠 The Engine: 8-Dimensional Vector Space & Geometric Profile

BildBlitz quantifies images into an **8-dimensional continuous feature space** coupled with structural hashes and color palettes:

$$\mathbf{x} = \left[ t, L, a, b, ar, S, B, R, \text{phash}, \text{palette} \right]$$

### 1. The 6 Determinant Driving Forces

1. **⏱ Time ($t$):** Welford's Z-score normalized modification / EXIF timestamp for burst & event grouping.
2. **🎨 Color / Palette ($L, a, b$):** Perceptually uniform CIELAB lightness & chromaticity channels combined with K-Means 8-Color Palette extraction.
3. **📐 Composition ($ar, \text{phash}$):** Dimensional aspect ratio variance coupled with 64-bit DCT Perceptual Hashing (Hamming distance).
4. **✏️ Sketch / Croquis ($S$):** Sobel edge gradient variance ratio evaluating line-art density:
   $$\text{Score}_{\text{sketch}} = \frac{\text{Var}(\|\nabla I_{\text{Sobel}}\|)}{\bar{S}_{\text{HSV}} + 0.001}$$
5. **⬛ Binary / Silhouette ($B$):** Otsu bimodal inter-class variance ratio measuring high-contrast binarity:
   $$\text{Score}_{\text{binary}} = \frac{\max_t \sigma_B^2(t)}{\sigma_{\text{Global}}^2}$$
6. **🧊 3D Raytracing ($R$):** $4\times 4$ micro-block linearity and high-frequency noise analysis:
   $$\text{Score}_{\text{raytrace}} = 0.6 \cdot \text{Ratio}_{\text{linear}} + 0.4 \cdot \text{Ratio}_{\text{noise}}$$

---

## 🏎️ Fast Ingestion Architecture & Hardware Acceleration

### ⚡ 3-Tier Fast Decoding Pipeline

To resolve the traditional image decoding bottleneck (~50% of CPU scan time):

1. **Tier 0 (SQLite Feature Cache):** Instant **0.0 ms** lookup for previously scanned files verified by `(file_path, modified_timestamp)`.
2. **Tier 1 (EXIF Header Extraction):** Reads only the first 32–64KB header to decode pre-rendered camera thumbnails in **< 0.5 ms** (~15× speedup over 24MP full decodes).
3. **Tier 2 (128KB Buffered Stream):** High-throughput buffered reads for PNG/WebP/AVIF.
4. **Zero-Cost Short-Circuiting:** Setting `weight_raytrace = 0.0` or disabling Sketch/Binary sliders dynamically skips micro-gradient math and intermediate thumbnail rendering, yielding **30–50% faster batch scans**.

### 🎮 Hardware Acceleration (TensorEngine)

Distance matrix computations during streaming leader clustering are executed as batch General Matrix Multiplications (**GEMM**) powered by:
* **NVIDIA CUDA / cuBLAS** via `candle-core` / `candle-nn`.
* **DirectML** via ONNX Runtime for Intel Arc and AMD Radeon hardware.
* **WGPU Compute Shaders** using native WGSL kernels.
* **SIMD CPU Fallback** powered by `rayon` parallel vector pipelines.

---

## 🛠️ Technology Stack

* **Language:** Rust (Edition 2024)
* **UI Framework:** `egui` & `eframe` (High-performance immediate-mode GUI)
* **Async Runtime:** `tokio` (Multi-threaded asynchronous I/O and bounded channel messaging)
* **Data Parallelism:** `rayon` (Parallel feature extraction and SIMD math)
* **Hardware Acceleration:** `candle-core` (CUDA/cuBLAS), `directml`, `wgpu` (WGSL Compute Shaders)
* **Database & Persistence:** `sqlx` (SQLite metadata and feature index)
* **Caching Engine:** `moka` (High-concurrency TinyLFU LRU cache)
* **Image Processing & Color:** `image`, `zune-jpeg`, `kamadak-exif`, `palette` (CIELAB)

---

## 📦 Getting Started

### Prerequisites

* Rust Toolchain (1.85+ recommended)
* Windows 10/11 (Optimized for Windows 11)
* *(Optional)* NVIDIA GPU with CUDA Toolkit for hardware acceleration

### Diagnostic Probe

Verify your GPU drivers and CUDA environment using the built-in diagnostic script:

```powershell
powershell -ExecutionPolicy Bypass -File .\test_cuda.ps1
```

### Building from Source

```bash
git clone https://github.com/titwan37/BildBlitz.git
cd BildBlitz
cargo build --release
cargo run --release
```

---

## 📈 Roadmap

* [x] **8D Multi-Dimensional Clustering:** 6-Force feature space (Time, Color, Composition, Sketch, Binary, Raytrace).
* [x] **Smart Folders & Culling Engine:** Automated thematic grouping, burst duplicate detection, and `_Burst_Rushes` isolation.
* [x] **Fast Multi-Tier Ingestion Pipeline:** EXIF thumbnail extraction (<0.5ms) & SQLite feature cache (0ms).
* [x] **Multi-Backend Tensor Acceleration:** CUDA/cuBLAS, DirectML, WGPU compute shaders, and SIMD CPU GEMM engine.
* [x] **Explainable Determinant Forces & Microsecond Telemetry:** Lock-free performance profiling per force with real-time throughput metrics.
* [x] **Virtual Collections & Duplicates View:** Tabbed preview mode for clusters and pHash duplicate detection.
* [x] **Database Integration:** Local SQLite database for persistent metadata and feature indexing.
* [x] **Lossless Transformations:** Native JPEG rotation and flipping with instant cache invalidation.

---

*Designed with precision for speed, reliability, and modern desktop aesthetics.*

