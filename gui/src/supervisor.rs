//! Runs witness_node.exe and keeps it running.
//!
//! The node is a separate process that outlives this app: a crash of the app must not take the node with it,
//! so it is not put in a kill-on-close job and gets no --parent-pid. The app finds a running node again through
//! a lock file in the data folder and stops it through the named event the node was started with.

use crate::logtail::{LogStage, LogTail};
use crate::rpc::{self, ChainInfo, Rpc};
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
const LOCK_FILE: &str = "graphene-node-gui.lock";

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
    restart_at: Option<Instant>,
    crashes: Vec<Instant>,
    last_exit: Option<String>,
    attached: bool,
    log: LogTail,
    chain: Option<ChainInfo>,
    rpc_error: Option<String>,
    first_block_time: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub phase: Phase,
    /// Tray icon colour: gray, yellow, green or red
    pub color: &'static str,
    pub summary: String,
    pub pid: Option<u32>,
    pub attached: bool,
    pub can_stop_cleanly: bool,
    pub log: LogStage,
    pub chain: Option<ChainInfo>,
    pub rpc_error: Option<String>,
    pub lag_seconds: Option<i64>,
    pub sync_percent: Option<f64>,
    pub synced: bool,
    /// The API answers with another chain ID than the one our node logged: the port is someone else's
    pub chain_id_mismatch: bool,
    pub restart_in_seconds: Option<u64>,
    pub last_exit: Option<String>,
    pub rpc_endpoint: String,
    pub data_dir: PathBuf,
}

