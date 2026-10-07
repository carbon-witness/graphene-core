//! Seed nodes: the node's built-in list and the seed-node entries of config.ini.
//!
//! The built-in list is the file the node compiles in (libraries/egenesis/seed-nodes.txt), so the app shows the
//! same seeds the node it ships with uses. Seeds the user adds go to config.ini as "seed-node = host:port", where
//! the node reads them at every start; a running node also gets them at once through network_node.add_node.
//! A "seed-nodes = [...]" entry in config.ini replaces the built-in list.

use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;

const SEED_NODES_TXT: &str = include_str!("../../libraries/egenesis/seed-nodes.txt");

/// The quoted "host:port" entries of seed-nodes.txt, which is a C++ initializer list with comments.
pub fn defaults() -> Vec<String> {
    SEED_NODES_TXT
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .filter_map(|l| {
            let start = l.find('"')? + 1;
            let len = l[start..].find('"')?;
            Some(l[start..start + len].to_string())
        })
        .collect()
}

/// "host:port" with a non-empty host and a port from 1 to 65535.
pub fn is_valid(addr: &str) -> bool {
    match addr.rsplit_once(':') {
        Some((host, port)) => {
            !host.is_empty()
                && !host.contains(|c: char| c.is_whitespace() || c == '/')
                && port.parse::<u16>().is_ok_and(|p| p > 0)
        }
        None => false,
    }
}

/// The IPv4 endpoints a seed resolves to, as the node writes them ("1.2.3.4:1776"); the node's P2P is IPv4 only.
pub fn resolve(addr: &str) -> Vec<String> {
    addr.to_socket_addrs()
        .map(|it| it.filter(SocketAddr::is_ipv4).map(|a| a.to_string()).collect())
        .unwrap_or_default()
}

/// The value of an active (not commented out) "key = value" line of config.ini
fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (k, v) = line.trim_start().split_once('=')?;
    (!k.starts_with('#') && k.trim() == key).then(|| v.trim()).filter(|v| !v.is_empty())
}

/// A commented-out "# key = ..." line, as in the template config.ini the node writes
fn is_template_of(line: &str, key: &str) -> bool {
    line.trim_start()
        .strip_prefix('#')
        .and_then(|rest| rest.split_once('='))
        .is_some_and(|(k, _)| k.trim() == key)
}

/// Seeds in config.ini: the seed-node entries, and the seed-nodes list if there is one (it replaces the
/// built-in seeds).
pub struct ConfigSeeds {
    pub seed_node: Vec<String>,
    pub seed_nodes: Option<Vec<String>>,
}

pub fn read_config(text: &str) -> ConfigSeeds {
    let seed_node = text.lines().filter_map(|l| value_of(l, "seed-node")).map(str::to_string).collect();
    let seed_nodes = text
        .lines()
        .filter_map(|l| value_of(l, "seed-nodes"))
        .last()
        .and_then(|v| serde_json::from_str::<Vec<String>>(v).ok());
    ConfigSeeds { seed_node, seed_nodes }
}

pub fn config_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("config.ini")
}

pub fn load_config(data_dir: &Path) -> ConfigSeeds {
    read_config(&std::fs::read_to_string(config_path(data_dir)).unwrap_or_default())
}

/// config.ini with "seed-node = addr" added: after the last active seed-node line, else under the commented
/// template line the node writes ("# seed-node = "), else at the end.
pub fn with_seed(text: &str, addr: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    let new_line = format!("seed-node = {addr}");
    let at = lines
        .iter()
        .rposition(|l| value_of(l, "seed-node").is_some())
        .or_else(|| lines.iter().position(|l| is_template_of(l, "seed-node")))
        .map(|i| i + 1)
        .unwrap_or(lines.len());
    lines.insert(at, &new_line);
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// config.ini without the active "seed-node = addr" lines
pub fn without_seed(text: &str, addr: &str) -> String {
    let mut out: String = text
        .lines()
        .filter(|l| value_of(l, "seed-node") != Some(addr))
        .collect::<Vec<_>>()
        .join("\n");
    out.push('\n');
    out
}

pub fn add_to_config(data_dir: &Path, addr: &str) -> Result<(), String> {
    let path = config_path(data_dir);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::write(&path, with_seed(&text, addr)).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn remove_from_config(data_dir: &Path, addr: &str) -> Result<(), String> {
    let path = config_path(data_dir);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(&path, without_seed(&text, addr)).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = "# P2P nodes to connect to on startup (may specify multiple times)\n# seed-node = \n\n\
                            # JSON array of P2P nodes to connect to on startup\n# seed-nodes = \n";

    #[test]
    fn edits_config_ini() {
        let one = with_seed(TEMPLATE, "1.2.3.4:1776");
        assert!(one.starts_with("# P2P nodes to connect to on startup (may specify multiple times)\n# seed-node = \nseed-node = 1.2.3.4:1776\n"), "{one}");
        let two = with_seed(&one, "h.example:4646");
        assert_eq!(read_config(&two).seed_node, vec!["1.2.3.4:1776", "h.example:4646"]);
        assert!(read_config(&two).seed_nodes.is_none());
        let back = without_seed(&two, "1.2.3.4:1776");
        assert_eq!(read_config(&back).seed_node, vec!["h.example:4646"]);
        assert_eq!(without_seed(&back, "h.example:4646"), TEMPLATE);
        assert_eq!(with_seed("", "1.2.3.4:1"), "seed-node = 1.2.3.4:1\n");
    }

    #[test]
    fn reads_seed_nodes_list() {
        let c = read_config("seed-nodes = [\"a:1\", \"b:2\"]\n# seed-node = x:1\n");
        assert_eq!(c.seed_nodes, Some(vec!["a:1".to_string(), "b:2".to_string()]));
        assert!(c.seed_node.is_empty());
    }

    #[test]
    fn reads_the_built_in_list() {
        let d = defaults();
        assert!(!d.is_empty());
        assert!(d.iter().all(|s| is_valid(s)), "{d:?}");
    }

    #[test]
    fn checks_addresses() {
        assert!(is_valid("1.2.3.4:1776"));
        assert!(is_valid("seed.example.org:4646"));
        assert!(!is_valid("1.2.3.4"));
        assert!(!is_valid("1.2.3.4:0"));
        assert!(!is_valid("1.2.3.4:70000"));
        assert!(!is_valid(":1776"));
        assert!(!is_valid("http://x:1"));
    }

    #[test]
    fn resolves_an_ip() {
        assert_eq!(resolve("1.2.3.4:1776"), vec!["1.2.3.4:1776".to_string()]);
    }
}
