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
    /// The release the peer runs and the commit it was built from, see peer_version
    pub version: String,
    pub build: String,
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
        version: String::new(),
        build: String::new(),
        user_agent: s("subver"),
        platform: s("platform"),
        connected_since: time("conntime"),
        last_received: time("lastrecv"),
        bytes_sent: n("bytessent"),
        bytes_received: n("bytesrecv"),
        head_block: n("current_head_block_number"),
    }
    .with_version(time("fc_git_revision_unix_timestamp"))
}

impl Peer {
    fn with_version(mut self, fc_time: i64) -> Peer {
        (self.version, self.build) = peer_version(&self.user_agent, fc_time);
        self
    }
}

/// An endpoint the node knows of, from network_node.get_potential_peers
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct PotentialPeer {
    pub addr: String,
    /// never_attempted_to_connect, last_connection_failed, last_connection_rejected,
    /// last_connection_handshaking_failed or last_connection_succeeded
    pub disposition: String,
    pub last_attempt: i64,
    pub failures: u32,
    pub error: String,
    /// From a handshake that stalled ("Terminating handshaking connection due to inactivity"): how long it
    /// waited, the step it stopped at (the peer connection's negotiation status) and the bytes each way
    pub stalled: Option<Stall>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Stall {
    pub timeout: u64,
    pub stage: String,
    pub sent: u64,
    pub received: u64,
}

/// The fields of the node's "Terminating handshaking connection due to inactivity" error, from its log data
fn stall_of(error: &Value) -> Option<Stall> {
    let data = error.get("stack")?.as_array()?.iter().find_map(|e| {
        let d = e.get("data")?;
        d.get("status").is_some().then_some(d)
    })?;
    let n = |k: &str| data.get(k).and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok())).unwrap_or(0);
    Some(Stall {
        timeout: n("timeout"),
        stage: data.get("status").and_then(Value::as_str).unwrap_or_default().to_string(),
        sent: n("sent"),
        received: n("received"),
    })
}

pub fn potential_peers(rpc: &mut Rpc) -> Result<Vec<PotentialPeer>, String> {
    let list = rpc.call(json!("network_node"), "get_potential_peers", json!([]))?;
    let one = |p: &Value| PotentialPeer {
        addr: p.get("endpoint").and_then(Value::as_str).unwrap_or_default().to_string(),
        disposition: p.get("last_connection_disposition").and_then(Value::as_str).unwrap_or_default().to_string(),
        last_attempt: p.get("last_connection_attempt_time").and_then(Value::as_str).and_then(parse_time).unwrap_or(0),
        failures: p.get("number_of_failed_connection_attempts").and_then(Value::as_u64).unwrap_or(0) as u32,
        error: p.get("last_error").map(exception_text).unwrap_or_default(),
        stalled: p.get("last_error").and_then(stall_of),
    };
    Ok(list.as_array().map(|a| a.iter().map(one).collect()).unwrap_or_default())
}

/// The text of an fc::exception as JSON. Its "message" is often just the generic "unspecified"; the reason
/// (e.g. "Connection reset by peer", "disconnecting because we never received a hello") is in the format strings
/// of its log stack, with ${name} placeholders filled from their data.
pub fn exception_text(e: &Value) -> String {
    let fill = |entry: &Value| -> Option<String> {
        let mut text = entry.get("format")?.as_str()?.to_string();
        if let Some(data) = entry.get("data").and_then(Value::as_object) {
            for (k, v) in data {
                let v = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
                text = text.replace(&format!("${{{k}}}"), &v);
            }
        }
        let text = text.trim().to_string();
        (!text.is_empty()).then_some(text)
    };
    let stack: Vec<String> = e
        .get("stack")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(fill).collect())
        .unwrap_or_default();
    if !stack.is_empty() {
        return stack.join("; ");
    }
    e.get("message").or_else(|| e.get("name")).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Asks the running node to try an endpoint ("1.2.3.4:1776"); it joins the node's list of potential peers.
pub fn add_node(rpc: &mut Rpc, endpoint: &str) -> Result<(), String> {
    rpc.call(json!("network_node"), "add_node", json!([endpoint])).map(|_| ())
}

