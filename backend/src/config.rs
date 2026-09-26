//! Settings from environment variables.

use std::env;

pub struct Config {
    pub router_socket: String,
    pub db_path: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            router_socket: env::var("ROUTER_SOCKET")
                .unwrap_or_else(|_| "/var/run/arduino-router.sock".to_string()),
            db_path: env::var("DB_PATH").unwrap_or_else(|_| "sleep-env.db".to_string()),
        }
    }
}
