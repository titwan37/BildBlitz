use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorite {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub favorites: Vec<Favorite>,
}

pub fn get_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|mut p| {
        p.push("BildBlitz");
        p.push("favorites.json");
        p
    })
}

pub fn load_config() -> AppConfig {
    if let Some(path) = get_config_path() {
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    // Preprocess content to handle invalid backslashes in paths (common user error)
                    // We double all backslashes to make them valid in JSON, but then fix any 
                    // correctly escaped quotes that were accidentally doubled (e.g., \" became \\\").
                    let sanitized = content.replace('\\', "\\\\").replace("\\\\\"", "\\\"");
                    match serde_json::from_str::<AppConfig>(&sanitized) {
                        Ok(config) => {
                            info!("Loaded config from {:?}", path);
                            return config;
                        }
                        Err(e) => error!("Failed to parse config at {:?}: {}. Raw content snippet: {}", path, e, &sanitized[..std::cmp::min(sanitized.len(), 100)]),
                    }
                }
                Err(e) => error!("Failed to read config file at {:?}: {}", path, e),
            }
        } else {
            info!("Config file not found at {:?}. Using default.", path);
        }
    } else {
        error!("Could not determine configuration directory");
    }
    AppConfig::default()
}

pub fn save_config(config: &AppConfig) -> anyhow::Result<()> {
    if let Some(path) = get_config_path() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(config)?;
        fs::write(&path, content)?;
        info!("Saved config to {:?}", path);
    }
    Ok(())
}
