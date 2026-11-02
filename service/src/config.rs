/// config.rs — XDG-based configuration for airdrop-bridge

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Directory where received files land (XDG_DOWNLOAD_DIR by default)
    pub receive_dir: PathBuf,
    /// Name advertised in AirDrop (defaults to hostname)
    pub display_name: Option<String>,
    /// Restore always-on receiving after login (requires explicit opt-in)
    pub restore_always_on: bool,
    /// Wi-Fi interface to use (None = let owlyd pick)
    pub station: Option<String>,
    /// AirDrop receive port
    pub receive_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        let receive_dir = xdg_download_dir();
        Config {
            receive_dir,
            display_name: None,
            restore_always_on: false,
            station: None,
            receive_port: 8771,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let xdg = xdg::BaseDirectories::with_prefix("airdrop-gnome")?;
        let config_path = xdg.get_config_file("config.json");

        if config_path.exists() {
            debug!(path = %config_path.display(), "loading config");
            let data = std::fs::read_to_string(&config_path)?;
            let cfg: Config = serde_json::from_str(&data)?;
            Ok(cfg)
        } else {
            debug!("no config file, using defaults");
            Ok(Config::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let xdg = xdg::BaseDirectories::with_prefix("airdrop-gnome")?;
        let config_path = xdg.place_config_file("config.json")?;
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&config_path, data)?;
        Ok(())
    }
}

fn xdg_download_dir() -> PathBuf {
    // Honour XDG_DOWNLOAD_DIR from user-dirs.dirs, fallback to ~/Downloads
    if let Ok(home) = std::env::var("HOME") {
        let user_dirs = PathBuf::from(&home)
            .join(".config/user-dirs.dirs");
        if user_dirs.exists() {
            if let Ok(content) = std::fs::read_to_string(&user_dirs) {
                for line in content.lines() {
                    if line.starts_with("XDG_DOWNLOAD_DIR=") {
                        let val = line
                            .trim_start_matches("XDG_DOWNLOAD_DIR=")
                            .trim_matches('"')
                            .replace("$HOME", &home);
                        return PathBuf::from(val);
                    }
                }
            }
        }
        PathBuf::from(home).join("Downloads")
    } else {
        PathBuf::from("/tmp")
    }
}
