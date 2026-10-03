//! Follows the node's default.log. While the database opens or replays the RPC port is closed, so the log is
//! the only way to tell "loading" from "dead"; it also feeds the Journal tab.

use regex::Regex;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::OnceLock;

const KEEP_LINES: usize = 5000;
/// On first open, start this far from the end instead of reading a long log from the start
const FIRST_READ_BYTES: u64 = 256 * 1024;

/// Hides WIF private keys: one reached a log through an assert message on 2026-09-16.
pub fn mask_wif(line: &str) -> String {
    static WIF: OnceLock<Regex> = OnceLock::new();
    let re = WIF.get_or_init(|| Regex::new(r"\b5[1-9A-HJ-NP-Za-km-z]{50}\b").unwrap());
    re.replace_all(line, "5****…").into_owned()
}

/// What the node is doing, as far as its log says.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct LogStage {
    /// "opening", "replaying", "started" or "" before anything is known
    pub stage: String,
    pub replay_percent: Option<f64>,
    pub chain_id: Option<String>,
    pub last_block: Option<u64>,
    /// The node's last "Shutdown: <step>" line, while it is stopping
    pub shutdown_step: Option<String>,
}

pub struct LogTail {
    path: PathBuf,
    pos: u64,
    opened: bool,
    partial: String,
    lines: VecDeque<(u64, String)>,
    next_seq: u64,
    pub stage: LogStage,
}

impl LogTail {
    pub fn new(path: PathBuf) -> LogTail {
        LogTail { path, pos: 0, opened: false, partial: String::new(), lines: VecDeque::new(), next_seq: 1, stage: LogStage::default() }
    }

    /// Reads what was appended since the last call. A file shorter than before was rotated or recreated.
    pub fn poll(&mut self) {
        let Ok(mut f) = File::open(&self.path) else { return };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if !self.opened {
            self.pos = len.saturating_sub(FIRST_READ_BYTES);
            self.opened = true;
        } else if len < self.pos {
            self.pos = 0;
            self.partial.clear();
        }
        if len == self.pos || f.seek(SeekFrom::Start(self.pos)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if f.take(len - self.pos).read_to_end(&mut buf).is_err() {
            return;
        }
        self.pos += buf.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&buf));
        while let Some(i) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=i).collect();
            self.push_line(line.trim_end_matches(['\r', '\n']));
        }
    }

    /// Restarts the stage tracking, e.g. when a new node process starts writing to the same log.
    pub fn reset_stage(&mut self) {
        self.stage = LogStage::default();
    }

    fn push_line(&mut self, line: &str) {
        let line = mask_wif(line);
        update_stage(&mut self.stage, &line);
        self.lines.push_back((self.next_seq, line));
        self.next_seq += 1;
        while self.lines.len() > KEEP_LINES {
            self.lines.pop_front();
        }
    }

    /// Lines after `cursor` (a sequence number from an earlier call), at most `limit` of the newest.
    pub fn lines_after(&self, cursor: u64, limit: usize) -> Vec<(u64, String)> {
        let new: Vec<_> = self.lines.iter().filter(|(s, _)| *s > cursor).cloned().collect();
        let skip = new.len().saturating_sub(limit);
        new.into_iter().skip(skip).collect()
    }

    pub fn tail_text(&self, n: usize) -> String {
        let skip = self.lines.len().saturating_sub(n);
        self.lines.iter().skip(skip).map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n")
    }
}

