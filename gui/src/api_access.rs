//! An API account for the app, so it can list the node's peers.
//!
//! Anonymous clients get only the database, broadcast, history and orders APIs; the peer list lives in the
//! network_node API. When config.ini does not choose an --api-access file of its own, the app writes one into
//! the data folder: the node's default anonymous access ("*"), unchanged, plus a user for the app with a fresh
//! random password that also has network_node_api. The password goes to the lock file next to it, so a later
//! run of the app that takes the node over can log in too.

use std::path::{Path, PathBuf};

use base64::Engine;
use sha2::{Digest, Sha256};

pub const FILE: &str = "graphene-node-gui-api.json";
pub const USER: &str = "graphene-node-gui";
/// What the node grants anonymous clients when it has no api-access file (application.cpp)
const DEFAULT_APIS: [&str; 4] = ["database_api", "network_broadcast_api", "history_api", "orders_api"];

/// True if config.ini sets api-access itself; the app then leaves the node's API permissions alone.
pub fn config_sets_api_access(data_dir: &Path) -> bool {
    std::fs::read_to_string(data_dir.join("config.ini"))
        .map(|text| text.lines().any(sets_api_access))
        .unwrap_or(false)
}

fn sets_api_access(line: &str) -> bool {
    let line = line.trim_start();
    !line.starts_with('#')
        && line.split_once('=').is_some_and(|(k, v)| k.trim() == "api-access" && !v.trim().is_empty())
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

/// The file's content for a password and salt; the node checks sha256(password + salt).
fn file_content(password: &str, salt: &[u8]) -> serde_json::Value {
    let b64 = base64::engine::general_purpose::STANDARD;
    let hash = Sha256::new().chain_update(password.as_bytes()).chain_update(salt).finalize();
    let mut gui_apis: Vec<&str> = DEFAULT_APIS.to_vec();
    gui_apis.push("network_node_api");
    serde_json::json!({
        "permission_map": [
            ["*", { "password_hash_b64": "*", "password_salt_b64": "*", "allowed_apis": DEFAULT_APIS }],
            [USER, { "password_hash_b64": b64.encode(hash), "password_salt_b64": b64.encode(salt),
                     "allowed_apis": gui_apis }],
        ]
    })
}

/// Writes a new access file into the data folder; returns its path and the app's password.
pub fn write(data_dir: &Path) -> Result<(PathBuf, String), String> {
    let password: String = random_bytes::<16>()?.iter().map(|b| format!("{b:02x}")).collect();
    let salt = random_bytes::<16>()?;
    let path = data_dir.join(FILE);
    let text = serde_json::to_string_pretty(&file_content(&password, &salt)).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((path, password))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_api_access_in_config() {
        assert!(sets_api_access("api-access = C:\\node\\access.json"));
        assert!(sets_api_access("  api-access=access.json"));
        assert!(!sets_api_access("# api-access = access.json"));
        assert!(!sets_api_access("api-access ="));
        assert!(!sets_api_access("api-access-x = 1"));
    }

    #[test]
    fn hash_matches_the_node() {
        // sha256("pw" + "salt"), as login_api::login computes it
        let v = file_content("pw", b"salt");
        let gui = &v["permission_map"][1][1];
        assert_eq!(gui["password_salt_b64"], "c2FsdA==");
        assert_eq!(gui["password_hash_b64"], "/lAC46G6SKmC98Mf7HIGXRtFFUfOIpCnZrpHe/7DIYI=");
        assert_eq!(v["permission_map"][0][0], "*");
    }
}
