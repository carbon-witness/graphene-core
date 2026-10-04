//! Runs witness_node.exe and keeps it running.
//!
//! The node is a separate process that outlives this app: a crash of the app must not take the node with it,
//! so it is not put in a kill-on-close job and gets no --parent-pid. The app finds a running node again through
//! a lock file in the data folder and stops it through the named event the node was started with.

use crate::i18n::{self, tr, trf};
use crate::logtail::{LogStage, LogTail};
use crate::api_access;
use crate::rpc::{self, ChainInfo, Peer, PotentialPeer, Rpc};
use crate::seeds;
use std::collections::HashMap;
use crate::settings::Settings;
use crate::win::{self, Event, Process};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How long a clean stop may take before the user is asked whether to kill the node
pub const STOP_TIMEOUT: Duration = Duration::from_secs(60);
/// Watchdog: pauses before restarts 1, 2, 3; the third crash within CRASH_WINDOW stops the restarts
const RESTART_DELAYS: [u64; 3] = [15, 60, 300];
const CRASH_WINDOW: Duration = Duration::from_secs(600);
const MAX_CRASHES: usize = 3;
/// Head this close to the wall clock counts as synced
const SYNCED_LAG_SECS: i64 = 60;
/// Behind and no new block for this long: the node most likely has no peers
const NO_BLOCKS_SECS: u64 = 90;
const LOCK_FILE: &str = "graphene-node-gui.lock";
/// The node's last shutdown step before its process ends (witness_node main.cpp)
const SHUTDOWN_DONE: &str = "done, exiting the process";
/// How long after the stop request a node that logged SHUTDOWN_DONE may take to exit before it is ended
const EXIT_GRACE: Duration = Duration::from_secs(10);
/// witness_node's exit code when another node holds its data directory (data_dir_lock.hpp)
const EXIT_DATA_DIR_IN_USE: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Stopped,
    Starting,
    Running,
    Stopping,
    /// The node did not exit within STOP_TIMEOUT; only killing it is left
    StopTimedOut,
    WaitingRestart,
    /// Crashed MAX_CRASHES times within CRASH_WINDOW; no more automatic restarts
    Failed,
}

#[derive(Serialize, Deserialize)]
struct LockFile {
    pid: u32,
    event: String,
    rpc_endpoint: String,
    /// The app's API password (api_access.rs); missing in lock files of older versions
    #[serde(default)]
    rpc_password: Option<String>,
}

/// The last thing that happened to the node, kept untranslated so a language switch applies to it too
#[derive(Clone)]
enum LastEvent {
    Exited(Option<u32>),
    /// The node refused to start: another node holds its data folder
    DataDirInUse,
    GaveUp(Option<u32>),
    Error(String),
}

impl LastEvent {
    fn render(&self, lang: &str) -> String {
        match self {
            LastEvent::Exited(code) => trf(lang, "last.exit", &[("what", describe_exit(lang, *code))]),
            LastEvent::GaveUp(code) => trf(lang, "last.gave_up", &[("what", describe_exit(lang, *code))]),
            LastEvent::Error(e) => e.clone(),
            LastEvent::DataDirInUse => tr(lang, "err.data_dir_in_use"),
        }
    }
}

struct Node {
    process: Process,
    /// None when attached to a node whose event could not be opened: it can only be killed
    event: Option<Event>,
}

