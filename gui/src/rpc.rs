//! Minimal client for the node's WebSocket API: only the anonymous database API is used.

use serde_json::{json, Value};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;
use tungstenite::{client, Message};

const TIMEOUT: Duration = Duration::from_secs(3);

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
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({"id": id, "method": "call", "params": ["database", method, params]});
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
    pub first_block_time: i64,
}

pub fn chain_info(rpc: &mut Rpc, first_block_time: Option<i64>) -> Result<ChainInfo, String> {
    let props = rpc.database("get_chain_properties", json!([]))?;
    let dgp = rpc.database("get_dynamic_global_properties", json!([]))?;
    let first_block_time = match first_block_time {
        Some(t) => t,
        None => rpc
            .database("get_block_header", json!([1]))?
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_time)
            .unwrap_or(0),
    };
    Ok(ChainInfo {
        chain_id: props.get("chain_id").and_then(Value::as_str).unwrap_or_default().to_string(),
        head_block: dgp.get("head_block_number").and_then(Value::as_u64).unwrap_or(0),
        head_time: dgp.get("time").and_then(Value::as_str).and_then(parse_time).unwrap_or(0),
        irreversible_block: dgp.get("last_irreversible_block_num").and_then(Value::as_u64).unwrap_or(0),
        first_block_time,
    })
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
pub fn sync_percent(first_block_time: i64, head_time: i64, now: i64) -> f64 {
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
    fn progress_by_time() {
        assert_eq!(sync_percent(0, 50, 100), 50.0);
        assert_eq!(sync_percent(0, 120, 100), 100.0);
    }
}
