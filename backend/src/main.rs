mod adapters;
mod config;
mod controllers;
mod domain;
mod repositories;
mod services;

use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::adapters::llm_client::{ChatModel, OpenAiChatModel};
use crate::config::Config;
use crate::controllers::AppState;
use crate::repositories::{SqliteReadingRepository, SqliteSessionRepository};
use crate::services::{AgentService, IngestService, ReadingService, SleepService};

#[tokio::main]
async fn main() {
    // Optional .env (gitignored) for LLM_API_KEY etc.
    let _ = dotenvy::dotenv();
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
    let sessions = match SqliteSessionRepository::open(&config.db_path) {
        Ok(sessions) => Arc::new(sessions),
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

    let model: Option<Arc<dyn ChatModel>> = match (&config.llm_api_key, &config.llm_model) {
        (Some(key), Some(model)) => {
            info!("assistant: model {model} at {}", config.llm_base_url);
            Some(Arc::new(OpenAiChatModel::new(&config.llm_base_url, key, model)))
        }
        _ => {
            warn!("assistant offline: set LLM_API_KEY and LLM_MODEL to enable chat");
            None
        }
    };
    if config.chat_token.is_none() {
        info!("chat is open to anyone who can reach the server (set CHAT_TOKEN to require a token)");
    }
    let readings = Arc::new(ReadingService::new(repo.clone()));
    let sleep = Arc::new(SleepService::new(sessions, repo));
    let state = AppState {
        agent: Arc::new(AgentService::new(model, readings.clone(), sleep.clone())),
        chat_token: config.chat_token.as_deref().map(Arc::from),
        readings,
        sleep,
        events,
    };
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
