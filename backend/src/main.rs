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
use crate::adapters::bridge::{BridgeChannels, DeviceCall};
use crate::services::{
    AgentService, AmbientService, DeviceService, IngestService, ReadingService, SleepService,
};

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
    let readings = Arc::new(ReadingService::new(repo.clone()));
    let sleep = Arc::new(SleepService::new(sessions, repo.clone(), events.clone()));

    // Bridge → ingest (readings) and bridge → device service (LCD calls).
    // Webcam light/sound (optional): adapters record into `ambient`, ingest attaches it.
    let ambient = Arc::new(AmbientService::default());
    match &config.camera_device {
        Some(device) => {
            info!("light level from camera {device}");
            adapters::camera::spawn(device.clone(), config.camera_exposure, ambient.clone());
        }
        None => info!("no light level (set CAMERA_DEVICE to enable)"),
    }
    match &config.mic_device {
        Some(device) => {
            let calibration = adapters::microphone::Calibration {
                offset_db: config.sound_calibration_db,
                floor_db: config.sound_floor_db,
            };
            let volume = config.mic_capture_volume;
            info!("sound level from microphone {device} ({calibration:?}, capture volume {volume:?})");
            adapters::microphone::spawn(device.clone(), calibration, volume, ambient.clone());
        }
        None => info!("no sound level (set MIC_DEVICE to enable)"),
    }

    let ingest = IngestService::new(repo, events.clone(), ambient);
    let device = DeviceService::new(sleep.clone(), readings.clone());
    let (readings_tx, mut readings_rx) = mpsc::channel(64);
    let (device_tx, mut device_rx) = mpsc::channel::<DeviceCall>(16);
    adapters::bridge::spawn(
        config.router_socket,
        BridgeChannels { readings: readings_tx, device: device_tx },
    );
    tokio::spawn(async move {
        while let Some(reading) = readings_rx.recv().await {
            ingest.handle(reading);
        }
    });
    tokio::spawn(async move {
        while let Some(call) = device_rx.recv().await {
            let result = device.handle(call.request).await.map_err(|e| e.to_string());
            // The bridge may have timed out and stopped waiting; that's fine.
            let _ = call.reply.send(result);
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
