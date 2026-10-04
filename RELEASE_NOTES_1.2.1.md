# graphene-core 1.2.1

**Date:** October 4, 2026
**Branch:** `feature/windows-witness-node` ([carbon-witness/graphene-core](https://github.com/carbon-witness/graphene-core/tree/feature/windows-witness-node))
**Tag:** `graphene-1.2.1`
**Previous version:** 1.2.0 — tag `graphene-1.2.0` (September 27, 2026), [release notes](RELEASE_NOTES_1.2.0.md)

## Summary

graphene-core 1.2.1 brings the node to Windows:

- `witness_node.exe`, a single static executable for 64-bit Windows 10 and later, cross-built on Linux with
  MinGW-w64;
- **Graphene Node** (`graphene-node-gui.exe`), a tray app and dashboard that starts, watches and stops the node.

The node now stops cleanly in every way a Windows user ends it: Ctrl+C, closing the console window, the GUI's stop
button, quitting the GUI, and a Windows shutdown, restart or logoff. Each of these used to leave the database dirty and
cost a replay at the next start.

Fixes that apply on every platform:

- a crash on shutdown while the node was syncing;
- a hang in the websocket server's shutdown;
- private keys printed into the log by a malformed `private-key` entry;
- no guard against two nodes on one data directory.

The file log now carries the level of each line.

Consensus rules, block format and serialization are unchanged.

## Getting the node

### Windows

Download `graphene-node-win64-1.2.1.zip` from the GitHub release, unpack it and run `graphene-node-gui.exe`. The zip
holds three files that must stay in one folder: `witness_node.exe`, `graphene-node-gui.exe` and `WebView2Loader.dll`.
By default the app uses `witness_node.exe` and `witness_node_data_dir` next to itself. The
WebView2 runtime ships with Windows 10 and 11.

`witness_node.exe` also runs on its own, from `cmd` or PowerShell, like the Linux node.

### Linux and Docker

The node is unchanged from 1.2.0 apart from the fixes below; see [README.md](README.md#getting-started) and
[README-docker.md](README-docker.md).

```
docker pull carbonwitness/graphene-core:1.2.1
docker pull ghcr.io/carbon-witness/graphene-core:1.2.1
```

## Windows build

`contrib/win64/` cross-builds `witness_node.exe` on a Linux host; see [contrib/win64/README.md](contrib/win64/README.md).

```
apt-get install g++-mingw-w64-x86-64-posix cmake make perl git wine64
contrib/win64/build-deps.sh ~/win64-deps          # Boost 1.90, OpenSSL 3.5, zlib, curl; about 30 min, once
contrib/win64/build.sh ~/win64-deps build-win64   # -> build-win64/programs/witness_node/witness_node.exe
```

- **Compiler:** the `-posix` MinGW-w64 compilers (`std::thread`).
- **Linking:** static, with `-mbig-obj`. The executable needs only DLLs that ship with Windows.
- **Dependencies:** `build-deps.sh` clones them at fixed tags and installs them as static libraries.
- **Build helpers:** the build runs its own helpers (`cat-parts`, `embed_genesis`). Wine runs them as the CMake
  cross-compiling emulator through `wine-run.sh`, which rewrites `/abs/path` arguments to `Z:/abs/path`.
- **fc fixes needed by MinGW** (graphene-fc `ac00004`):
  - a missing Boost.PP include in `reflect.hpp`;
  - the reserved size of the Win64 `tcp_socket` implementation (208 bytes with Boost 1.90, 168 reserved);
  - `_WIN32_WINNT` raised to Windows 10, for `WaitOnAddress` in Boost.Atomic.

The GUI is built with Cargo for the `x86_64-pc-windows-gnu` target; see [gui/README.md](gui/README.md).

### CI

The new workflow `.github/workflows/windows.yml` cross-builds the Windows package on `ubuntu-24.04` on every push and
pull request:

- **Build:** `witness_node.exe` with `contrib/win64/`, then `graphene-node-gui.exe` with Cargo; the GUI's tests run
  under Wine.
- **Smoke test:** `witness_node.exe --version` runs under Wine.
- **Package:** `graphene-node-win64-<version>.zip` with `witness_node.exe`, `graphene-node-gui.exe` and
  `WebView2Loader.dll`. Every run keeps it as an artifact; a tag `graphene-X.Y.Z` attaches it to the release.
- **Caches:** the dependencies are cached by the hash of `build-deps.sh`, so they are built only once; the node is
  built with ccache.

```
rustup target add x86_64-pc-windows-gnu
cd gui && cargo build --release --target x86_64-pc-windows-gnu
```

## witness_node on Windows

### Clean stop from a GUI: `--shutdown-event`, `--parent-pid`
A GUI that runs the node without a console cannot send it Ctrl+C, and `TerminateProcess` leaves the database dirty.
Two Windows-only options cover this:

- `--shutdown-event <name>` makes the node wait on a named event that the GUI creates and signals;
- `--parent-pid <pid>` makes the node exit when that process exits, so it does not outlive a crashed GUI.

Both take the same exit path as SIGINT. The event and the process are opened before startup, so a wrong name or
PID fails at once instead of leaving a node nobody can stop.
Commit: graphene-core `265d67ca`.

### Closing the console window
Closing the console window killed the node after the system's grace period, and the next start replayed blocks.
`CTRL_CLOSE_EVENT` and `CTRL_BREAK_EVENT` now take the clean exit path. The handler blocks until the node has shut
down, because Windows ends the process as soon as the handler returns.
Commit: graphene-core `30f5cab6`.

### The process ends right after "Shutdown: done"
On a real Windows machine, the node logged `Shutdown: done, exiting the process` with the database closed, but the
process stayed. It hung in the runtime's static destructors and thread joins, on network threads with live peers.
Everything that needs a clean close is closed at that point, so on Windows the node now flushes its output and calls
`_exit` there.
Commit: graphene-core `e24ffd7`.

### Ended last at a Windows shutdown
The node now asks Windows to end it last: `SetProcessShutdownParameters(0x100)`, the lowest level open to
applications. The GUI asks to be notified first and has time to stop the node (see "Windows shutdown, restart and
logoff" under the GUI below).
Commit: graphene-core `d9f066a`.

### Startup errors stay readable after a double-click
Started from Explorer, the node gets a console of its own, which closes with the process. A startup error (data
directory in use, a bad command line, a plugin conflict) flashed by unread. On those exits the node now waits for
Enter, but only when the console belongs to it alone:

- not when started from `cmd` or PowerShell, where the console stays open anyway;
- not when started from the GUI, which shows no console.

Commit: graphene-core `5da5c17`.

## Graphene Node (GUI)

A Tauri 2 app, `gui/` in this repository, version 0.1.0.

### The node as its own process
- **Separate from the app.** Closing the app, or the app crashing, leaves the node running. The next start of the app
  finds the node through `graphene-node-gui.lock` in the data folder.
- **Stopping.** The app stops the node through the named event it passed as `--shutdown-event`. If the node has logged
  `Shutdown: done` but its process is still there 10 s later, the app ends it; the database is already closed by then.
- **Watchdog.** A crashed node is restarted after 15 s, then 1 min, then 5 min. After 3 crashes within 10 minutes the
  app stops trying.
- **Data folder in use.** A node refused with exit code 3 (see "One node per data directory" below) is not counted as
  a crash. The status turns red at once with "Data folder in use by another node".
- **Second node.** A second node on the same data folder is detected before the window opens. The app then shows an
  error box with OK and exits.

### Window
- **Dashboard:** status, head and irreversible block, sync progress, blocks per minute and chain ID. The data comes
  from the node's anonymous `database` API over WebSocket and from its log.
- **Journal:** the node's log, coloured by level as in a Linux console, with a filter by level and a "Blocks" view.
  The journal and the blocks table keep the newest 5000 rows.
- **Seeds and Peers:**
  - *Default seeds:* the node's built-in seeds (the same `seed-nodes.txt` the node compiles in) and the
    `seed-node` entries of `config.ini`, with their IP addresses and the node's last attempt to reach each one:
    connected, failed, rejected, handshake failed or not tried yet, with the error. **Add seed** writes
    `seed-node = host:port` into `config.ini` in the data folder, where the node reads it at every start, and
    passes it to a running node at once (`network_node.add_node`); added seeds can be removed again. A
    `seed-nodes` list in `config.ini` replaces the built-in seeds, which are then shown as not used.
  - *Peers:* the connected P2P peers with address, direction, release, build, platform, the peer's block,
    connection time and traffic.

  The data comes from the node's `network_node` API, which anonymous clients cannot use: unless `config.ini`
  sets `api-access`, the app writes `graphene-node-gui-api.json` into the data folder with the node's default
  anonymous access unchanged plus an account for the app (a random password per node start, kept in
  `graphene-node-gui.lock`), and starts the node with `--api-access`.
- **Peer release and build.** From 1.2.1 on, the node's P2P user agent carries its build string (see below).
  Older releases send no version; the app recognises 1.0, 1.1 and 1.2.0 by the commit time of the fc library
  they report and shows the commit of their release tag as the build; other builds get a "?".
- **No blocks from the network.** Behind and without a new block for 90 s, the status says so. With no peers it
  points at unreachable seed nodes and a firewall; with peers it shows how many are connected.
- **Actions:** start, stop and restart, as buttons of one width with player glyphs. While stopping, the status shows
  the node's current shutdown step.
- **Settings:**
  - paths to the node and the data folder; an empty path means next to the app, so the folder can be moved;
  - RPC endpoint;
  - language: English or Russian;
  - "Start with Windows".
- **Closing the window** asks: stop the node and quit, or keep running in the tray.

### Tray
- **Icon:** a white circle with a glyph for the node's state:

  | State | Glyph |
  |---|---|
  | Running | Green play |
  | Stopped | Pause |
  | Starting or syncing | Yellow dot |
  | Restarting | Blue-cyan arrow |
  | Failed | Red cross |

  The window shows the same glyph.
- **Menu:** start, stop and restart with the same glyphs, plus "Quit (stop the node)".
- The icon is removed when the app quits.
- The tooltip and the menu are updated only when they change.

### Start with Windows
The app is registered in the per-user `Run` key (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value
`Graphene Node`), so no administrator rights are needed. Started this way, it opens in the tray, without its window.

### Windows shutdown, restart and logoff
Windows ended the node without notice: as a console program that loads `user32.dll`, it gets no console event at
shutdown. The next start replayed the database from block 1.

The app now has a hidden top-level window that receives `WM_QUERYENDSESSION`:

1. The app blocks the shutdown with a reason ("Stopping the Graphene node so that its database stays intact…"), stops the node and only then lets the
   shutdown continue, after 20 s at most.
2. If the shutdown is cancelled, the node is started again.
3. The app asks Windows to notify it first (`SetProcessShutdownParameters(0x3FF)`), and the node asks to be ended
   last.

Each step is written to `%APPDATA%\org.graphene.node-gui\gui.log`.

Verified on Windows 10 with two restarts: the node stopped in 2.4 s and 1.1 s, with `Shutdown: done` in its log.
After each boot it replayed only 6–7 reversible blocks (0.003 s), without a full replay.

### Sync progress of a fresh node
A node without block 1 showed about 90 % synced: the progress was measured from Unix time 0 instead of the first
block. It now shows 0 % until the first block arrives.

### Files
- **Settings:** `%APPDATA%\org.graphene.node-gui\settings.json`; the session-end log `gui.log` sits next to it.
- **Language:** the chosen language is stored in `settings.json`, not in the registry.

## Bug fixes (all platforms)

### Crash on shutdown while syncing
While syncing, the P2P node queues many `handle_block` calls ahead of time. Shutdown yields while it waits for the
plugins, the P2P node and the database, so the queued calls kept pushing blocks into a database that was being rewound
and closed. On Windows this crashed with an access violation right after "Rewinding from N to M", in 4 of 14 stops
during sync.

- From the start of shutdown, `handle_block` and `handle_transaction` refuse new items.
- The plugins and the chain database are shut down only after the items already being applied have finished, waiting
  up to 30 s.

Verified: 0 of 14 crashes with the change.
Commit: graphene-core `e9cb629`.

### Websocket server shutdown hung or used a destroyed server
Two defects in the websocket server's destructor broke the node's shutdown when clients connected while it stopped:

- **Hang.** The destructor waited without limit for the listener's "closed" callback. websocketpp sends it only if an
  accept is pending at that moment, so right after a connection the node never exited (3 of 4 stops under a
  connection storm). The waits for this callback and for clients to close are now limited to 10 s, with a warning in
  the main log.
- **Use after free.** A connection still opening when the server stopped could call its close or fail handler after
  the server was destroyed (2 of 24 stops crashed). The handlers now hold their own copy of the shutdown state and
  return once the server is gone.

Verified: 16 of 16 stops under a connection storm exited cleanly.
Commit: graphene-fc `f592269`.

The node also logs each shutdown step (`Shutdown: stopping plugins`, `closing the P2P network`, `closing the chain
database`, …), so a slow stop shows where it waits.
Commit: graphene-core `3bc569e`.

### Private keys in the log
A malformed `private-key` entry put the private key into the exception text, and so into the log, in three cases:

- the "Invalid WIF-format private key" message printed it;
- a WIF given in place of the public key failed in `public_key_type` with the WIF in the message;
- a JSON error carried the whole entry.

Any failure while parsing the entry is now replaced with an error that names at most the public key.
Commit: graphene-core `bf3be87`.

### One node per data directory
Nothing stopped two nodes from opening one data directory and corrupting its database. This could happen with:

- a manual run next to a systemd service;
- two copies of the node;
- the Windows GUI next to a node started by hand.

The node now takes an operating system lock on `<data dir>/witness_node.lock` before it reads its configuration or
opens the database. It uses `LockFileEx` on Windows and `flock` elsewhere. A second node prints which PID holds the
directory and exits with code 3:

```
Another witness_node (PID 4312) is already using the data directory ...
Stop it first: two nodes on one data directory corrupt its database.
```

The lock is released with the process, even after a crash or `kill -9`, so there is never a stale lock file to remove.
Commit: graphene-core `434a654`.

## P2P user agent with the build string

The P2P hello message carries no version, so peers could not tell releases apart. The node's user agent is now
`Graphene Reference Implementation <build>`, e.g. `Graphene Reference Implementation 1.2.1-286e080c`. The hello
message itself is unchanged.

## Log format

File appender lines now carry the level before the `]`:

```
2026-10-03T23:18:54  th_a:?unnamed?   main info  ] Started Graphene node on a chain with 123804 blocks.
```

Searches for `] message` still match.
Commit: graphene-fc `9706c96`.

## Compatibility

- No protocol changes and no hardforks; the database format is unchanged.
- **Exit code 3** is new: another node holds the data directory. Service managers that restart on failure will retry
  until the other node stops.
- **P2P user agent:** `Graphene Reference Implementation` is followed by the build string.
- **Log format:** lines in the file log have a new level field. Tools that parse the log by column position need
  updating.
- **Lock file:** `witness_node.lock` appears in the data directory. Do not delete it while a node is running. Deleting
  it does not release the lock, but a new node would then create a lock on a different file.
- `--shutdown-event` and `--parent-pid` exist only in Windows builds.

## Known limitations

- **No installer.** The Windows files are shipped as a folder.
- **GUI tests.** The GUI's supervisor tests run under Wine. The window itself (WebView2) was checked by hand on
  Windows 10.
- **Explorer restarts on the test machine** (`explorer.exe`, event 1002) began months before 1.2.1 and continue with
  the app removed from startup. They are not attributed to the node or the GUI.

## Components

| Repository | Branch | Commit |
|---|---|---|
| [carbon-witness/graphene-core](https://github.com/carbon-witness/graphene-core/tree/feature/windows-witness-node) | `feature/windows-witness-node` | `graphene-1.2.1` |
| [carbon-witness/graphene-fc](https://github.com/carbon-witness/graphene-fc/tree/feature/windows-witness-node) | `feature/windows-witness-node` | `f592269` |
| [carbon-witness/websocketpp](https://github.com/carbon-witness/websocketpp/tree/graphene) | `graphene` | `571a7b0` |
| [carbon-witness/editline](https://github.com/carbon-witness/editline/tree/graphene) | `graphene` | `224e256` |
| [carbon-witness/secp256k1-zkp](https://github.com/carbon-witness/secp256k1-zkp/tree/graphene) | `graphene` | `bd06794` |

websocketpp, editline and secp256k1-zkp are unchanged since 1.2.0.

## Commits

**graphene-core**
- [`265d67ca`](https://github.com/carbon-witness/graphene-core/commit/265d67ca49d8bd80dfd8e62fc592802f441e4933) witness_node: add --shutdown-event and --parent-pid on Windows
- [`f9fbd272`](https://github.com/carbon-witness/graphene-core/commit/f9fbd272a7e8ddcbdda4e1c2145dca66e4125793) contrib/win64: cross-build witness_node.exe with MinGW-w64
- [`deafaf2e`](https://github.com/carbon-witness/graphene-core/commit/deafaf2e47cd94f80e4a8264e2c36132e1a2e0f8) contrib/win64: add the MinGW-w64 toolchain file
- [`96b9a3f8`](https://github.com/carbon-witness/graphene-core/commit/96b9a3f8a8b3484b2fa7546071e1a089aa40a5fd) contrib/win64: keep the fc fixes as a patch
- [`0db8e60a`](https://github.com/carbon-witness/graphene-core/commit/0db8e60a5ea171f5f6261b13b5efeb44f7f9e327) fc: build with MinGW-w64 for 64-bit Windows
- [`30f5cabe`](https://github.com/carbon-witness/graphene-core/commit/30f5cabe53ef43c3359a22550ccff0a8af400825) witness_node: exit cleanly when the console window is closed on Windows
- [`e9cb6290`](https://github.com/carbon-witness/graphene-core/commit/e9cb6290896a1c3d0f7a4499bd8bc66a151f4f6f) app: refuse P2P blocks during shutdown and let applied ones finish
- [`bf3be87c`](https://github.com/carbon-witness/graphene-core/commit/bf3be87c2740f44d1cee5c79aacf6b74c4c903ab) witness: keep private keys out of private-key parse errors
- [`e67fa481`](https://github.com/carbon-witness/graphene-core/commit/e67fa481ca9cf80f666c090b364aab985cba9924) gui: tray supervisor and dashboard for witness_node on Windows
- [`8eabcc73`](https://github.com/carbon-witness/graphene-core/commit/8eabcc737c84237d97015a1a16ea4b833dcca7a0) gui: languages, Linux-coloured journal, close dialog, second-node guard
- [`3bc569e5`](https://github.com/carbon-witness/graphene-core/commit/3bc569e56d89d462bf73bd9201874510c594abae) Stop the node quickly from the GUI; log each shutdown step
- [`8cfd0956`](https://github.com/carbon-witness/graphene-core/commit/8cfd095617fd3c9de9928fe7ef1ed0e109ec4c87) gui: node and data paths follow the app's folder
- [`97a33742`](https://github.com/carbon-witness/graphene-core/commit/97a33742d6f0ee71b3dea4f6022cc5a6e1b53472) gui: an error box with OK when the node cannot start
- [`f7c6755c`](https://github.com/carbon-witness/graphene-core/commit/f7c6755c8aea4279fbcdeccaf1cef3cf94f5d0e1) gui: check for a conflicting node before the window opens
- [`434a6541`](https://github.com/carbon-witness/graphene-core/commit/434a654163d911ea8e05e9ef8b81f3ba352d2e34) witness_node: refuse to start on a data directory another node is using
- [`90d7c2b9`](https://github.com/carbon-witness/graphene-core/commit/90d7c2b97542c5fb1b15131833097985c5b4283d) gui: a node refused for a data folder in use is not a crash
- [`5da5c17c`](https://github.com/carbon-witness/graphene-core/commit/5da5c17c471cc6578f7c863d59a89daf62951220) witness_node: keep the window open on a startup error after a double-click
- [`9769c15b`](https://github.com/carbon-witness/graphene-core/commit/9769c15b71b1c30a1814d56972753c286e16b419) gui: graphene-node-gui.exe, player glyphs, start with Windows
- [`e24ffd79`](https://github.com/carbon-witness/graphene-core/commit/e24ffd79fb63b27212dfca76db5ee70a12d74b38) witness_node: end the process right after a clean shutdown on Windows
- [`65d9659e`](https://github.com/carbon-witness/graphene-core/commit/65d9659e249940f7c651217f34d06a61b87f3420) gui: end a node that logged its shutdown but did not exit
- [`f09c18b4`](https://github.com/carbon-witness/graphene-core/commit/f09c18b4fc11e1b4422a3471bba5467804fa82db) gui: larger tray glyphs; a plain yellow disc while starting or syncing
- [`4e889b31`](https://github.com/carbon-witness/graphene-core/commit/4e889b31e6f55dcc2cbf99c37a7867dca94e57d4) gui: restart glyph during a restart; the window shows the tray's glyph
- [`7ae59024`](https://github.com/carbon-witness/graphene-core/commit/7ae5902453cd542b18de0fde2b8be858a510a56f) gui: the syncing glyph is a yellow dot inside the white circle
- [`5749bcdf`](https://github.com/carbon-witness/graphene-core/commit/5749bcdfd197411711b4cec90ba10e0234d0c705) gui: remove the tray icon before the app exits
- [`e6cfb6d6`](https://github.com/carbon-witness/graphene-core/commit/e6cfb6d60b6dcbd80c3e543890c3ea54a9efc19f) gui: bound the journal's blocks table like the raw feed
- [`948743d0`](https://github.com/carbon-witness/graphene-core/commit/948743d0a691d5d3bf3091786eb8c5157882f648) gui: stop the node cleanly when Windows shuts down, restarts or logs off
- [`d9f066a8`](https://github.com/carbon-witness/graphene-core/commit/d9f066a856e4d20584944c7c500bebd017a967c0) witness_node: ask Windows to end the node last at shutdown
- [`0549560a`](https://github.com/carbon-witness/graphene-core/commit/0549560aea9a8ba9014d6e9a6f97bd5e4eaf5902) gui: stop the node on WM_QUERYENDSESSION; log the session end
- [`40801f47`](https://github.com/carbon-witness/graphene-core/commit/40801f4760e571d10be01a0bc36e7a3d9a0b154a) gui: update the tray tooltip and menu only when they change

**graphene-fc**
- [`ac00004`](https://github.com/carbon-witness/graphene-fc/commit/ac00004908f44669c89b578b56d4af808f7caff0) Build with MinGW-w64 for 64-bit Windows
- [`9706c96`](https://github.com/carbon-witness/graphene-fc/commit/9706c96e8a6bb1533e46979dc45b2a06b3eb0cde) log: write the level into file appender lines
- [`f592269`](https://github.com/carbon-witness/graphene-fc/commit/f592269d033147fa1544f23ff658361f58ee4704) websocket: a server shutdown no longer hangs or touches a destroyed server
