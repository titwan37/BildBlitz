# **Context and Role**

Act as an Expert Rust Developer and Windows Desktop Software Architect. Your task is to design and provide the foundational code structure for **"BildBlitz"**, a new, ultra-lightweight, and fast-processing image browser and library manager native to Windows 11\.

# **Project Overview**

The **BildBlitz** application must prioritize blazing-fast performance, low memory footprint, and instant responsiveness, even when handling directories with tens of thousands of high-resolution images. It should feel native to Windows 11 (supporting features like Snap Layouts, rounded corners, and potentially Mica materials) while leveraging Rust's "fearless concurrency" for background processing.

# **Core Feature Requirements**

## **1\. UI & Navigation (The User Experience)**

* **Folder Tree Structure Navigation:** A collapsible, fast-syncing sidebar displaying the local file system.  
* **Split Panes & Tabbed Interface:** Ability to open multiple directories in tabs or split the main view into dual panes for seamless file management.  
* **Compared Folder View:** Side-by-side synchronized scrolling to visually compare the contents of two different folders.  
* **Adaptive View Modes:** Dynamic grid view (adjustable thumbnail sizes), list view (with detailed file stats), and an immersive full-screen viewer.  
* **Frameless & Immersive Viewing:** A borderless full-screen mode featuring auto-hiding toolbars that only appear when the mouse touches the screen edges (similar to FastStone).  
* **High-Quality Magnifier:** A quick-access loupe/magnifier tool for instant 100% pixel-peeping without changing the overall zoom level.

## **2\. File Operations, Management & Editing**

* **Advanced Drag and Drop:** Seamless drag-and-drop support within the app (between split panes/folders) and externally (to/from Windows Explorer).  
* **Batch Processing Powerhouse:** A robust utility for lightning-fast batch renaming (supporting RegEx and sequential numbering), batch format conversion, and batch resizing.  
* **Lossless Transformations:** Support for lossless JPEG rotation, flipping, and cropping without re-encoding the image file.  
* **Lightweight Non-Destructive Editing:** Basic on-the-fly adjustments including crop, resize, gamma/brightness/contrast sliders, and red-eye removal.  
* **Tagging & Metadata:** Read/Write support for EXIF, IPTC, and XMP data. Ability to assign color labels, star ratings, and custom tags without altering the original image file (using a sidecar file or local lightweight DB).  
* **Smart Selection & Duplicate Finder:** Advanced selection tools (by extension, date, inverse) and a built-in duplicate detection tool utilizing file hashing and basic visual similarity.

## **3\. Performance & Processing (The Engine)**

* **Intelligent Pre-loading:** Asynchronously pre-fetch and decode the next and previous images in a directory into memory to guarantee zero-latency scrolling.  
* **Hardware-Accelerated Rendering:** Utilize GPU acceleration for perfectly smooth panning, zooming, scaling, and animated image playback (GIF/WebP).  
* **Asynchronous I/O & Caching:** Multi-threaded background thumbnail generation with an intelligent, persistent LRU (Least Recently Used) cache stored on disk.  
* **Instant Image Decoding:** Support for standard formats (JPG, PNG, GIF, WebP, AVIF) and RAW formats using fast, native Rust decoders.  
* **Lazy Loading:** Only load thumbnails and metadata for images currently visible in the viewport.  
* **Zero-Copy Operations:** Utilize native Windows APIs (via windows-rs) for file moves/copies to ensure native OS speeds.

# **Technical Stack Preferences**

* **Core Language:** Rust (latest stable).  
* **GUI Framework:** Recommend the best lightweight framework for this use case (e.g., Slint, Iced, egui, or Tauri if web-technologies are acceptable, but strictly prioritize performance).  
* **Database (for Library/Tags):** SQLite (via rusqlite or sqlx) for fast, local, serverless metadata storage.  
* **OS Integration:** windows-rs crate for deep Windows 11 API integration.

# **Desired Output**

Please provide the following to kickstart the **BildBlitz** project:

1. **System Architecture:** A high-level overview of how the UI thread, I/O threads, pre-loading workers, and caching system will interact.  
2. **Crate Recommendations:** A comprehensive Cargo.toml dependencies list with justifications for each crate chosen (e.g., image decoding, UI, database, async runtime).  
3. **Project Structure:** The recommended directory and module layout for the Rust project.  
4. **Proof of Concept (PoC) Code:** A basic Rust implementation demonstrating the asynchronous loading and hardware-accelerated rendering of thumbnails from a directory into a basic UI grid.

# **Desirable extended features:**

1. **Batch Processing Powerhouse & Lossless Transformations:** This brings in the legendary utility of **IrfanView**. Users want to be able to rotate a JPEG or rename 1,000 files instantly without losing quality or opening a heavy photo editor.  
2. **Frameless Viewing & Magnifier:** Inspired directly by **FastStone Image Viewer** and **ImageGlass**, this gives power users a clean workspace where UI elements get out of the way until they hover over the edges.  
3. **Intelligent Pre-loading & Hardware Acceleration:** This mimics the instant "snappiness" found in modern minimalist viewers like **FlyPhotos**, ensuring that when a user hits the right arrow key, the next image is already decoded in memory and swaps instantly.  
4. **Duplicate Finder:** A highly requested feature in modern managers like **Excire** and **Tonfotos** to help clean up messy hard drives.

