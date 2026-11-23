//! Per-user layout preferences: region splits. `$XDG_CONFIG_HOME/ventosa/ui.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Share of the first region, per split id.
    pub splits: HashMap<String, f32>,
    /// The graph canvas view per workspace: [zoom, pan x, pan y].
    pub views: HashMap<String, [f32; 3]>,
}

fn path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("ventosa").join("ui.json")
}

impl Prefs {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let p = path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }
    pub fn split(&self, id: &str, default: f32) -> f32 {
        self.splits.get(id).copied().unwrap_or(default)
    }
    pub fn set_split(&mut self, id: &str, share: f32) {
        self.splits.insert(id.to_string(), share);
    }
    pub fn reset_split(&mut self, id: &str) {
        self.splits.remove(id);
        self.save();
    }
}
