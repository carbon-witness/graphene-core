# Graphene Node (GUI)

Tray app and dashboard for `witness_node.exe` on Windows (Tauri 2). Plan and decisions: the
"Graphene: GUI-нода под Windows" document.

The node runs as its own process: closing or crashing the app leaves it running, and the next start of the
app finds it again through `graphene-node-gui.lock` in the data folder. The app stops the node through the
named event it passes as `--shutdown-event`.

## Build (Linux host, cross to Windows)

    rustup target add x86_64-pc-windows-gnu
    apt-get install g++-mingw-w64-x86-64-posix wine64   # wine only for the tests
    cargo build --release --target x86_64-pc-windows-gnu

Output in `target/x86_64-pc-windows-gnu/release/`: `graphene-node.exe` (the app) and `supervisor-cli.exe`.
Ship `graphene-node.exe` with `WebView2Loader.dll` (from the `webview2-com-sys` crate, `x64/`) and
`witness_node.exe` in one folder; by default the app looks for the node and `witness_node_data_dir` next to itself.

## Tests

    cargo test --release --target x86_64-pc-windows-gnu --lib      # runs under Wine (.cargo/config.toml)

`supervisor-cli.exe <node.exe> <data dir> <rpc endpoint> start|stop|watch N` drives the supervisor without
the window (Wine has no WebView2): start, re-attach through the lock file, watchdog restarts, clean stop.

## Layout

| Path | Role |
| --- | --- |
| `src/supervisor.rs` | Node process, lock file, stop via event, watchdog (15 s / 1 min / 5 min, stops after 3 crashes in 10 min), status |
| `src/logtail.rs` | Follows `logs/default/default.log`: stages (opening, replay %, started), chain ID, WIF masking |
| `src/rpc.rs` | WebSocket client for the anonymous database API: head, irreversible block, chain ID |
| `src/main.rs` | Tauri: tray icon and menu, window commands, hide-to-tray, "Выход" stops the node |
| `dist/` | The window: dashboard, journal, settings (plain HTML/JS, no build step) |