pub struct Supervisor {
    inner: Mutex<Inner>,
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn describe_exit(code: Option<u32>) -> String {
    match code {
        Some(0) => "завершилась с кодом 0".into(),
        Some(c) if c >= 0xC000_0000 => format!("аварийно завершилась (0x{c:08X})"),
        Some(c) => format!("завершилась с кодом {c}"),
        None => "завершилась".into(),
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
                restart_at: None,
                crashes: Vec::new(),
                last_exit: None,
                attached: false,
                log,
                chain: None,
                rpc_error: None,
                first_block_time: None,
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
        i.start_node()
    }

    pub fn stop(&self) -> Result<(), String> {
        self.lock().stop_node()
    }

    pub fn restart(&self) -> Result<(), String> {
        let mut i = self.lock();
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
        loop {
            let (endpoint, ask_rpc, first_block_time) = {
                let mut i = self.lock();
                i.log.poll();
                let up = i.node.is_some() && matches!(i.phase, Phase::Running | Phase::Stopping);
                if !up {
                    i.chain = None;
                    i.rpc_error = None;
                }
                (i.settings.rpc_endpoint.clone(), up, i.first_block_time)
            };
            if !ask_rpc {
                rpc = None;
            } else {
                if rpc.is_none() {
                    rpc = Rpc::connect(&endpoint).ok();
                }
                let result = match rpc.as_mut() {
                    Some(r) => rpc::chain_info(r, first_block_time),
                    None => Err("RPC недоступен".into()),
                };
                let mut i = self.lock();
                match result {
                    Ok(c) => {
                        i.first_block_time = Some(c.first_block_time);
                        i.chain = Some(c);
                        i.rpc_error = None;
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
                self.want_running = true;
                self.attached = true;
                self.set_phase(Phase::Running);
            }
            None => {
                std::fs::remove_file(&path).ok(); // stale: that node is gone
            }
        }
    }

    fn start_node(&mut self) -> Result<(), String> {
        if self.node.is_some() {
            return Ok(());
        }
        let s = self.settings.clone();
        if !s.node_exe.is_file() {
            self.want_running = false;
            return Err(format!("Не найден {}", s.node_exe.display()));
        }
        std::fs::create_dir_all(&s.data_dir).map_err(|e| format!("Не создать {}: {e}", s.data_dir.display()))?;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let event_name = format!("Local\\graphene-node-{}-{}-{}", std::process::id(), now_unix(), nanos);
        let event = Event::create(&event_name)?;
        let args = vec![
            "--data-dir".into(),
            s.data_dir.display().to_string(),
            "--rpc-endpoint".into(),
            s.rpc_endpoint.clone(),
            "--shutdown-event".into(),
            event_name.clone(),
        ];
        let pid = win::spawn(&s.node_exe, &args)?;
        let lock = LockFile { pid, event: event_name, rpc_endpoint: s.rpc_endpoint.clone() };
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
            None => Err("Нода запущена без события остановки: остановить её можно только принудительно".into()),
        };
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
            self.last_exit = Some(format!("Нода {}", describe_exit(code)));
            self.set_phase(Phase::Stopped);
            if std::mem::take(&mut self.restart_after_stop) {
                self.want_running = true;
                if let Err(e) = self.start_node() {
                    self.last_exit = Some(e);
                }
            }
            return;
        }
        let now = Instant::now();
        self.crashes.retain(|t| now.duration_since(*t) < CRASH_WINDOW);
        self.crashes.push(now);
        let what = describe_exit(code);
        if self.crashes.len() >= MAX_CRASHES {
            self.want_running = false;
            self.last_exit = Some(format!("Нода {what}; {MAX_CRASHES} падения за 10 минут, перезапуски остановлены"));
            self.set_phase(Phase::Failed);
        } else {
            let delay = RESTART_DELAYS[self.crashes.len() - 1];
            self.last_exit = Some(format!("Нода {what}"));
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
            Phase::Starting if self.log.stage.stage == "started" => self.set_phase(Phase::Running),
            Phase::Stopping if self.phase_since.elapsed() > STOP_TIMEOUT => self.set_phase(Phase::StopTimedOut),
            Phase::WaitingRestart if self.restart_at.is_some_and(|t| Instant::now() >= t) => {
                if let Err(e) = self.start_node() {
                    self.last_exit = Some(e);
                    self.set_phase(Phase::Failed);
                }
            }
            _ => {}
        }
    }

    fn status(&self) -> Status {
        let now = now_unix();
        let lag = self.chain.as_ref().map(|c| now - c.head_time);
        let synced = lag.is_some_and(|l| l <= SYNCED_LAG_SECS);
        let sync_percent = self.chain.as_ref().map(|c| rpc::sync_percent(c.first_block_time, c.head_time, now));
        let mismatch = match (&self.chain, &self.log.stage.chain_id) {
            (Some(c), Some(id)) => !c.chain_id.is_empty() && &c.chain_id != id,
            _ => false,
        };
        let restart_in = self.restart_at.map(|t| t.saturating_duration_since(Instant::now()).as_secs());
        let summary = match self.phase {
            Phase::Stopped => "Нода остановлена".to_string(),
            Phase::Starting => match (self.log.stage.stage.as_str(), self.log.stage.replay_percent) {
                ("replaying", Some(p)) => format!("Replay базы: {p:.0}%"),
                ("replaying", None) => "Replay базы…".into(),
                ("opening", _) => "Открываю базу данных…".into(),
                _ => "Запуск ноды…".into(),
            },
            Phase::Running => match (&self.chain, synced) {
                _ if mismatch => format!("На порту {} чужая нода (другой chain ID)", self.settings.rpc_endpoint),
                (Some(c), true) => format!("Синхронизирована · блок {}", c.head_block),
                (Some(c), false) => format!(
                    "Синхронизация {:.1}% · блок {} · отстаёт на {}",
                    sync_percent.unwrap_or(0.0),
                    c.head_block,
                    human_duration(lag.unwrap_or(0))
                ),
                (None, _) => "Нода запущена, жду API…".into(),
            },
            Phase::Stopping => "Останавливаю ноду…".into(),
            Phase::StopTimedOut => "Нода не остановилась за 60 с".into(),
            Phase::WaitingRestart => format!("Нода упала, перезапуск через {} с", restart_in.unwrap_or(0)),
            Phase::Failed => "Нода падает, перезапуски остановлены".into(),
        };
        let color = match self.phase {
            Phase::Stopped => "gray",
            Phase::Failed | Phase::StopTimedOut => "red",
            Phase::Running if mismatch => "red",
            Phase::Running if synced => "green",
            _ => "yellow",
        };
        Status {
            phase: self.phase,
            color,
            summary,
            pid: self.node.as_ref().map(|n| n.process.pid),
            attached: self.attached,
            can_stop_cleanly: self.node.as_ref().is_some_and(|n| n.event.is_some()),
            log: self.log.stage.clone(),
            chain: self.chain.clone(),
            rpc_error: self.rpc_error.clone(),
            lag_seconds: lag,
            sync_percent,
            synced,
            chain_id_mismatch: mismatch,
            restart_in_seconds: restart_in,
            last_exit: self.last_exit.clone(),
            rpc_endpoint: self.settings.rpc_endpoint.clone(),
            data_dir: self.settings.data_dir.clone(),
        }
    }
}

pub fn human_duration(secs: i64) -> String {
    let s = secs.max(0);
    match s {
        0..=119 => format!("{s} с"),
        120..=7199 => format!("{} мин", s / 60),
        7200..=172_799 => format!("{} ч", s / 3600),
        _ => format!("{} дн", s / 86400),
    }
}
