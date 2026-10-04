//! Minimal client for the node's WebSocket API: the anonymous database API, and the peer list of the
//! network_node API after a login (api_access.rs).

use serde_json::{json, Value};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;
use tungstenite::{client, Message};

// The node answers API calls on the thread that also applies blocks, so during sync an answer can take seconds
const TIMEOUT: Duration = Duration::from_secs(10);

pub struct Rpc {
    ws: tungstenite::WebSocket<TcpStream>,
    next_id: u64,
}

impl Rpc {
    pub fn connect(endpoint: &str) -> Result<Rpc, String> {
        let addr: SocketAddr = endpoint
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or("no address")?;
        let stream = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
        stream.set_read_timeout(Some(TIMEOUT)).ok();
        stream.set_write_timeout(Some(TIMEOUT)).ok();
        let (ws, _) = client(format!("ws://{endpoint}"), stream).map_err(|e| e.to_string())?;
        Ok(Rpc { ws, next_id: 1 })
    }

    pub fn database(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.call(json!("database"), method, params)
    }

    /// Logs in on this connection (API 1 is the login API); false for a wrong user or password.
    pub fn login(&mut self, user: &str, password: &str) -> Result<bool, String> {
        Ok(self.call(json!(1), "login", json!([user, password]))?.as_bool().unwrap_or(false))
    }

    fn call(&mut self, api: Value, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({"id": id, "method": "call", "params": [api, method, params]});
        self.ws.send(Message::text(req.to_string())).map_err(|e| e.to_string())?;
        loop {
            let msg = self.ws.read().map_err(|e| e.to_string())?;
            let Message::Text(text) = msg else { continue };
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if v.get("id").and_then(Value::as_u64) != Some(id) {
                continue; // a notice, not our answer
            }
            if let Some(err) = v.get("error") {
                return Err(err.get("message").and_then(Value::as_str).unwrap_or("RPC error").to_string());
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

/// Chain head as the API reports it.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct ChainInfo {
    pub chain_id: String,
    pub head_block: u64,
    pub head_time: i64,
    pub irreversible_block: u64,
    /// Time of block 1; None until the node has it
    pub first_block_time: Option<i64>,
}

pub fn chain_info(rpc: &mut Rpc, first_block_time: Option<i64>) -> Result<ChainInfo, String> {
    let props = rpc.database("get_chain_properties", json!([]))?;
    let dgp = rpc.database("get_dynamic_global_properties", json!([]))?;
    let head_block = dgp.get("head_block_number").and_then(Value::as_u64).unwrap_or(0);
    let first_block_time = match first_block_time {
        Some(t) => Some(t),
        None if head_block == 0 => None,
        None => rpc
            .database("get_block_header", json!([1]))?
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_time),
    };
    Ok(ChainInfo {
        chain_id: props.get("chain_id").and_then(Value::as_str).unwrap_or_default().to_string(),
        head_block,
        head_time: dgp.get("time").and_then(Value::as_str).and_then(parse_time).unwrap_or(0),
        irreversible_block: dgp.get("last_irreversible_block_num").and_then(Value::as_u64).unwrap_or(0),
        first_block_time,
    })
}

/// A connected P2P peer, from network_node.get_connected_peers
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Peer {
    pub addr: String,
    pub inbound: bool,
    pub user_agent: String,
    pub platform: String,
    /// Unix seconds
    pub connected_since: i64,
    pub last_received: i64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub head_block: u64,
}

pub fn peers(rpc: &mut Rpc) -> Result<Vec<Peer>, String> {
    let list = rpc.call(json!("network_node"), "get_connected_peers", json!([]))?;
    Ok(list.as_array().map(|a| a.iter().map(parse_peer).collect()).unwrap_or_default())
}

fn parse_peer(p: &Value) -> Peer {
    let info = &p["info"];
    let s = |k: &str| info.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let n = |k: &str| info.get(k).and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok())).unwrap_or(0);
    let time = |k: &str| match info.get(k) {
        Some(Value::String(t)) => parse_time(t).unwrap_or(0),
        Some(v) => v.as_i64().unwrap_or(0),
        None => 0,
    };
    Peer {
        addr: s("addr"),
        inbound: info.get("inbound").and_then(Value::as_bool).unwrap_or(false),
        user_agent: s("subver"),
        platform: s("platform"),
        connected_since: time("conntime"),
        last_received: time("lastrecv"),
        bytes_sent: n("bytessent"),
        bytes_received: n("bytesrecv"),
        head_block: n("current_head_block_number"),
    }
}

/// "2021-04-14T21:02:45" (UTC, as the node prints it) to Unix seconds.
pub fn parse_time(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    // days from civil, Howard Hinnant's algorithm
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

/// Sync progress by time: the number of blocks still to come is not known ahead.
/// Without block 1 (a fresh node) nothing is synced yet.
pub fn sync_percent(first_block_time: Option<i64>, head_time: i64, now: i64) -> f64 {
    let Some(first_block_time) = first_block_time else { return 0.0 };
    if now <= first_block_time {
        return 100.0;
    }
    (((head_time - first_block_time) as f64 / (now - first_block_time) as f64) * 100.0).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_time() {
        assert_eq!(parse_time("1970-01-01T00:00:00"), Some(0));
        assert_eq!(parse_time("2021-04-14T21:02:45"), Some(1618434165));
        assert_eq!(parse_time("bad"), None);
    }

    #[test]
    fn reads_a_peer() {
        let p = parse_peer(&json!({"version": 0, "host": "1.2.3.4:1776", "info": {
            "addr": "1.2.3.4:1776", "inbound": false, "subver": "Graphene Reference Implementation",
            "platform": "linux", "conntime": "2026-10-04T10:00:00", "lastrecv": 1791108000,
            "bytessent": 1200, "bytesrecv": "3400", "current_head_block_number": 55000000}}));
        assert_eq!(p.addr, "1.2.3.4:1776");
        assert!(!p.inbound);
        assert_eq!(p.connected_since, parse_time("2026-10-04T10:00:00").unwrap());
        assert_eq!(p.last_received, 1791108000);
        assert_eq!((p.bytes_sent, p.bytes_received, p.head_block), (1200, 3400, 55000000));
    }

    #[test]
    fn progress_by_time() {
        assert_eq!(sync_percent(Some(0), 50, 100), 50.0);
        assert_eq!(sync_percent(Some(0), 120, 100), 100.0);
        // A fresh node reports the genesis time as its head time: 0 %, not head_time / now
        assert_eq!(sync_percent(None, 1_618_000_000, 1_791_000_000), 0.0);
    }
}
