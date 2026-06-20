# BildBlitz ⚡

**BildBlitz** is a blazing-fast, lightweight image browser and library manager designed natively for Windows 11. Built from the ground up in Rust using `egui` and `eframe`, it delivers an instant, zero-latency user experience—even when navigating massive directories, network-attached storage, or performing complex machine-learning-driven organization.

Designed as a modern spiritual successor to classic image viewers like IrfanView and FastStone, BildBlitz prioritizes extreme performance, a minimal memory footprint, and a non-blocking architecture.

---

## ✨ Key Features

* **✨ Agile Streaming Clustering:** Revolutionary thematic organization using **Champion-Seeded Online Leader Clustering**. Group images by **8-Color Dominant Analysis**, **Time Proximity**, and **Perceptual Hash (PHash)** in real-time with mathematical stability.
* **📂 Virtual Collections & Duplicates:** Dedicated tabbed interfaces to preview clustered themes or identify visually identical files across your library via **PHash fingerprints**.
* **⚙️ Determinant Force Analysis:** A "White Box" clustering approach. BildBlitz visualizes exactly which feature (Time, Color, or Composition) drove the grouping, empowering users to tune weights with precision.
* **⚓ Dockable Side Panel:** A modern, resizable interface for the Auto-Grouping engine. Tweak parameters (Epsilon, Weights) on-the-fly and re-run clustering instantly.
* **📐 Lossless Transform Toolbar:** Native, high-performance buttons for **Rotate 90°**, **180°**, and **Horizontal/Vertical Flip** with instant thumbnail cache invalidation.
* **🖼️ Dual-Pane File Management:** A classic, highly efficient dual-pane interface enabling rapid file operations, drag-and-drop, and side-by-side folder comparison.
* **⚡ Zero-Latency Architecture:** The main UI thread is completely decoupled from I/O. All feature extraction and transformations are offloaded to background `tokio` and `rayon` pools.
* **🧠 Intelligent Memory Management:** Utilizes a highly concurrent LRU cache (`moka`) to manage decoded thumbnails and full-resolution images, ensuring instantaneous rendering.
* **🔭 Immersive Gallery Viewer:** A distraction-free viewer with pre-fetching and GPU acceleration for fluid, hardware-accelerated scaling and panning.
* **🔭 Live Task Auditing:** A dedicated real-time audit panel that tracks the lifecycle of every background operation (Transformations, Clusters, Thumbnail Generation) with detailed success/failure reporting and image format detection.
* **📈 Research-Grade Benchmarking:** Includes a headless benchmarking harness to evaluate clustering quality (Silhouette Score, Davies-Bouldin Index) and system performance (O(K) memory footprint) across synthetic and real-world datasets.

---

## 🧠 The Engine: Agile Streaming Organization

BildBlitz features a high-performance streaming clustering pipeline that quantifies images into multi-dimensional vectors for real-time thematic grouping:

1. **8-Color Dominant Analysis:** Uses K-Means quantization to extract the primary color palette of every image in the Lab space, enabling deep visual similarity matching.
2. **Temporal Proximity (Time):** Uses **Welford's Algorithm** for real-time Z-score normalization of EXIF dates, detecting bursts and events as they are scanned.
3. **Perceptual Hashing (PHash):** Compares 64-bit image fingerprints using **Hamming distance** to prevent over-clustering of visually distinct but color-similar images.
4. **Champion-Seeded Clustering:** Utilizes a strategic "Champion Seeding" initialization (bootstrapping Welford's statistics with representative images) to ensure mathematical stability and prevent variance collapse in large, varied datasets.

The engine presents results in a "Virtual Collections" view that respects your original file structure until you explicitly click "Commit".

### 📊 Determinant Force Feedback

Unlike "black box" algorithms, BildBlitz calculates the **Weighted Variance** of each feature dimension after normalization. It presents this as **Determinant Forces**—visual percentages that show whether **Time**, **Color**, or **Composition** (Aspect Ratio) was the primary driver for a specific clustering run. This allows for iterative, data-driven parameter tuning.

---

## 🛠️ Technology Stack

* **Language:** Rust (Edition 2024)
* **UI Framework:** `egui` & `eframe` (High-performance immediate-mode GUI)
* **Async Runtime:** `tokio` (Multi-threaded asynchronous I/O and message passing)
* **Clustering Engine:** Custom Streaming Manager (Optimized for incremental ingestion)
* **Color Science:** `palette` (Perceptual CIELAB color conversion)
* **Data Parallelism:** `rayon` (Parallel feature extraction pipeline)
* **Caching Engine:** `moka` (High-performance concurrent caching)
* **Image Processing:** `image` & `img_hash` (Robust decoding and perceptual fingerprinting)

---

## 🧠 Architectural Highlights

BildBlitz was architected to solve the classic "UI freeze" problem:

1. **Strict Thread Separation:** All heavy lifting—parsing EXIF data, ML feature extraction, or traversing deep directory trees—is dispatched via `mpsc` channels to dedicated thread pools.
2. **Virtualized Rendering:** Utilizes `egui`'s virtual scrolling to handle folders with 100,000+ images by only querying and rendering what is actively visible.
3. **Predictive Pre-loading:** The engine anticipates navigation and schedules decoding tasks for adjacent files before you even press the arrow key.

---

## 📦 Getting Started

### Prerequisites

* Rust Toolchain (1.85+ recommended)
* Windows 10/11 (Optimized for Windows 11)

### Building from Source

```bash
git clone https://github.com/titwan37/BildBlitz.git
cd BildBlitz
cargo build --release
cargo run --release
```

---

## 📈 Roadmap

* [x] **ML Auto-Grouping:** Time, Color, and Name-based clustering.

* [x] **Virtual Collections View:** Tabbed preview mode for clusters.
* [x] **Database Integration:** Local SQLite database for persistent metadata indexing.
* [x] **Duplicate Detection:** Perceptual hashing to identify visually identical files.
* [x] **Lossless Transformations:** Native JPEG rotation and flipping.

---

*Designed with precision for speed, reliability, and modern desktop aesthetics.*
