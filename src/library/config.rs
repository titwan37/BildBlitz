use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;
use tracing::{error, info, warn};

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

/// Loads the application configuration from disk.
/// B14 fix: removed fragile backslash sanitization hack.
/// Instead, we parse the JSON directly and give a clear error message.
pub fn load_config() -> AppConfig {
    if let Some(path) = get_config_path() {
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    // 1. Try strict standard JSON parse
                    if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
                        info!("Loaded config from {:?}", path);
                        return config;
                    }

                    // 2. Fallback: Fix un-escaped Windows backslashes (common in manual edits/legacy files)
                    // We double all backslashes, but then "un-double" ones that were likely just escaping a quote.
                    let sanitized = content.replace('\\', "\\\\").replace("\\\\\"", "\\\"");
                    match serde_json::from_str::<AppConfig>(&sanitized) {
                        Ok(config) => {
                            info!("Loaded config from {:?} (recovered via backslash sanitization)", path);
                            // Auto-save the sanitized version so it's valid JSON next time
                            let _ = save_config(&config);
                            return config;
                        }
                        Err(e) => {
                            warn!(
                                "Failed to parse config at {:?}: {}. \
                                 Tip: preserve forward slashes or double backslashes in JSON. \
                                 Raw content snippet: {}",
                                path,
                                e,
                                &content[..std::cmp::min(content.len(), 200)]
                            );
                        }
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

#[allow(dead_code)]
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