struct Inner {
    settings: Settings,
    want_running: bool,
    node: Option<Node>,
    phase: Phase,
    phase_since: Instant,
    restart_after_stop: bool,
    /// A restart the user asked for is under way (stop, then start); shown with the restart glyph
    restarting: bool,
    restart_at: Option<Instant>,
    crashes: Vec<Instant>,
    last_exit: Option<LastEvent>,
    attached: bool,
    log: LogTail,
    chain: Option<ChainInfo>,
    rpc_error: Option<String>,
    /// When the API last answered; during sync it can lag behind for many seconds
    chain_updated: Option<Instant>,
    first_block_time: Option<i64>,
    /// The head block the API last reported, and when it last changed
    head_seen: Option<(u64, Instant)>,
    /// The app's API password for the running node; None when it has no account for the app
    rpc_password: Option<String>,
    peers: Option<Vec<Peer>>,
    /// Every endpoint the node knows of, with its last connection attempt
    potential: Option<Vec<PotentialPeer>>,
    /// Seed "host:port" to the IPv4 endpoints it resolved to
    resolved: HashMap<String, Vec<String>>,
    /// Seeds added while the node runs, still to be passed to it
    pending_seeds: Vec<String>,
    /// Why there is no peer list: an i18n key, or an error text from the node
    peers_note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub phase: Phase,
    /// Tray icon colour: gray, yellow, green or red
    pub color: &'static str,
    /// The glyph the tray and the window show: play, pause, busy, restart or cross (glyphs.rs)
    pub glyph: &'static str,
    pub summary: String,
    pub pid: Option<u32>,
    pub attached: bool,
    pub can_stop_cleanly: bool,
    pub log: LogStage,
    pub chain: Option<ChainInfo>,
    pub rpc_error: Option<String>,
    /// Seconds since the API last answered, once that is over a few seconds; the chain figures are that old
    pub api_stale_seconds: Option<u64>,
    pub lag_seconds: Option<i64>,
    pub sync_percent: Option<f64>,
    pub synced: bool,
    /// Connected P2P peers; None when the list is not available (see peers_note)
    pub peers: Option<Vec<Peer>>,
    /// The built-in seeds, then the ones the user added
    pub seeds: Vec<SeedStatus>,
    pub peers_note: Option<String>,
    /// Seconds without a new block while behind, once that is over NO_BLOCKS_SECS: no peers, most likely
    pub no_blocks_seconds: Option<u64>,
    /// The API answers with another chain ID than the one our node logged: the port is someone else's
    pub chain_id_mismatch: bool,
    pub restart_in_seconds: Option<u64>,
    pub last_exit: Option<String>,
    pub rpc_endpoint: String,
    pub data_dir: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct SeedStatus {
    pub addr: String,
    /// builtin, seed_node (config.ini; the app can add and remove these) or seed_nodes (config.ini list)
    pub source: &'static str,
    pub ips: Vec<String>,
    /// connected, succeeded, failed, rejected, handshake_failed, never, unresolved, unused (replaced by
    /// seed-nodes in config.ini) or unknown (no data from the node)
    pub state: &'static str,
    pub last_attempt: i64,
    pub failures: u32,
    pub error: String,
}

pub struct Supervisor {
    inner: Mutex<Inner>,
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn describe_exit(lang: &str, code: Option<u32>) -> String {
    match code {
        Some(0) => tr(lang, "exit.code0"),
        Some(c) if c >= 0xC000_0000 => trf(lang, "exit.crash", &[("code", format!("0x{c:08X}"))]),
        Some(c) => trf(lang, "exit.code", &[("code", c.to_string())]),
        None => tr(lang, "exit.unknown"),
    }
}

impl Supervisor {
    /// Creates the supervisor, attaches to a node left running by an earlier run, and starts its threads.
    pub fn start_new(settings: Settings) -> Arc<Supervisor> {
        let log = LogTail::new(settings.log_path());
        let sup = Arc::new(Supervisor {
            inner: Mutex::new(Inner {
                settings,
                want_running: false,
                node: None,
                phase: Phase::Stopped,
                phase_since: Instant::now(),
                restart_after_stop: false,
                restarting: false,
                restart_at: None,
                crashes: Vec::new(),
                last_exit: None,
                attached: false,
                log,
                chain: None,
                rpc_error: None,
                chain_updated: None,
                first_block_time: None,
                head_seen: None,
                rpc_password: None,
                peers: None,
                potential: None,
                resolved: HashMap::new(),
                pending_seeds: Vec::new(),
                peers_note: None,
            }),
        });
        sup.lock().attach_existing();
        let s = sup.clone();
        thread::spawn(move || loop {
            s.lock().tick();
            thread::sleep(Duration::from_millis(500));
        });
        let s = sup.clone();
        thread::spawn(move || s.poll_loop());
        sup
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn start(&self) -> Result<(), String> {
        let mut i = self.lock();
        i.crashes.clear(); // a manual start lifts the watchdog's stop
        i.want_running = true;
        i.restarting = false;
        let r = i.start_node();
        if let Err(e) = &r {
            i.want_running = false;
            i.last_exit = Some(LastEvent::Error(e.clone())); // also visible when the app started the node itself
        }
        r
    }

    /// Checked before the window opens: with a conflict the app shows the error and exits.
    pub fn conflict(&self) -> Option<String> {
        self.lock().conflict()
    }

    pub fn stop(&self) -> Result<(), String> {
        let mut i = self.lock();
        i.restarting = false;
        i.restart_after_stop = false;
        i.stop_node()
    }

    pub fn restart(&self) -> Result<(), String> {
        let mut i = self.lock();
        i.restarting = true;
        if i.node.is_none() {
            i.want_running = true;
            return i.start_node();
        }
        i.restart_after_stop = true;
        i.stop_node()
    }

    pub fn kill(&self) -> Result<(), String> {
        let i = self.lock();
        match &i.node {
            Some(n) => n.process.terminate(),
            None => Ok(()),
        }
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// Takes effect at the next start of the node.
    pub fn set_settings(&self, s: Settings) {
        let mut i = self.lock();
        if s.log_path() != i.settings.log_path() {
            i.log = LogTail::new(s.log_path());
        }
        i.settings = s;
    }

    /// Adds "seed-node = addr" to config.ini and, when the node runs, passes the seed on at once.
    pub fn add_seed(&self, addr: &str) -> Result<(), String> {
        let addr = addr.trim().to_string();
        let mut i = self.lock();
        let lang = i.settings.language.clone();
        if !seeds::is_valid(&addr) {
            return Err(trf(&lang, "seeds.invalid", &[("addr", addr)]));
        }
        let data_dir = i.settings.data_dir.clone();
        if seeds::defaults().contains(&addr) || seeds::load_config(&data_dir).seed_node.contains(&addr) {
            return Err(trf(&lang, "seeds.exists", &[("addr", addr)]));
        }
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
        seeds::add_to_config(&data_dir, &addr)?;
        i.pending_seeds.push(addr);
        Ok(())
    }

    /// Removes "seed-node = addr" from config.ini; the node forgets the seed at its next start.
    pub fn remove_seed(&self, addr: &str) -> Result<(), String> {
        let mut i = self.lock();
        i.pending_seeds.retain(|s| s != addr);
        seeds::remove_from_config(&i.settings.data_dir, addr)
    }

    pub fn is_node_running(&self) -> bool {
        self.lock().node.is_some()
    }

    /// Waits until the node has exited; false if `timeout` passed first.
    pub fn wait_stopped(&self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            if !self.is_node_running() {
                return true;
            }
            thread::sleep(Duration::from_millis(200));
        }
        !self.is_node_running()
    }

    pub fn log_lines(&self, cursor: u64, limit: usize) -> Vec<(u64, String)> {
        self.lock().log.lines_after(cursor, limit)
    }

    pub fn log_tail_text(&self, n: usize) -> String {
        self.lock().log.tail_text(n)
    }

    pub fn status(&self) -> Status {
        self.lock().status()
    }

    /// Reads the log and asks the API about the chain, without holding the lock during network calls.
    fn poll_loop(&self) {
        let mut rpc: Option<Rpc> = None;
        // Whether this connection is logged in as the app's API user
        let mut logged_in = false;
        let mut round: u64 = 0;
        loop {
            round += 1;
            // Seeds resolve to IPs, which is how the node lists them; a host name is looked up every 10 minutes
            let wanted: Vec<String> = {
                let i = self.lock();
                let config = seeds::load_config(&i.settings.data_dir);
                let mut all = seeds::defaults();
                all.extend(config.seed_node);
                all.extend(config.seed_nodes.unwrap_or_default());
                all.into_iter().filter(|a| round % 300 == 1 || !i.resolved.contains_key(a)).collect()
            };
            if !wanted.is_empty() {
                let found: Vec<(String, Vec<String>)> = wanted.into_iter().map(|a| { let r = seeds::resolve(&a); (a, r) }).collect();
                self.lock().resolved.extend(found);
            }
            let (endpoint, ask_rpc, first_block_time, lang, password) = {
                let mut i = self.lock();
                i.log.poll();
                // Also while starting: without a file log (no logging.ini) the API is the only sign of readiness.
                // Until the database is open the port refuses connections at once, so asking early is cheap.
                // Not while stopping: the node's websocket server waits for its clients to close their
                // connections, so ours is dropped as soon as the stop begins.
                let up = i.node.is_some() && matches!(i.phase, Phase::Starting | Phase::Running);
                if !up {
                    i.chain = None;
                    i.chain_updated = None;
                    i.head_seen = None;
                    i.peers = None;
                    i.potential = None;
                    i.rpc_error = None;
                }
                (i.settings.rpc_endpoint.clone(), up, i.first_block_time, i.settings.language.clone(), i.rpc_password.clone())
            };
            if !ask_rpc {
                rpc = None;
            } else {
                if rpc.is_none() {
                    rpc = Rpc::connect(&endpoint).ok();
                    logged_in = false;
                }
                let mut login_failed = false;
                if let (Some(r), Some(pw), false) = (rpc.as_mut(), password.as_deref(), logged_in) {
                    match r.login(api_access::USER, pw) {
                        Ok(ok) => {
                            logged_in = ok;
                            login_failed = !ok;
                        }
                        Err(_) => rpc = None,
                    }
                }
                let result = match rpc.as_mut() {
                    Some(r) => rpc::chain_info(r, first_block_time),
                    None => Err(tr(&lang, "rpc.unavailable")),
                };
                // The peer list, on the same connection; a failure here leaves the chain figures alone
                let peers = match (&result, rpc.as_mut()) {
                    (Ok(_), Some(r)) if logged_in => Some(rpc::peers(r)),
                    _ => None,
                };
                // Seeds added since the last round, then every 10 s the node's list of known endpoints
                let mut potential = None;
                if let (Ok(_), Some(r), true) = (&result, rpc.as_mut(), logged_in) {
                    let pending: Vec<String> = std::mem::take(&mut self.lock().pending_seeds);
                    for addr in pending {
                        for ep in seeds::resolve(&addr) {
                            rpc::add_node(r, &ep).ok();
                        }
                    }
                    if round % 5 == 1 {
                        potential = rpc::potential_peers(r).ok();
                    }
                }
                let mut i = self.lock();
                match result {
                    Ok(c) => {
                        i.first_block_time = c.first_block_time;
                        if i.head_seen.map(|(h, _)| h) != Some(c.head_block) {
                            i.head_seen = Some((c.head_block, Instant::now()));
                        }
                        i.chain = Some(c);
                        i.chain_updated = Some(Instant::now());
                        i.rpc_error = None;
                        if potential.is_some() {
                            i.potential = potential;
                        }
                        match peers {
                            Some(Ok(list)) => {
                                i.peers = Some(list);
                                i.peers_note = None;
                            }
                            Some(Err(e)) => {
                                i.peers = None;
                                i.peers_note = Some(e);
                            }
                            None if login_failed => {
                                i.peers = None;
                                i.peers_note = Some("peers.login_failed".into());
                            }
                            None => {}
                        }
                    }
                    Err(e) => {
                        rpc = None;
                        i.rpc_error = Some(e);
                    }
                }
            }
            thread::sleep(Duration::from_secs(2));
        }
    }
}

impl Inner {
    fn lock_path(&self) -> PathBuf {
        self.settings.data_dir.join(LOCK_FILE)
    }

