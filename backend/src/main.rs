mod adapters;
mod config;
mod controllers;
mod domain;
mod repositories;
mod services;

use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::controllers::AppState;
use crate::repositories::SqliteReadingRepository;
use crate::services::{IngestService, ReadingService};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env();
    let repo = match SqliteReadingRepository::open(&config.db_path) {
        Ok(repo) => Arc::new(repo),
        Err(e) => {
            error!("failed to open database {}: {e}", config.db_path);
            std::process::exit(1);
        }
    };
    info!("database: {}", config.db_path);

    let (events, _) = broadcast::channel(64);
    let ingest = IngestService::new(repo.clone(), events.clone());
    let (tx, mut rx) = mpsc::channel(64);
    adapters::bridge::spawn(config.router_socket, tx);
    tokio::spawn(async move {
        while let Some(reading) = rx.recv().await {
            ingest.handle(reading);
        }
    });

    let state = AppState { readings: Arc::new(ReadingService::new(repo)), events };
    let listener = match TcpListener::bind(&config.bind_addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("failed to bind {}: {e}", config.bind_addr);
            std::process::exit(1);
        }
    };
    info!("listening on http://{}", config.bind_addr);
    if let Err(e) = axum::serve(listener, controllers::router(state)).await {
        error!("server error: {e}");
    }
}
