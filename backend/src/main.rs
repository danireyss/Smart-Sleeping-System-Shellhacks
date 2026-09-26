mod adapters;
mod config;
mod domain;
mod repositories;
mod services;

use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::repositories::SqliteReadingRepository;
use crate::services::IngestService;

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
    let ingest = IngestService::new(repo);

    let (tx, mut rx) = mpsc::channel(64);
    adapters::bridge::spawn(config.router_socket, tx);

    while let Some(reading) = rx.recv().await {
        ingest.handle(reading);
    }
}
