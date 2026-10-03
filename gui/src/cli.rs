//! Drives the supervisor without a window, for tests:
//!   supervisor-cli <node.exe> <data dir> <rpc endpoint> start   start the node, print status until it runs, leave it
//!   supervisor-cli <node.exe> <data dir> <rpc endpoint> stop    attach through the lock file and stop it cleanly
//!   supervisor-cli <node.exe> <data dir> <rpc endpoint> watch N print status every second for N seconds

use node_gui::settings::Settings;
use node_gui::supervisor::{Phase, Supervisor, STOP_TIMEOUT};
use std::time::{Duration, Instant};

fn show(sup: &Supervisor) -> node_gui::supervisor::Status {
    let s = sup.status();
    println!(
        "{:?} {} {} pid={:?} attached={} clean_stop={} head={:?} stale={:?} | {}{}",
        s.phase,
        s.color,
        s.glyph,
        s.pid,
        s.attached,
        s.can_stop_cleanly,
        s.chain.as_ref().map(|c| c.head_block),
        s.api_stale_seconds,
        s.summary,
        s.last_exit.as_deref().map(|e| format!(" | last: {e}")).unwrap_or_default()
    );
    s
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage: supervisor-cli <node.exe> <data dir> <rpc endpoint> start|stop|watch N");
        std::process::exit(2);
    }
    let settings = Settings {
        node_exe: a[1].clone().into(),
        data_dir: a[2].clone().into(),
        rpc_endpoint: a[3].clone(),
        start_node_with_app: false,
        ..Settings::default()
    };
    let sup = Supervisor::start_new(settings);
    std::thread::sleep(Duration::from_millis(700));
    match a[4].as_str() {
        "start" => {
            if let Err(e) = sup.start() {
                println!("start failed: {e}");
                std::process::exit(1);
            }
            let end = Instant::now() + Duration::from_secs(120);
            while Instant::now() < end {
                let s = show(&sup);
                if s.phase == Phase::Running && s.chain.is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            std::process::exit(1);
        }
        "stop" => {
            let s = show(&sup);
            if s.pid.is_none() {
                println!("no node to stop");
                std::process::exit(1);
            }
            let t = Instant::now();
            if let Err(e) = sup.stop() {
                println!("stop failed: {e}");
            }
            let ok = sup.wait_stopped(STOP_TIMEOUT + Duration::from_secs(5));
            show(&sup);
            println!("stopped={ok} in {} ms", t.elapsed().as_millis());
            std::process::exit(if ok { 0 } else { 1 });
        }
        "restart" => {
            sup.restart().ok();
            for _ in 0..40 {
                show(&sup);
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        "watch" => {
            let n: u64 = a.get(5).and_then(|x| x.parse().ok()).unwrap_or(10);
            for _ in 0..n {
                show(&sup);
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        other => {
            eprintln!("unknown command {other}");
            std::process::exit(2);
        }
    }
}