fn update_stage(st: &mut LogStage, line: &str) {
    static RE: OnceLock<[Regex; 4]> = OnceLock::new();
    let [replay_pct, started, chain, got] = RE.get_or_init(|| {
        [
            Regex::new(r"\[by num: ([0-9.]+)%").unwrap(),
            Regex::new(r"Started Graphene node on a chain with (\d+) blocks").unwrap(),
            Regex::new(r"Chain ID is ([0-9a-f]{64})").unwrap(),
            Regex::new(r"Got block: #(\d+)").unwrap(),
        ]
    });
    if let Some(i) = line.find("] Shutdown: ") {
        let step = &line[i + "] Shutdown: ".len()..];
        st.shutdown_step = Some(step.split('\t').next().unwrap_or(step).trim().to_string());
    } else if line.contains("Opening object database") {
        *st = LogStage { stage: "opening".into(), ..LogStage::default() };
    } else if line.contains("Replaying blocks") {
        st.stage = "replaying".into();
        st.replay_percent = Some(0.0);
    } else if let Some(c) = replay_pct.captures(line) {
        st.stage = "replaying".into();
        st.replay_percent = c[1].parse().ok();
    } else if let Some(c) = started.captures(line) {
        st.stage = "started".into();
        st.replay_percent = None;
        st.last_block = c[1].parse().ok();
    } else if let Some(c) = chain.captures(line) {
        st.chain_id = Some(c[1].to_string());
    } else if let Some(c) = got.captures(line) {
        st.last_block = c[1].parse().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_wif_only() {
        let wif = "5JRcMZJjjRHnM7YJbgZ5HzC1X7r1EHiChuMaFXr8QY3guEMWfhA";
        let pubkey = "GPH6MRyAjQq8ud7hVNYcfnVPJqcVpscN5So8BhtHuGYqET5GDW5CV";
        let line = format!("Invalid key {wif} for {pubkey}");
        assert_eq!(mask_wif(&line), format!("Invalid key 5****… for {pubkey}"));
        let id = "Chain ID is 7fcf452d6bb058949cdc875b13c8908c8f54b0f264c39faf8152b682af0740ee";
        assert_eq!(mask_wif(id), id);
    }

    #[test]
    fn follows_stages() {
        let mut st = LogStage::default();
        for l in [
            "open ] Opening object database from C:/x/blockchain ...",
            "reindex ] Replaying blocks, starting at 202069...",
            "reindex ]    [by size: 93.86555%   23780055 of 25334167]   [by num: 93.80235%   210000 of 223875]",
        ] {
            update_stage(&mut st, l);
        }
        assert_eq!(st.stage, "replaying");
        assert_eq!(st.replay_percent, Some(93.80235));
        update_stage(&mut st, "main ] Started Graphene node on a chain with 223875 blocks.");
        update_stage(&mut st, "main ] Chain ID is 7fcf452d6bb058949cdc875b13c8908c8f54b0f264c39faf8152b682af0740ee");
        update_stage(&mut st, "handle_block ] Got block: #230000 0003827 time: 2021-04-14T21:02:45 transaction(s): 0");
        assert_eq!(st.stage, "started");
        assert_eq!(st.last_block, Some(230000));
        assert_eq!(st.chain_id.as_deref().map(|s| &s[..8]), Some("7fcf452d"));
    }

    #[test]
    fn notes_shutdown_steps() {
        let mut st = LogStage::default();
        update_stage(&mut st, "2026-10-03T19:13:58 th_a:?unnamed?  shutdown info  ] Shutdown: closing the chain database\t\t\tapplication.cpp:1226");
        assert_eq!(st.shutdown_step.as_deref(), Some("closing the chain database"));
    }

    #[test]
    fn tails_appends_and_rotation() {
        let dir = std::env::temp_dir().join(format!("logtail-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("default.log");
        std::fs::write(&p, "a\nb\npart").unwrap();
        let mut t = LogTail::new(p.clone());
        t.poll();
        assert_eq!(t.lines_after(0, 10).iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        std::fs::write(&p, "a\nb\npartial\nc\n").unwrap();
        t.poll();
        assert_eq!(t.lines_after(2, 10).iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["partial", "c"]);
        std::fs::write(&p, "new\n").unwrap(); // rotated: shorter than before
        t.poll();
        assert_eq!(t.lines_after(4, 10)[0].1, "new");
        std::fs::remove_dir_all(&dir).ok();
    }
}
