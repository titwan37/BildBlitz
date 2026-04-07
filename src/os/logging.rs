use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing_subscriber::{fmt, prelude::*, EnvFilter, Layer};
use tracing_appender::non_blocking::WorkerGuard;

/// Handle for live UI log stream
pub struct LogStream {
    pub receiver: mpsc::UnboundedReceiver<String>,
}

/// Custom layer to send log entries to an mpsc channel for live UI display
struct UiLogLayer {
    sender: mpsc::UnboundedSender<String>,
}

impl<S> Layer<S> for UiLogLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut buffer = String::new();
        let mut visitor = LogVisitor(&mut buffer);
        event.record(&mut visitor);
        
        // Format the message with level and timestamp
        let level = *event.metadata().level();
        let time = chrono::Local::now().format("%H:%M:%S");
        let formatted = format!("[{}] {:<5} | {}", time, level, buffer);
        
        let _ = self.sender.send(formatted);
    }
}

struct LogVisitor<'a>(&'a mut String);
impl<'a> tracing::field::Visit for LogVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write;
            let _ = write!(self.0, "{:?}", value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        }
    }
}

/// Initializes the logging system.
/// Returns a guard that must be kept alive for the file appender to work,
/// and a receiver for live log entries.
pub fn init_logging() -> (WorkerGuard, LogStream, PathBuf) {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("BildBlitz")
        .join("logs");

    if !log_dir.exists() 
    {
        let _ = std::fs::create_dir_all(&log_dir);
    }

    let file_appender = tracing_appender::rolling::daily(&log_dir, "bildblitz.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let (tx, rx) = mpsc::unbounded_channel();

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,bildblitz=debug"));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .json() // Use JSON for auditable logs in file
        .with_target(true)
        .with_thread_ids(true);

    let stdout_layer = fmt::layer()
        .with_target(false)
        .compact();

    let ui_layer = UiLogLayer { sender: tx };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .with(ui_layer)
        .init();

    tracing::info!("Logging initialized. Folder: {:?}", log_dir);

    (guard, LogStream { receiver: rx }, log_dir)
}

pub fn open_log_folder(path: &Path) {
    if let Err(e) = opener::open(path) {
        tracing::error!("Failed to open log folder: {}", e);
    }
}