    fn set_phase(&mut self, p: Phase) {
        if matches!(p, Phase::Running | Phase::Failed | Phase::WaitingRestart) {
            self.restarting = false; // the restart is over, one way or the other
        }
        self.phase = p;
        self.phase_since = Instant::now();
    }

    /// Takes over a node an earlier run of the app started, if the lock file names a live witness_node.
    fn attach_existing(&mut self) {
        let path = self.lock_path();
        let Some(lock) = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<LockFile>(&s).ok())
        else {
            return;
        };
        let exe_name = self.settings.node_exe.file_name().map(|n| n.to_ascii_lowercase());
        let ours = Process::open(lock.pid).filter(|p| {
            p.is_alive() && p.image_path().and_then(|i| i.file_name().map(|n| n.to_ascii_lowercase())) == exe_name
        });
        match ours {
            Some(process) => {
                self.node = Some(Node { process, event: Event::open(&lock.event) });
                self.peers_note = lock.rpc_password.is_none().then(|| "peers.restart_needed".to_string());
                self.rpc_password = lock.rpc_password;
                self.want_running = true;
                self.attached = true;
                self.set_phase(Phase::Running);
            }
            None => {
                std::fs::remove_file(&path).ok(); // stale: that node is gone
            }
        }
    }

    /// A node this app did not start (by hand, or by another copy of the app), or another program on the RPC
    /// port: starting ours next to it would break both, as two nodes cannot share a data folder or port.
    /// A node this app runs or found through the lock file is not a conflict.
    fn conflict(&self) -> Option<String> {
        let s = &self.settings;
        let lang = s.language.as_str();
        let ours = self.node.as_ref().map(|n| n.process.pid);
        let exe_name = s.node_exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(pid) = win::find_processes(&exe_name).into_iter().find(|p| Some(*p) != ours) {
            return Some(trf(lang, "err.already_running", &[("exe", exe_name), ("pid", pid.to_string())]));
        }
        if ours.is_none() {
            if let Ok(addr) = s.rpc_endpoint.parse::<std::net::SocketAddr>() {
                if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok() {
                    return Some(trf(lang, "err.port_busy", &[("endpoint", s.rpc_endpoint.clone())]));
                }
            }
        }
        None
    }

