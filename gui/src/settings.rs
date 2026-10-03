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

/// The folder the app runs from: the node and its data are looked for next to it by default
fn app_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default()
}

fn defaults_in(dir: &Path) -> Settings {
    Settings {
        node_exe: dir.join("witness_node.exe"),
        data_dir: dir.join("witness_node_data_dir"),
        rpc_endpoint: "127.0.0.1:8090".into(),
        start_node_with_app: true,
        language: crate::i18n::DEFAULT_LANG.into(),
    }
}

impl Default for Settings {
    fn default() -> Self {
        defaults_in(&app_dir())
    }
}

impl Settings {
    /// Settings live in the user's profile, not next to the app, so a copy of the app in another folder
    /// reads them too. An empty path in the file means "next to the app"; a saved node path that no longer
    /// exists, while a node sits next to the app, is taken as the app having moved (with the data folder,
    /// if it was the one next to the old node).
    pub fn load(path: &Path) -> Settings {
        let stored = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| Settings { node_exe: PathBuf::new(), data_dir: PathBuf::new(), ..Settings::default() });
        stored.resolved_in(&app_dir())
    }

    fn resolved_in(mut self, dir: &Path) -> Settings {
        let d = defaults_in(dir);
        let moved = !self.node_exe.as_os_str().is_empty() && !self.node_exe.is_file() && d.node_exe.is_file();
        if moved && self.data_dir == self.node_exe.with_file_name("witness_node_data_dir") {
            self.data_dir = d.data_dir.clone();
        }
        if self.node_exe.as_os_str().is_empty() || moved {
            self.node_exe = d.node_exe;
        }
        if self.data_dir.as_os_str().is_empty() {
            self.data_dir = d.data_dir;
        }
        self
    }

    /// Saves paths that are the defaults as empty, so they follow the app when its folder is moved or copied.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let d = Settings::default();
        let mut stored = self.clone();
        if stored.node_exe == d.node_exe {
            stored.node_exe = PathBuf::new();
        }
        if stored.data_dir == d.data_dir {
            stored.data_dir = PathBuf::new();
        }
        let json = serde_json::to_string_pretty(&stored).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("Cannot save settings: {e}"))
    }

    pub fn log_path(&self) -> PathBuf {
        self.data_dir.join("logs").join("default").join("default.log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_app_to_a_new_folder() {
        let root = std::env::temp_dir().join(format!("settings-test-{}", std::process::id()));
        let (old, new) = (root.join("06"), root.join("07"));
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("witness_node.exe"), b"").unwrap();

        // saved by the copy in 06, which is gone; the node now sits next to the app in 07
        let saved = Settings { node_exe: old.join("witness_node.exe"), data_dir: old.join("witness_node_data_dir"), ..defaults_in(&old) };
        let s = saved.resolved_in(&new);
        assert_eq!(s.node_exe, new.join("witness_node.exe"));
        assert_eq!(s.data_dir, new.join("witness_node_data_dir"));

        // a data folder chosen elsewhere stays
        let custom = root.join("chain-data");
        let saved = Settings { node_exe: old.join("witness_node.exe"), data_dir: custom.clone(), ..defaults_in(&old) };
        assert_eq!(saved.resolved_in(&new).data_dir, custom);

        // empty means next to the app
        let saved = Settings { node_exe: PathBuf::new(), data_dir: PathBuf::new(), ..defaults_in(&old) };
        assert_eq!(saved.resolved_in(&new).node_exe, new.join("witness_node.exe"));
        std::fs::remove_dir_all(&root).ok();
    }
}
