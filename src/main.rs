mod app;
mod engine;
mod library;
mod messages;
mod os;
mod server;
mod ui;

use crate::app::BildBlitzApp;

#[tokio::main]
async fn main() -> eframe::Result<()> {
    // Initialize the logging system
    let (_guard, log_stream, log_dir) = crate::os::logging::init_logging();

    if std::env::args().any(|arg| arg == "--benchmark") {
        crate::engine::benchmark::run_benchmark_suite().await;
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("BildBlitz")
            .with_drag_and_drop(true),
        ..Default::default()
    };

    let db = crate::library::db::DatabaseManager::new().await.expect("Failed to initialize database");

    // Start background API server
    let db_for_server = db.clone();
    match crate::server::start_server(db_for_server) {
        Ok(server) => {
            tokio::spawn(server);
        }
        Err(e) => {
            eprintln!("Failed to start web server: {}", e);
        }
    }

    eframe::run_native(
        "BildBlitz",
        options,
        Box::new(|cc| Ok(Box::new(BildBlitzApp::new(cc, log_stream, log_dir, db)))),
    )
}