    fn start_node(&mut self) -> Result<(), String> {
        if self.node.is_some() {
            return Ok(());
        }
        let s = self.settings.clone();
        let lang = s.language.as_str();
        if !s.node_exe.is_file() {
            self.want_running = false;
            return Err(trf(lang, "err.not_found", &[("path", s.node_exe.display().to_string())]));
        }
        if let Some(e) = self.conflict() {
            self.want_running = false;
            return Err(e);
        }
        std::fs::create_dir_all(&s.data_dir).map_err(|e| {
            trf(lang, "err.mkdir", &[("path", s.data_dir.display().to_string()), ("e", e.to_string())])
        })?;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let event_name = format!("Local\\graphene-node-{}-{}-{}", std::process::id(), now_unix(), nanos);
        let event = Event::create(&event_name)?;
        let mut args = vec![
            "--data-dir".into(),
            s.data_dir.display().to_string(),
            "--rpc-endpoint".into(),
            s.rpc_endpoint.clone(),
            "--shutdown-event".into(),
            event_name.clone(),
        ];
        // The node reads the added seeds from config.ini itself
        self.pending_seeds.clear();
        // An API account for the peer list, unless config.ini manages API access itself
        self.peers = None;
        self.rpc_password = None;
        if api_access::config_sets_api_access(&s.data_dir) {
            self.peers_note = Some("peers.config_owns".into());
        } else {
            match api_access::write(&s.data_dir) {
                Ok((path, password)) => {
                    args.push("--api-access".into());
                    args.push(path.display().to_string());
                    self.rpc_password = Some(password);
                    self.peers_note = None;
                }
                Err(e) => self.peers_note = Some(e),
            }
        }
        let pid = win::spawn(&s.node_exe, &args)?;
        let lock = LockFile {
            pid,
            event: event_name,
            rpc_endpoint: s.rpc_endpoint.clone(),
            rpc_password: self.rpc_password.clone(),
        };
        std::fs::write(self.lock_path(), serde_json::to_string(&lock).unwrap_or_default()).ok();
        self.log.reset_stage();
        self.attached = false;
        self.restart_at = None;
        self.first_block_time = None;
        match Process::open(pid) {
            Some(process) => {
                self.node = Some(Node { process, event: Some(event) });
                self.set_phase(Phase::Starting);
            }
            None => {
                // exited before we could watch it
                self.on_exit(None);
            }
        }
        Ok(())
    }

