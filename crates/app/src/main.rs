//! Ferrum API desktop process bootstrap.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use eframe::egui;
use ferrum_app_services::FerrumService;
use ferrum_http_client::HttpEngine;
use ferrum_secrets::OsSecretStore;
use ferrum_storage::SqliteStore;
use ferrum_ui::FerrumApp;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> Result<()> {
    let data_dir = data_directory()?;
    std::fs::create_dir_all(&data_dir).context("could not create the Ferrum data directory")?;
    let _log_guard = configure_logging(&data_dir)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("ferrum-worker")
        .build()
        .context("could not start the asynchronous runtime")?;
    let store = runtime
        .block_on(SqliteStore::open(&data_dir.join("ferrum.db")))
        .context("could not open local workspace data")?;
    let client = Arc::new(
        HttpEngine::new(data_dir.join("responses"))
            .context("could not initialize secure HTTP networking")?,
    );
    let secrets = Arc::new(OsSecretStore::new("dev.ferrum-api.desktop"));
    let service = Arc::new(FerrumService::new(store, client, secrets));
    let snapshot = runtime
        .block_on(service.initialize())
        .context("could not restore the local workspace")?;
    let handle = runtime.handle().clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Ferrum API"),
        ..Default::default()
    };
    eframe::run_native(
        "Ferrum API",
        options,
        Box::new(move |context| {
            Ok(Box::new(FerrumApp::new(
                context,
                service.clone(),
                handle.clone(),
                snapshot,
            )))
        }),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(())
}

fn configure_logging(
    data_dir: &std::path::Path,
) -> Result<tracing_appender::non_blocking::WorkerGuard> {
    let appender = tracing_appender::rolling::daily(data_dir.join("logs"), "ferrum.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,wgpu=warn")),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer),
        )
        .try_init()
        .context("could not initialize structured logging")?;
    Ok(guard)
}

fn data_directory() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("FERRUM_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    #[cfg(target_os = "windows")]
    {
        let root = std::env::var_os("APPDATA").context("APPDATA is unavailable")?;
        Ok(PathBuf::from(root).join("FerrumAPI"))
    }
    #[cfg(target_os = "macos")]
    {
        let root = std::env::var_os("HOME").context("HOME is unavailable")?;
        Ok(PathBuf::from(root)
            .join("Library")
            .join("Application Support")
            .join("FerrumAPI"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = std::env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(root).join("ferrum-api"));
        }
        let root = std::env::var_os("HOME").context("HOME is unavailable")?;
        Ok(PathBuf::from(root)
            .join(".local")
            .join("share")
            .join("ferrum-api"))
    }
}
