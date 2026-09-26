//! Settings from environment variables (and a `.env` file, if present; it is
//! gitignored, so the LLM API key never goes in the repo).

use std::env;

pub struct Config {
    pub router_socket: String,
    pub db_path: String,
    pub bind_addr: String,
    /// OpenAI-compatible API base URL (Groq by default).
    pub llm_base_url: String,
    /// `None` leaves the assistant offline.
    pub llm_api_key: Option<String>,
    /// `None` leaves the assistant offline.
    pub llm_model: Option<String>,
    /// When set, POST /api/chat requires `Authorization: Bearer <token>`.
    pub chat_token: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            router_socket: env::var("ROUTER_SOCKET")
                .unwrap_or_else(|_| "/var/run/arduino-router.sock".to_string()),
            db_path: env::var("DB_PATH").unwrap_or_else(|_| "sleep-env.db".to_string()),
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            llm_base_url: env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "https://api.groq.com/openai/v1".to_string()),
            llm_api_key: non_empty("LLM_API_KEY"),
            llm_model: non_empty("LLM_MODEL"),
            chat_token: non_empty("CHAT_TOKEN"),
        }
    }
}

fn non_empty(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.trim().is_empty())
}