    fn stop_node(&mut self) -> Result<(), String> {
        self.want_running = false;
        self.restart_at = None;
        let Some(node) = &self.node else {
            if matches!(self.phase, Phase::WaitingRestart | Phase::Failed) {
                self.set_phase(Phase::Stopped);
            }
            return Ok(());
        };
        let result = match &node.event {
            Some(ev) => ev.set(),
            None => Err(tr(&self.settings.language, "err.no_event")),
        };
        self.log.stage.shutdown_step = None;
        match result {
            Ok(()) => self.set_phase(Phase::Stopping),
            Err(_) => self.set_phase(Phase::StopTimedOut),
        }
        result
    }

    fn on_exit(&mut self, code: Option<u32>) {
        self.node = None;
        std::fs::remove_file(self.lock_path()).ok();
        let expected = matches!(self.phase, Phase::Stopping | Phase::StopTimedOut) || !self.want_running;
        if expected {
            self.last_exit = Some(LastEvent::Exited(code));
            self.set_phase(Phase::Stopped);
            if std::mem::take(&mut self.restart_after_stop) {
                self.want_running = true;
                if let Err(e) = self.start_node() {
                    self.last_exit = Some(LastEvent::Error(e));
                }
            }
            return;
        }
        if code == Some(EXIT_DATA_DIR_IN_USE) {
            // Not a crash: restarting cannot help while the other node runs
            self.want_running = false;
            self.last_exit = Some(LastEvent::DataDirInUse);
            self.set_phase(Phase::Failed);
            return;
        }
        let now = Instant::now();
        self.crashes.retain(|t| now.duration_since(*t) < CRASH_WINDOW);
        self.crashes.push(now);
        if self.crashes.len() >= MAX_CRASHES {
            self.want_running = false;
            self.last_exit = Some(LastEvent::GaveUp(code));
            self.set_phase(Phase::Failed);
        } else {
            let delay = RESTART_DELAYS[self.crashes.len() - 1];
            self.last_exit = Some(LastEvent::Exited(code));
            self.restart_at = Some(now + Duration::from_secs(delay));
            self.set_phase(Phase::WaitingRestart);
        }
    }

