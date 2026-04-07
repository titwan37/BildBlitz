# BildBlitz Design and Architecture

## 1. System Architecture

The BildBlitz architecture is designed for maximum responsiveness. The core principle is keeping the main UI thread completely free of blocking operations.

### Key Components

* **UI Thread (Renderer):** Powered by an immediate-mode GUI or lightweight retained-mode GUI. Responsible solely for drawing frames and dispatching user input. It requests images from the Cache Manager and never blocks.
* **Cache Manager (State & Memory):** An in-memory concurrent LRU cache (e.g., `moka`) storing decoded thumbnails and full-size images ready for upload to the GPU.
* **I/O runtime (Async Worker Pool):** An asynchronous runtime (`tokio`) handling non-blocking file system traversal, database queries, and polling OS events.
* **Decoder Thread Pool (CPU Workers):** A dedicated thread pool (`rayon` or `tokio::spawn_blocking`) for CPU-heavy tasks: decompressing JPEGs, generating thumbnails, and computing file hashes.
* **Pre-loading Engine:** A predictive module that listens to user navigation (e.g., scrolling down, pressing Right Arrow) and schedules decoding tasks for adjacent files before the user requests them.
* **Metadata DB:** A local SQLite database storing EXIF data, custom tags, and cached hashes for instant duplicate detection and rapid sorting.

## 2. Crate Recommendations

Here is the proposed dependency stack (`Cargo.toml`):

### UI & Rendering

* **`egui` & `eframe`:** Highly performant immediate-mode GUI. Excellent for building custom, frameless, hardware-accelerated interfaces quickly. (Alternative: `slint` for a more native, retained-mode approach if complex styling is preferred).
* **`image`:** The ecosystem standard for supporting standard image formats (PNG, standard JPG, GIF).
* **`turbojpeg`:** Specifically for lightning-fast JPEG decoding, scaling, and lossless transformations (rotation/cropping). It wraps libjpeg-turbo and is crucial for "instant" loading.

### Concurrency & Async

* **`tokio`:** The standard async runtime for Rust. Ideal for handling file I/O, database access, and the overall event loop.
* **`rayon`:** Data-parallelism library. Essential for processing large batches of files (e.g., batch resizing) or parallel image decoding.
* **`moka`:** A fast, concurrent cache library supporting LRU eviction policies, perfect for managing our in-memory decoded image cache.

### System & I/O

* **`windows`:** Official Microsoft crate for calling native Windows APIs directly (needed for Mica borders, native file copy dialogues, and deep OS integration).
* **`walkdir` & `ignore`:** For blazing-fast, recursive directory traversal.
* **`notify`:** Cross-platform filesystem notification library to watch for external file changes (e.g., when a user dragging a file via Windows Explorer).

### Metadata & Database

* **`rusqlite`:** Ergonomic bindings to SQLite for the local tagging and library database.
* **`kamadak-exif` or `rexiv2`:** To parse and write EXIF/IPTC/XMP metadata efficiently.

## 3. Project Structure

```text
bildblitz/
├── Cargo.toml
├── src/
│   ├── main.rs          # Entry point, initializes tokio runtime and UI
│   ├── app.rs           # Core UI states and application logic (the egui App)
│   ├── ui/              # UI Components
│   │   ├── mod.rs
│   │   ├── grid.rs      # Thumbnail grid view
│   │   ├── viewer.rs    # Full-screen immersive viewer
│   │   └── tools.rs     # Toolbar, magnifier, and split-pane controls
│   ├── engine/          # The Processing Engine
│   │   ├── mod.rs
│   │   ├── loader.rs    # Async image reading and decoding (using turbojpeg/image)
│   │   ├── cache.rs     # LRU Cache management (moka)
│   │   └── prefetch.rs  # Predictive pre-loading logic
│   ├── library/         # File Management & DB
│   │   ├── mod.rs
│   │   ├── scanner.rs   # Directory crawling (walkdir)
│   │   ├── db.rs        # SQLite handling (rusqlite)
│   │   └── metadata.rs  # EXIF parsing
│   └── os/              # Native OS Integrations
│       ├── mod.rs
│       └── windows.rs   # Safe wrappers around windows-rs (Mica, native drag-drop)
```

## 4. Verification Plan

* **Manual Verification:** We will create a Proof of Concept (PoC) that opens a default directory, loads images asynchronously into an in-memory cache, and displays them in a responsive UI grid using `egui`. Hardware acceleration will be implicitly verified if the UI remains responsive at 60fps+ while images load.

Context and Role

Act as an Expert Rust Desktop GUI Developer. We are continuing the development of "BildBlitz", a blazing-fast, lightweight image browser and library manager native to Windows 11.

Your current task is to design and implement the logic and UI structure for the Main Welcoming Page and the Left Navigation Pane (Sidebar).

UI/UX Requirements

The main window should feature a classic, clean split layout:

Left Sidebar (Navigation Pane): A collapsible, hierarchical folder tree.

Right Main View (Welcome Area): A placeholder area that will eventually display the image grid, currently showing a welcoming message or "Select a folder to begin" placeholder.

Navigation Pane Specifics

The Left Sidebar must be divided into two primary, visually distinct sections:

1. Quick Access (Favorites)

This section should be pinned at the top of the sidebar and contain dynamically resolved links to the current Windows user's standard media folders.

Downloads (e.g., C:\Users\<current_user>\Downloads)

Pictures (e.g., C:\Users\<current_user>\Pictures)

Crucial Implementation Detail: You must use a robust crate (like dirs) to programmatically resolve the correct paths for the active Windows user, rather than hardcoding C:\Users\....

1. Local Drives (This PC / Folder Root)

Below the Quick Access section, display a root-level tree of the local file system.

Mapped Drives: Automatically detect and list all mapped drives and volumes currently attached to the Windows machine (e.g., C:\, D:\, network drives).

Lazy Loading Tree: When a user expands a drive (e.g., clicking the arrow next to C:\), the application must only query and load the immediate child directories. Do not recursively index the entire drive on startup to ensure instant application launch times.

Visuals: Use standard folder icons for directories and distinct drive icons for the root mapped drives.

Technical Requirements & Constraints

Language: Rust.

Drive Detection: Use appropriate Windows-native crates (like windows-rs using GetLogicalDrives / GetVolumeInformation) or reliable cross-platform crates (like sysinfo) to accurately list the drive letters and their volume labels.

Path Handling: Utilize std::path::PathBuf for robust path manipulation.

State Management: The UI state must clearly track which folder is currently selected and highlighted in the sidebar.

Desired Output

Please provide the following:

Crate Additions: Any new dependencies needed in Cargo.toml for fetching standard user directories and listing Windows drives.

Core Logic (Rust): A dedicated Rust module (e.g., fs_nav.rs) containing the functions to:

Get the paths for "Downloads" and "Pictures" for the active user.

Query the OS to return a list of all available root drives.

Fetch the immediate child directories of a given path (for the lazy-loading tree).

UI Component Structure: Pseudocode or the actual GUI framework code (e.g., using Slint, Iced, or egui) demonstrating how this two-section sidebar is rendered and how the data binds to the UI.
