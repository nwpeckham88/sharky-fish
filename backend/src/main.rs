mod actors;
mod config;
mod db;
mod library;
mod metadata;
mod messages;
mod server;

use crate::actors::{
    brain::BrainActor, forge::ForgeActor, identifier::IdentifierActor, queue::QueueActor,
    watcher::WatcherActor,
};
use crate::config::AppConfig;
use crate::metadata::prewarm_recent_library_metadata;
use crate::messages::{IdentifiedMedia, IngestEvent, QueueMsg, SseEvent};
use crate::server::{build_router, AppState};

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock, Semaphore};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sharky_fish=info,tower_http=info".into()),
        )
        .init();

    // Load configuration.
    let config_path =
        std::env::var("SHARKY_CONFIG_PATH").unwrap_or_else(|_| "/config".into());
    let cfg = AppConfig::load(&config_path);
    info!(port = cfg.port, "sharky-fish starting");

    // Initialize SQLite pool.
    let db_path = PathBuf::from(&cfg.config_path).join("sharky.db");
    let pool = db::init_pool(&db_path).await?;

    // Shared synchronization primitives.
    let io_semaphore = Arc::new(Semaphore::new(cfg.max_io_concurrency));
    let (sse_tx, _) = broadcast::channel::<SseEvent>(256);

    // Actor channels.
    let (ingest_tx, ingest_rx) = mpsc::channel::<IngestEvent>(256);
    let (identified_tx, identified_rx) = mpsc::channel::<IdentifiedMedia>(256);
    let (queue_tx, queue_rx) = mpsc::channel::<QueueMsg>(256);

    let metadata_pool = pool.clone();
    let library_root = PathBuf::from(&cfg.data_path);
    let prewarm_limit = cfg.metadata_prewarm_limit;
    let prewarm_concurrency = cfg.max_io_concurrency;
    tokio::spawn(async move {
        match prewarm_recent_library_metadata(
            metadata_pool,
            library_root,
            prewarm_limit,
            prewarm_concurrency,
        )
        .await
        {
            Ok(count) => tracing::info!(count, "metadata: prewarm completed"),
            Err(error) => tracing::warn!(err = %error, "metadata: prewarm failed"),
        }
    });

    // Start actors.
    let watcher = WatcherActor::new(
        PathBuf::from(&cfg.ingest_path),
        PathBuf::from(&cfg.data_path),
        ingest_tx,
        sse_tx.clone(),
        pool.clone(),
    );
    tokio::spawn(async move {
        if let Err(e) = watcher.run().await {
            tracing::error!(err = %e, "watcher actor crashed");
        }
    });

    let identifier = IdentifierActor::new(ingest_rx, identified_tx, io_semaphore.clone());
    tokio::spawn(async move {
        if let Err(e) = identifier.run().await {
            tracing::error!(err = %e, "identifier actor crashed");
        }
    });

    let brain = BrainActor::new(identified_rx, queue_tx.clone(), cfg.llm.clone());
    tokio::spawn(async move {
        if let Err(e) = brain.run().await {
            tracing::error!(err = %e, "brain actor crashed");
        }
    });

    let queue = QueueActor::new(queue_rx, pool.clone());
    tokio::spawn(async move {
        if let Err(e) = queue.run().await {
            tracing::error!(err = %e, "queue actor crashed");
        }
    });

    let forge = ForgeActor::new(
        queue_tx.clone(),
        pool.clone(),
        sse_tx.clone(),
        io_semaphore,
        PathBuf::from(&cfg.data_path),
    );
    tokio::spawn(async move {
        if let Err(e) = forge.run().await {
            tracing::error!(err = %e, "forge actor crashed");
        }
    });

    // Start Axum HTTP server.
    let port = cfg.port;
    let library_path = PathBuf::from(&cfg.data_path);
    let ingest_path = PathBuf::from(&cfg.ingest_path);
    let shared_config = Arc::new(RwLock::new(cfg));
    let state = AppState {
        pool,
        sse_tx,
        library_path,
        ingest_path,
        config: shared_config,
    };
    let app = build_router(state);
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "HTTP server listening");
    axum::serve(listener, app).await?;

    Ok(())
}