const GRAPHENE_AGENT: &str = "Graphene Reference Implementation";
const BITSHARES_AGENT: &str = "BitShares Reference Implementation";
/// Release builds before 1.2.1, by the commit time of the fc library they report (the P2P hello carries no
/// version): release, graphene-core commit of its tag. 1.2.0 was tagged in two repositories, on different
/// fc commits.
const RELEASE_FC_TIMES: [(i64, &str, &str); 5] = [
    (1569050266, "1.0", "23df6159"),   // graphene-fc 6d8d030
    (1789458113, "1.1 carbon-build", "a108c380"), // carbon-witness/graphene-fc a108c38, the build is the fc commit
    (1789991063, "1.1", "fd7c7dff"),   // graphene-fc f17ef47
    (1790499127, "1.2.0", "d5a98f9a"), // carbon-witness/graphene-fc 551377b
    (1790609551, "1.2.0", "9a37e5a9"), // graphene-blockchain/graphene-fc 0a5fcbe
];

/// The release a peer runs and the graphene-core commit it was built from. From 1.2.1 on, the user agent ends
/// with the build string ("... 1.2.1-286e0801"); older releases are recognised by their fc revision time, and
/// by the user agent: up to 1.1 the node called itself BitShares. "?" marks what cannot be told.
pub fn peer_version(user_agent: &str, fc_time: i64) -> (String, String) {
    if let Some(build) = user_agent.strip_prefix(GRAPHENE_AGENT).map(str::trim).filter(|b| !b.is_empty()) {
        return match build.rsplit_once('-') {
            Some((v, commit)) => (v.to_string(), commit.to_string()),
            None => (build.to_string(), "?".into()),
        };
    }
    if let Some((_, v, commit)) = RELEASE_FC_TIMES.iter().find(|(t, _, _)| *t == fc_time) {
        return (v.to_string(), commit.to_string());
    }
    let v = match user_agent {
        GRAPHENE_AGENT => "1.2.0?",
        BITSHARES_AGENT => "≤ 1.1?",
        _ => "?",
    };
    (v.into(), "?".into())
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
    fn tells_releases_apart() {
        let v = |a: &str, t: i64| {
            let (v, b) = peer_version(a, t);
            format!("{v} {b}")
        };
        assert_eq!(v("Graphene Reference Implementation 1.2.1-286e0801", 0), "1.2.1 286e0801");
        assert_eq!(v("Graphene Reference Implementation", 1790499127), "1.2.0 d5a98f9a");
        assert_eq!(v("Graphene Reference Implementation", 1790609551), "1.2.0 9a37e5a9");
        assert_eq!(v("BitShares Reference Implementation", 1789991063), "1.1 fd7c7dff");
        assert_eq!(v("BitShares Reference Implementation", 1569050266), "1.0 23df6159");
        assert_eq!(v("BitShares Reference Implementation", 1789458113), "1.1 carbon-build a108c380");
        assert_eq!(v("BitShares Reference Implementation", 1), "≤ 1.1? ?");
        assert_eq!(v("Graphene Reference Implementation", 1), "1.2.0? ?");
        assert_eq!(v("Something else", 1), "? ?");
    }

    #[test]
    fn reads_exception_reasons() {
        let e = json!({"code": 0, "name": "exception", "message": "unspecified", "stack": [
            {"context": {"level": "info"}, "format": "disconnecting because ${r}", "data": {"r": "we never received a hello"}},
            {"context": {}, "format": "", "data": {}}]});
        assert_eq!(exception_text(&e), "disconnecting because we never received a hello");
        assert_eq!(exception_text(&json!({"message": "unspecified", "stack": []})), "unspecified");
        let stalled = json!({"stack": [{"format": "Terminating handshaking connection due to inactivity of ${timeout} seconds.",
            "data": {"timeout": 5, "status": "peer_connection_accepted", "sent": 656, "received": 576}}]});
        let st = stall_of(&stalled).unwrap();
        assert_eq!((st.timeout, st.stage.as_str(), st.sent, st.received), (5, "peer_connection_accepted", 656, 576));
        assert!(stall_of(&e).is_none());
    }

    #[test]
    fn progress_by_time() {
        assert_eq!(sync_percent(Some(0), 50, 100), 50.0);
        assert_eq!(sync_percent(Some(0), 120, 100), 100.0);
        // A fresh node reports the genesis time as its head time: 0 %, not head_time / now
        assert_eq!(sync_percent(None, 1_618_000_000, 1_791_000_000), 0.0);
    }
}
