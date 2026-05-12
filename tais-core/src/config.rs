// Configuration — loaded from tais.toml or environment variables

use serde::{Deserialize, Serialize};
use tracing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub evolution: EvolutionConfig,
    pub database: DatabaseConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionConfig {
    pub threshold: f64,
    pub min_sessions: u32,
    pub auto_deploy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

fn default_max_connections() -> u32 { 5 }

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 8080,
            },
            evolution: EvolutionConfig {
                threshold: 0.7,
                min_sessions: 10,
                auto_deploy: false,
            },
            database: DatabaseConfig {
                url: "sqlite:tais.db?mode=rwc".into(),
                max_connections: 5,
            },
        }
    }
}

impl Config {
    /// Load from tais.toml or use defaults
    pub fn load() -> Self {
        if let Ok(content) = std::fs::read_to_string("tais.toml") {
            match toml::from_str::<Config>(&content) {
                Ok(cfg) => {
                    tracing::info!("Config loaded from tais.toml");
                    cfg
                }
                Err(e) => {
                    tracing::warn!("tais.toml parse error: {e} — using defaults");
                    Self::default()
                }
            }
        } else {
            tracing::info!("No tais.toml found — using defaults");
            Self::default()
        }
    }
}