    fn tick(&mut self) {
        if let Some(n) = &self.node {
            if !n.process.is_alive() {
                let code = n.process.exit_code();
                self.on_exit(code);
            }
        }
        match self.phase {
            Phase::Starting if self.log.stage.stage == "started" || self.chain.is_some() => self.set_phase(Phase::Running),
            // The node logged that it closed everything and is exiting, yet the process lingers: ending it
            // loses nothing, the database is closed already
            Phase::Stopping | Phase::StopTimedOut
                if self.log.stage.shutdown_step.as_deref() == Some(SHUTDOWN_DONE)
                    && self.phase_since.elapsed() > EXIT_GRACE =>
            {
                if let Some(n) = &self.node {
                    n.process.terminate().ok();
                }
            }
            Phase::Stopping if self.phase_since.elapsed() > STOP_TIMEOUT => self.set_phase(Phase::StopTimedOut),
            Phase::WaitingRestart if self.restart_at.is_some_and(|t| Instant::now() >= t) => {
                if let Err(e) = self.start_node() {
                    self.last_exit = Some(LastEvent::Error(e));
                    self.set_phase(Phase::Failed);
                }
            }
            _ => {}
        }
    }

    fn seed_status(&self) -> Vec<SeedStatus> {
        let config = seeds::load_config(&self.settings.data_dir);
        // seed-nodes in config.ini replaces the built-in list; seed-node entries come on top of either
        let replaced = config.seed_nodes.is_some();
        let builtin = seeds::defaults().into_iter().map(|a| (a, "builtin"));
        let listed = config.seed_nodes.unwrap_or_default().into_iter().map(|a| (a, "seed_nodes"));
        let added = config.seed_node.into_iter().map(|a| (a, "seed_node"));
        builtin
            .chain(listed)
            .chain(added)
            .map(|(addr, source)| {
                let ips = self.resolved.get(&addr).cloned();
                let mut st = SeedStatus {
                    addr,
                    source,
                    ips: ips.clone().unwrap_or_default(),
                    state: "unknown",
                    last_attempt: 0,
                    failures: 0,
                    error: String::new(),
                };
                if replaced && source == "builtin" {
                    st.state = "unused";
                    return st;
                }
                let ips = match ips {
                    Some(ips) if ips.is_empty() => {
                        st.state = "unresolved";
                        return st;
                    }
                    Some(ips) => ips,
                    None => return st,
                };
                if self.peers.as_ref().is_some_and(|p| p.iter().any(|p| ips.contains(&p.addr))) {
                    st.state = "connected";
                }
                // The most recent attempt over the seed's addresses
                let record = self
                    .potential
                    .as_ref()
                    .and_then(|list| list.iter().filter(|p| ips.contains(&p.addr)).max_by_key(|p| p.last_attempt));
                if let Some(p) = record {
                    st.last_attempt = p.last_attempt;
                    st.failures = p.failures;
                    // The error of an earlier attempt means nothing once a connection worked
                    st.error = p.error.clone();
                    if st.state != "connected" {
                        st.state = match p.disposition.as_str() {
                            "last_connection_succeeded" => "succeeded",
                            "last_connection_failed" => "failed",
                            "last_connection_rejected" => "rejected",
                            "last_connection_handshaking_failed" => "handshake_failed",
                            _ => "never",
                        };
                    }
                } else if self.potential.is_some() && st.state != "connected" {
                    st.state = "never";
                }
                if !matches!(st.state, "failed" | "rejected" | "handshake_failed") {
                    st.error.clear();
                }
                st
            })
            .collect()
    }

