use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Playlist {
    pub name: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub playlists: Vec<Playlist>
}

fn config_path() -> PathBuf {
    let mut dir = dirs::config_dir().expect("could not find config directory");
    dir.push("playlist-fetcher");
    std::fs::create_dir_all(&dir).ok();
    dir.push("config.json");
    dir
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) {
        let path = config_path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}
