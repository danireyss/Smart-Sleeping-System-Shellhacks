mod adapters;
mod config;
mod domain;
mod services;

use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::services::IngestService;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env();
    let ingest = IngestService;

    let (tx, mut rx) = mpsc::channel(64);
    adapters::bridge::spawn(config.router_socket, tx);

    while let Some(reading) = rx.recv().await {
        ingest.handle(reading);
    }
}
