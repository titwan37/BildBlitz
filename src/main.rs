mod app;
mod engine;
mod library;
mod messages;
mod os;
mod ui;

use crate::app::BildBlitzApp;

#[tokio::main]
async fn main() -> eframe::Result<()> {
    // Initialize the logging system
    let (_guard, log_stream, log_dir) = crate::os::logging::init_logging();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("BildBlitz")
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "BildBlitz",
        options,
        Box::new(|cc| Ok(Box::new(BildBlitzApp::new(cc, log_stream, log_dir)))),
    )
}
