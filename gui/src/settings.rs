//! What the app needs to run the node, kept as JSON in the app's config folder.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub node_exe: PathBuf,
    pub data_dir: PathBuf,
    /// Passed as --rpc-endpoint; anything but 127.0.0.1 opens the API to the network
    pub rpc_endpoint: String,
    pub start_node_with_app: bool,
    /// UI language code, see i18n.rs
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        // Next to the app, the way the node is unpacked today
        let dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        Settings {
            node_exe: dir.join("witness_node.exe"),
            data_dir: dir.join("witness_node_data_dir"),
            rpc_endpoint: "127.0.0.1:8090".into(),
            start_node_with_app: true,
            language: crate::i18n::DEFAULT_LANG.into(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("Cannot save settings: {e}"))
    }

    pub fn log_path(&self) -> PathBuf {
        self.data_dir.join("logs").join("default").join("default.log")
    }
}