    fn status(&self) -> Status {
        let now = now_unix();
        let lag = self.chain.as_ref().map(|c| now - c.head_time);
        let synced = lag.is_some_and(|l| l <= SYNCED_LAG_SECS);
        let sync_percent = self.chain.as_ref().map(|c| rpc::sync_percent(c.first_block_time, c.head_time, now));
        let no_blocks = self
            .head_seen
            .map(|(_, t)| t.elapsed().as_secs())
            .filter(|s| *s >= NO_BLOCKS_SECS && !synced && self.phase == Phase::Running);
        let mismatch = match (&self.chain, &self.log.stage.chain_id) {
            (Some(c), Some(id)) => !c.chain_id.is_empty() && &c.chain_id != id,
            _ => false,
        };
        let restart_in = self.restart_at.map(|t| t.saturating_duration_since(Instant::now()).as_secs());
        let lang = self.settings.language.as_str();
        let block = |n: u64| ("block", n.to_string());
        let summary = match self.phase {
            Phase::Stopped => tr(lang, "status.stopped"),
            Phase::Starting => match (self.log.stage.stage.as_str(), self.log.stage.replay_percent) {
                ("replaying", Some(p)) => trf(lang, "status.replay_pct", &[("pct", format!("{p:.0}"))]),
                ("replaying", None) => tr(lang, "status.replay"),
                ("opening", _) => tr(lang, "status.opening"),
                _ => tr(lang, "status.starting"),
            },
            Phase::Running => match (&self.chain, synced) {
                _ if mismatch => trf(lang, "status.mismatch", &[("endpoint", self.settings.rpc_endpoint.clone())]),
                (Some(c), true) => trf(lang, "status.synced", &[block(c.head_block)]),
                (Some(c), false) if no_blocks.is_some() => {
                    let time = ("time", i18n::duration(lang, no_blocks.unwrap_or(0) as i64));
                    match self.peers.as_ref().map(Vec::len) {
                        // Connected, but nobody sends blocks: the seeds are reachable, the cause is elsewhere
                        Some(n) if n > 0 => {
                            trf(lang, "status.no_blocks_peers", &[block(c.head_block), time, ("n", n.to_string())])
                        }
                        _ => trf(lang, "status.no_blocks", &[block(c.head_block), time]),
                    }
                }
                (Some(c), false) => trf(
                    lang,
                    "status.syncing",
                    &[
                        ("pct", format!("{:.1}", sync_percent.unwrap_or(0.0))),
                        block(c.head_block),
                        ("lag", i18n::duration(lang, lag.unwrap_or(0))),
                    ],
                ),
                (None, _) => tr(lang, "status.waiting_api"),
            },
            Phase::Stopping => match &self.log.stage.shutdown_step {
                Some(step) => format!("{} · {step}", tr(lang, "status.stopping")),
                None => tr(lang, "status.stopping"),
            },
            Phase::StopTimedOut => tr(lang, "status.stop_timeout"),
            Phase::WaitingRestart => trf(lang, "status.restart_in", &[("s", restart_in.unwrap_or(0).to_string())]),
            Phase::Failed => match self.last_exit {
                Some(LastEvent::DataDirInUse) => tr(lang, "status.data_dir_in_use"),
                _ => tr(lang, "status.failed"),
            },
        };
        let color = match self.phase {
            Phase::Stopped => "gray",
            Phase::Failed | Phase::StopTimedOut => "red",
            Phase::Running if mismatch => "red",
            Phase::Running if synced => "green",
            _ => "yellow",
        };
        let glyph = match (self.restarting, color) {
            (true, _) if matches!(self.phase, Phase::Stopping | Phase::Stopped | Phase::Starting) => "restart",
            (_, "green") => "play",
            (_, "yellow") => "busy",
            (_, "red") => "cross",
            _ => "pause",
        };
        Status {
            phase: self.phase,
            color,
            glyph,
            summary,
            pid: self.node.as_ref().map(|n| n.process.pid),
            attached: self.attached,
            can_stop_cleanly: self.node.as_ref().is_some_and(|n| n.event.is_some()),
            log: self.log.stage.clone(),
            chain: self.chain.clone(),
            rpc_error: self.rpc_error.clone(),
            api_stale_seconds: self
                .chain_updated
                .map(|t| t.elapsed().as_secs())
                // While stopping, the node closes its API first, so silence there is expected
                .filter(|s| *s >= 6 && self.chain.is_some() && self.phase == Phase::Running),
            lag_seconds: lag,
            sync_percent,
            synced,
            no_blocks_seconds: no_blocks,
            peers: self.peers.clone(),
            seeds: self.seed_status(),
            // An i18n key is translated; an error text from the node is shown as it is
            peers_note: self.peers_note.as_ref().map(|n| if n.starts_with("peers.") { tr(lang, n) } else { n.clone() }),
            chain_id_mismatch: mismatch,
            restart_in_seconds: restart_in,
            last_exit: self.last_exit.as_ref().map(|e| e.render(lang)),
            rpc_endpoint: self.settings.rpc_endpoint.clone(),
            data_dir: self.settings.data_dir.clone(),
        }
    }
}
