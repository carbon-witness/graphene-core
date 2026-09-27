# graphene-core 1.2.0

**Date:** September 27, 2026
**Branch:** `graphene` ([carbon-witness/graphene-core](https://github.com/carbon-witness/graphene-core/tree/graphene))
**Tag:** `graphene-1.2.0`
**Previous version:** 1.1 — tag `graphene-1.1` (September 15, 2026), [release notes](https://github.com/graphene-blockchain/graphene-core/blob/graphene-1.1/RELEASE_NOTES_1.1.md)

## Summary

graphene-core 1.2.0 fixes the four defects found after the 1.1 release: the peer database erased on every shutdown,
a BitShares version string in `--version`, `cli_wallet` unable to connect over `wss://`, and unmasked websocket frames
from clients. Two more were found and fixed along the way: a startup race that delayed `--seed-node` connections by
30 seconds, and the intermittent `app_test/two_node_network`.

1.2.0 is also the first release with a working Docker image. It is built on Ubuntu 26.04 and published to Docker Hub
and the GitHub Container Registry by a GitHub Actions workflow.

All submodules now come from the `carbon-witness` forks; the fork of `secp256k1-zkp` is new in 1.2.0.

The built-in seed node list has been refreshed.

Consensus rules, block format and serialization are unchanged.

## Getting the node

The five ways to get a node, from Docker images to source builds, are described in [README.md](README.md#getting-started);
running the node in a container is described in [README-docker.md](README-docker.md).

```
docker pull carbonwitness/graphene-core:1.2.0
docker pull ghcr.io/carbon-witness/graphene-core:1.2.0
```

Release candidates are published only under their own tags, e.g. `1.2.0-rc2`; `latest` points to the newest release.

The toolchain is the same as in 1.1: Ubuntu 26.04 with GCC 15, CMake 4, Boost 1.90 and OpenSSL 3.5. In an existing
clone, run `git submodule sync --recursive && git submodule update --init --recursive` after updating: the
submodules point to new repositories (see below).

## Repositories

All submodules are built from the `carbon-witness` forks, branch `graphene`:

- [graphene-fc](https://github.com/carbon-witness/graphene-fc) (`libraries/fc`) carries the 1.2.0 fixes: TLS 1.2+,
  masked client frames and the git hash;
- [websocketpp](https://github.com/carbon-witness/websocketpp) and [editline](https://github.com/carbon-witness/editline)
  (`fc/vendor/`) moved from the `graphene-blockchain` forks, on the same commits as in 1.1;
- [secp256k1-zkp](https://github.com/carbon-witness/secp256k1-zkp) (`fc/vendor/secp256k1-zkp`) is a new fork in 1.2.0.
  1.1 took the library directly from `bitshares/secp256k1-zkp`; the fork is on the same commit, `bd06794`, so the code is
  unchanged, and every dependency of the node is now built from a repository of this project.

## Bug fixes

### Peer database erased on every shutdown
The P2P node was closed three times during one shutdown: in `application::shutdown()`, in `~application()` and in
`~node_impl()`. `peer_database_impl::close()` saved the peers to `p2p/peers.json` and cleared them, so the second and
third calls wrote `[]` over the file. The node always started from the built-in seed nodes.

- `peer_database_impl::close()` writes the file only on the first call after `open()`.

Verified with two local nodes: after `SIGINT` the old binary leaves `[]` in `peers.json`, the new one keeps the peer.
The new test `tests/tests/peer_database_tests.cpp` fails on the old code and passes on the new.
Commit: graphene-core `172f6006`.

### `--version` showed a BitShares tag
The version came from `git describe --tags`, and the history still contains the BitShares release tags. The result
was `2.0.171025-minor-fix-1-…`, or `unknown` in a clone without tags.

- The release number is set in the code: `set( GRAPHENE_VERSION "1.2.0" )` in the root `CMakeLists.txt`. It changes
  together with the `graphene-<version>` tag and is also used as the CPack package version.
- The build string is the version plus 8 characters of the commit hash, e.g. `1.2.0-8a4c1123`; without git (a
  source archive), just `1.2.0`. A new commit is picked up by `make`, without re-running `cmake`.
- `witness_node --version` and `cli_wallet --version` print an aligned table; `about()` in the wallet reports the same
  build string as `client_version`.

```
Version:     1.2.0
Build:       1.2.0-8a4c1123
SHA:         8a4c1123c50183e6d604598920787727ce61bd9c
Timestamp:   …
SSL:         OpenSSL 3.5.5 27 Jan 2026
Boost:       1.90
Websocket++: 0.7.0
```

Fixed along the way:
- `get_git_head_revision()` in fc returned the branch name instead of the hash when refs were packed (`git gc`,
  `git pack-refs`). The hash now comes from `git rev-parse HEAD`, which fixes both `graphene_revision` and
  `fc_revision`.
- The P2P user agent is `Graphene Reference Implementation` instead of `BitShares Reference Implementation`.

Commits: graphene-core `71c789b0`, `7f180caa`; graphene-fc `290808d`.

### `cli_wallet` could not connect over `wss://`
fc created its TLS contexts as `ssl::context::tlsv1`, which fixes both the minimum and the maximum version to TLS 1.0.
OpenSSL 3.5 forbids TLS 1.0 at its default security level, so the client sent a `protocol_version` alert instead of a
ClientHello. The node's TLS RPC server could not accept a single connection either.

SNI was not broken, contrary to the 1.1 release notes: websocketpp sets it from the endpoint role.

- Contexts are created with `tls_client` / `tls_server`; TLS 1.2 and 1.3 are supported, and a connection uses the
  newest version both sides know. TLS 1.0 and 1.1 are disabled.

Verified end to end: `cli_wallet` → `witness_node` over `wss` connects with TLS 1.3 and SNI. A certificate for another
host name, an unknown CA and a TLS 1.0-only server are rejected.
Commit: graphene-fc `85f5d52`.

### Websocket client frames were sent with a zero mask
fc's client endpoints were built on the server configs `asio` / `asio_tls`. Their base `config::core` uses
`random::none` as the random number generator, which always returns zero, so every client frame carried the masking
key `00 00 00 00`. RFC 6455 requires a random key, and strict intermediaries drop such frames.

- Client endpoints have their own configs based on `asio_client` / `asio_tls_client`, with `random_device` as the
  generator. Logging, the handshake timeout and permessage-deflate are unchanged.

Verified with a probe that prints the masking key of each client frame: `00000000` with the old `cli_wallet`, random
keys with the new one, over both `ws` and `wss`.
Commit: graphene-fc `85f5d52` (a separate change from the TLS fix, in the same commit).

### `--seed-node` connections delayed by 30 seconds
The node connected to its `--seed-node` peers before `connect_to_p2p_network()`. Until the connection loop is running,
`is_accepting_new_connections()` returns false, so the seed's early hello was rejected with "not accepting any more
incoming connections", and the next attempt came only after
`GRAPHENE_NET_DEFAULT_PEER_CONNECTION_RETRY_TIME` (30 seconds).

- Connections to seed nodes are opened after `connect_to_p2p_network()` and `sync_from()`.

Commit: graphene-core `ba391107`.

### Intermittent `app_test/two_node_network`
The test failed in about half of the runs on a 2-core machine. There were four causes:

- the two nodes could get different genesis files, and so different chain ids, when a 3-second boundary fell between
  the two `create_genesis_file()` calls; both nodes now share one file;
- the second node's port stayed in `TIME-WAIT` for about a minute after a run; it now listens on a random port;
- the `--seed-node` startup race above;
- a transaction broadcast while the peer was still syncing was dropped; the test now repeats the broadcast every
  500 ms until the transaction arrives.

Fixed `usleep` calls were replaced with `wait_for()`, which polls a condition for up to 10 seconds.
Verified: 100 / 100 single runs, 10 / 10 full `app_test` runs, 20 / 20 with both cores under load.
Commit: graphene-core `99037764`.

## Seed nodes

The built-in list of seed nodes in `libraries/egenesis/seed-nodes.txt` has been updated to the nodes that currently
accept P2P connections.

Commit: graphene-core [`5990f378`](https://github.com/carbon-witness/graphene-core/commit/5990f378f408db753c5332579dad9a5e75013193).

## Docker image

The `Dockerfile` inherited from BitShares was based on `phusion/baseimage:0.11` (Ubuntu 18.04) and no longer built.
It has been rewritten.

- **Multi-stage build.** The `builder` stage compiles `witness_node`, `cli_wallet` and `get_dev_key` on Ubuntu 26.04
  as `RelWithDebInfo` (`-O3 -g`), linking with mold and caching with ccache.
- **Final image:** Ubuntu 26.04 with the stripped binaries and their runtime libraries, 254 MB on disk and 67 MB
  compressed.
- **Debug symbols** are split off with `objcopy` into a separate `debug-symbols` target. The workflow attaches them to
  the GitHub release of the tag as `graphene-core-X.Y.Z-debug-symbols-linux-x86_64.tar.xz`.
- **Version.** The build context includes `.git`, so `--version` in the container shows the commit hash.
- **Submodule check.** A clone without `--recurse-submodules` fails at the start of the build with a clear message.
- **Data.** Blockchain, `config.ini` and logs live in `/var/lib/graphene`. The image declares no `VOLUME`; mount a
  named volume or a host directory there.
- **Shutdown.** `STOPSIGNAL SIGINT`; `compose.yml` sets `stop_grace_period: 5m`, with `docker run` use
  `--stop-timeout 300`. The default of 10 seconds can kill the node before it writes its database, and the next start
  replays the whole chain.
- **Entry point.**
  - The node writes its default `config.ini` into the data directory on the first start. The old entry point replaced
    the user's `config.ini` with a symlink on every start.
  - Default endpoints: P2P `0.0.0.0:1776`, RPC `0.0.0.0:8090`.
  - Arguments after the image name go to `witness_node`; an argument that does not start with `-` is run as a command,
    e.g. `cli_wallet` or `get_dev_key`.
  - The `GRAPHENED_*` environment variables are kept.
- **Permissions.** The node runs as uid/gid 10001 (`graphene`). The container starts as root, fixes permissions and
  drops to 10001 with `setpriv`:
  - a data directory whose top level belongs to someone else is chowned to 10001;
  - a file passed to `witness_node` that 10001 cannot read, such as a bind-mounted `api-access.json` with mode 600
    owned by root, is copied to `/run/graphene` and the option is pointed to the copy;
  - `witness_node` stays PID 1 and receives `SIGINT` from `docker stop` directly.

  Started with `--user`, the container does none of this. Commands run with `docker exec` start as root: add
  `-u graphene`.
- `docker/default_config.ini` with BitShares settings has been removed.

### Publishing

The workflow `.github/workflows/docker.yml` builds the image on every push and pull request.

- Push or pull request: build and smoke test, nothing is published; the debug symbols are kept as a workflow artifact.
- Tag `graphene-X.Y.Z`: `X.Y.Z` and `latest` to Docker Hub and GHCR; a draft release with the debug symbols.
- Tag `graphene-X.Y.Z-rcN`: only `X.Y.Z-rcN`; a draft pre-release with the debug symbols.

The compiler cache is kept between runs: a rebuild takes about 7 minutes instead of 25.

## Tests

- `peer_database_tests` (new, part of `chain_test`) and `app_test/two_node_network`: see the fixes above.
- fc `all_tests`: `websocket_test` passes.
- Docker image: the workflow's smoke test runs `witness_node --version` and `cli_wallet --version` on every build.
  Key generation with `get_dev_key`, uid 10001, ownership of the data volume and reading a root-owned
  `api-access.json` were checked by hand.

## Performance

Full sync from scratch in the Docker image `1.2.0-rc1`, pulled from Docker Hub on a VPS with 2 vCPU, running next to a
production node on the same machine, with the plugins `account_history market_history grouped_orders
api_helper_indexes`:

- 7 h 16 min to block 55,849,207; a native 1.1 node on the same VPS took about 11 h;
- about 2,000 blocks/s, steady from the first hour to the last;
- 9.07 GB of data;
- `docker stop` on the full database: 4.1 s, clean exit; the restart took 2 s and replayed 7 reversible blocks,
  without a full replay.

## Compatibility

- No protocol changes and no hardforks.
- The P2P user agent string changed from `BitShares Reference Implementation` to `Graphene Reference Implementation`.
- TLS 1.0 and 1.1 are no longer accepted by `cli_wallet` or by the node's TLS RPC server.
- Docker: the image runs the node as uid 10001, reads its configuration from the data directory and no longer ships
  `docker/default_config.ini`. A data directory created by the old image is chowned to 10001 on the first start.

## Known limitations

- The full test suite (`chain_test`, `cli_test`, `app_test`, fc `all_tests`) has not been rerun on a `-g` build
  for 1.2.0; the checks listed under Tests were run.
- `cmake -DENABLE_INSTALLER=ON` fails: CPack looks for a missing `LICENSE.md`. This predates 1.2.0.
- In fc `all_tests`, the three `fc_stacktrace` tests fail on a build without `-g`: there is nothing to symbolize.

## Components

| Repository | Branch | Commit |
|---|---|---|
| [carbon-witness/graphene-core](https://github.com/carbon-witness/graphene-core/tree/graphene) | `graphene` | `graphene-1.2.0` |
| [carbon-witness/graphene-fc](https://github.com/carbon-witness/graphene-fc/tree/graphene) | `graphene` | `551377b` |
| [carbon-witness/websocketpp](https://github.com/carbon-witness/websocketpp/tree/graphene) | `graphene` | `571a7b0` |
| [carbon-witness/editline](https://github.com/carbon-witness/editline/tree/graphene) | `graphene` | `224e256` |
| [carbon-witness/secp256k1-zkp](https://github.com/carbon-witness/secp256k1-zkp/tree/graphene) | `graphene` | `bd06794` |

websocketpp, editline and secp256k1-zkp are unchanged since 1.1; their submodule URLs now point to the `carbon-witness`
forks.

## Commits

**graphene-core**
- [`172f6006`](https://github.com/carbon-witness/graphene-core/commit/172f6006244fb23f0c1dcca52cbd6df79a497d4d) net: keep peers.json when the peer database is closed twice
- [`7f180caa`](https://github.com/carbon-witness/graphene-core/commit/7f180caaf7f00b4544d6d472e86b23427ac5fe76) app: announce the P2P node as Graphene, not BitShares
- [`71c789b0`](https://github.com/carbon-witness/graphene-core/commit/71c789b05d0ebe3649a932280a6281b725c86700) version: print 1.2.0 and a version-hash build string
- [`26965296`](https://github.com/carbon-witness/graphene-core/commit/26965296827c38793d0b3921183e2a6b09cd9e53) fc: bump to TLS 1.2+, masked client frames and the git hash fix
- [`ba391107`](https://github.com/carbon-witness/graphene-core/commit/ba391107511b00c174096df87d715d98f6398b1a) app: connect to --seed-node peers after the P2P node is running
- [`99037764`](https://github.com/carbon-witness/graphene-core/commit/990377644725367d18faacbf34c2039c0d68050c) tests: make app_test two_node_network deterministic
- [`a097bcd4`](https://github.com/carbon-witness/graphene-core/commit/a097bcd430af515c6efd21c2d938c9e582cca8f7) build: point the fc submodule at the carbon-witness fork
- [`c1b5151c`](https://github.com/carbon-witness/graphene-core/commit/c1b5151cd2c76f7f670207da1a2c5a23ee12697c) docker: rebuild the image on Ubuntu 26.04
- [`eb0d3f70`](https://github.com/carbon-witness/graphene-core/commit/eb0d3f70cf25639b893fb1901b3ae92ae1ae5863) ci: build the Docker image and publish it on release tags
- [`4df66b71`](https://github.com/carbon-witness/graphene-core/commit/4df66b7171d1a3341c096a9a156d693681b22654) ci: move the workflow actions to their Node 24 releases
- [`78b4229a`](https://github.com/carbon-witness/graphene-core/commit/78b4229ade8e91f908b8809d0105929a9e8205a7) ci: keep the compiler cache between workflow runs
- [`8798f8b8`](https://github.com/carbon-witness/graphene-core/commit/8798f8b86713f8a4fef606df4b6a043a0f5b2009) ci: do not tag pre-releases as latest
- [`8a4c1123`](https://github.com/carbon-witness/graphene-core/commit/8a4c1123c50183e6d604598920787727ce61bd9c) docker: start as root, fix permissions, drop to uid 10001
- [`949b5787`](https://github.com/carbon-witness/graphene-core/commit/949b578765018312811ed4f85c5996f3a42ebf71) fc: bump to websocketpp and editline on the carbon-witness forks
- [`82ce9145`](https://github.com/carbon-witness/graphene-core/commit/82ce9145630df03b3afa1da7e7940d9f70608556) fc: bump to secp256k1-zkp on the carbon-witness fork
- [`5990f378`](https://github.com/carbon-witness/graphene-core/commit/5990f378f408db753c5332579dad9a5e75013193) egenesis: drop the dead seed nodes, add three live ones
- README and release notes: the ways to get a node, clone commands and submodules on the carbon-witness forks ([`f8091fc1`](https://github.com/carbon-witness/graphene-core/commit/f8091fc1d17ed9ca6fb07099a45af3c1e449940e), [`ec86b97f`](https://github.com/carbon-witness/graphene-core/commit/ec86b97faf7772c28a3f0e6e3486cc0d78f4e642), [`1b970896`](https://github.com/carbon-witness/graphene-core/commit/1b970896ff43247d41af3028bb230ac7a8a5e9c6), [`254a82a0`](https://github.com/carbon-witness/graphene-core/commit/254a82a002593c2fd0ec2f77f2c0809440925e7a), [`a3934db7`](https://github.com/carbon-witness/graphene-core/commit/a3934db7b81152eef39357dbf6e128f7e4b29274), [`0a5560c2`](https://github.com/carbon-witness/graphene-core/commit/0a5560c2f4c96703f0bca9d94f2dd7ced8798e4a), [`bff0234c`](https://github.com/carbon-witness/graphene-core/commit/bff0234cd02806d50088449a79d8dd37eed141f0))

**graphene-fc**
- [`85f5d52`](https://github.com/carbon-witness/graphene-fc/commit/85f5d526958451db72bfda7612d6d84a4d67c12d) websocket: TLS 1.2+ and masked client frames
- [`290808d`](https://github.com/carbon-witness/graphene-fc/commit/290808d63bbc91fc75c2ec8dbed17d28e35315f6) cmake: take the HEAD hash from git rev-parse
- [`f516383`](https://github.com/carbon-witness/graphene-fc/commit/f51638374794b044c16153fd2e24d31ab134b30f) build: point websocketpp and editline at the carbon-witness forks
- [`551377b`](https://github.com/carbon-witness/graphene-fc/commit/551377b5bd66410e2c0ffa41bca1358452484a9a) build: point secp256k1-zkp at the carbon-witness fork
