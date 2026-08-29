use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    #[default]
    Normal,
    Minimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lyrics_backdrop: bool,
    pub lyrics_motion: Motion,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lyrics_backdrop: true,
            lyrics_motion: Motion::Normal,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        crate::config_dir()
            .ok()
            .and_then(|dir| std::fs::read_to_string(dir.join("settings.json")).ok())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Ok(dir) = crate::config_dir() else {
            return;
        };
        let Ok(json) = serde_json::to_string_pretty(self) else {
            return;
        };
        let path = dir.join("settings.json");
        let staging = path.with_extension("part");
        if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&staging, json).is_ok() {
            let _ = std::fs::rename(&staging, &path);
        }
    }
}
