# graphene-core 1.1

**Date:** September 15, 2026
**Branch:** `fix/modern-toolchain` ([carbon-witness/graphene-core](https://github.com/carbon-witness/graphene-core/tree/fix/modern-toolchain))
**Commit:** `e34d05b`
**Previous version:** 1.0 — commit `23df6159` (May 18, 2022)

## Summary

graphene-core 1.1 builds on current Linux distributions. Version 1.0 built only on Ubuntu 18.04–20.04; from Ubuntu 22.04 onwards the build failed. Besides the port to the new toolchain, this release fixes four bugs that showed up at runtime: the node hanging or crashing on shutdown, blind transfers in `cli_wallet` being rejected, a crash in Diffie–Hellman, and stack traces without function names.

Consensus rules, block format and serialization are unchanged.

## Supported platforms

| | 1.0 | 1.1 |
|---|---|---|
| Tested OS | Ubuntu 18.04 | Ubuntu 26.04.1 |
| Compiler | GCC 7.5 | GCC 15.2 |
| Boost | 1.65.1 | 1.90 |
| OpenSSL | 1.1.1 | 3.5.5 |
| CMake | 3.x | 4.2.3 (3.5 minimum) |
| Autoconf | 2.69 | 2.72 |

1.1 has been built and tested only on Ubuntu 26.04.1. Building on Ubuntu 18.04–24.04 has not been tested.

## Building

Ubuntu 26.04 packages:

```
sudo apt-get install build-essential cmake git autoconf automake libtool pkg-config libboost-all-dev libssl-dev libreadline-dev zlib1g-dev libbz2-dev libcurl4-openssl-dev libzstd-dev libncurses-dev libicu-dev liblzma-dev doxygen
```

Compared to 1.0, `libicu-dev` and `liblzma-dev` are new: static Boost needs them at link time.

**Option 1, debug build** (`witness_node` 366 MB, `cli_wallet` 426 MB): same optimisation as Release plus full debug information, for running the node under a debugger or reading its stack traces line by line. Sync speed is unaffected (1 h 56 min against 1 h 59 min for the Release build).

```
git clone --recurse-submodules -b fix/modern-toolchain https://github.com/carbon-witness/graphene-core.git
cd graphene-core && mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_CXX_FLAGS_RELWITHDEBINFO="-O3 -g -DNDEBUG"
make -j2 witness_node cli_wallet
```

A plain `-DCMAKE_BUILD_TYPE=Debug` builds with `-O0` and produces a noticeably slower binary; that build is not tested for this release.

**Option 2, slim build** (`witness_node` 27 MB, 20 MB after `strip`; `cli_wallet` 33 MB before `strip`): the build to run in production, on a small VPS or in a container image. The two `strip` lines are optional. After `strip` the stack traces print bare addresses instead of function names, so skip them on a node you may need to diagnose.

```
git clone --recurse-submodules -b fix/modern-toolchain https://github.com/carbon-witness/graphene-core.git
cd graphene-core && mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
make -j2 witness_node cli_wallet
strip programs/witness_node/witness_node
strip programs/cli_wallet/cli_wallet
```

Run `make` without targets to build all programs and tests. The compiler needs about 2–4 GB of memory per job; on machines with little RAM, add swap.

The `libraries/fc`, `fc/vendor/websocketpp` and `fc/vendor/editline` submodules now point to the `carbon-witness` forks. In an existing clone, run `git submodule sync --recursive && git submodule update --init --recursive` after updating.

## Bug fixes

### Node hang and crash on shutdown
If RPC requests arrived while the node was shutting down (`SIGINT`/`SIGTERM`), the node crashed with `SIGSEGV` or hung and never exited. The RPC server was destroyed after the database had been closed, and websocket server handlers waited on the main thread, which was no longer running tasks.

- RPC servers are stopped first, before plugins, P2P and the database.
- Websocket server handlers capture the connection by value; the server tracks pending handler work and waits for it before it is destroyed.

Verified with 20 shutdowns under concurrent RPC load: all exited cleanly within 1–2 seconds (before the fix, 3 out of 3 crashed).
Commits: graphene-fc `a108c38`, graphene-core `1702df2`.

### Blind transfers from `cli_wallet` rejected by the network
`cli_wallet` signed blind transaction outputs with 49-bit range proofs — a value taken from BitShares, whose maximum share supply is 10¹⁵. On this chain `GRAPHENE_MAX_SHARE_SUPPLY` is 10¹³, so `blind_transfer_operation::validate` rejected every blind transfer with change. This bug was also present in 1.0.

- The range proof size is derived from `GRAPHENE_MAX_SHARE_SUPPLY` (43 bits on this chain) and checked by a `static_assert`.

Commit: graphene-core `4d845ba`.

### Crash in Diffie–Hellman (fc)
OpenSSL 3 refuses to generate DH parameters below 512 bits, and `fc::diffie_hellman::generate_params` then dereferenced a null pointer. In addition, without the `q` parameter OpenSSL 3 accepts any generator, so parameter validation was weaker than with OpenSSL 1.1.

- `generate_params` returns `false` when OpenSSL does not generate parameters.
- `validate` accepts only generators 2 and 5, as with OpenSSL 1.1.

The node does not use DH. Commit: graphene-fc `516266e`.

### Stack traces without function names (fc)
With Boost 1.90, the stack traces fc writes to the log on a crash contained only addresses.

- When GCC provides `libbacktrace`, Boost.Stacktrace uses it, and stack traces again include function names and source lines.

Commit: graphene-fc `516266e`.

## Changes for the new toolchain

**Build system**
- `cmake_minimum_required` raised to 3.5 in graphene-core, fc and websocketpp: CMake 4 no longer supports older values.
- The `system` component removed from the Boost component lists: its CMake package was removed in Boost 1.89.
- editline: `AS_IF` bodies in `configure.ac` quoted for autoconf 2.72; editline is configured with `-std=gnu17`.
- The `-Wtemplate-body` error disabled for GCC ≥ 14.
- fc tests: `BOOST_TEST_DYN_LINK` is defined only with dynamic Boost.

**Boost.Asio 1.87+** (fc and websocketpp)
- `io_service` → `io_context`, `io_service::work` → `executor_work_guard`.
- `strand::wrap` → `bind_executor`, `io_context::post` → `asio::post`, `reset` → `restart`, `expires_from_now` → `expiry()`.
- `resolver::query` / `resolver::iterator` → `resolve(host, port)` / `results_type`.
- `address_v4::from_string` / `to_ulong` → `make_address_v4` / `to_uint`; `ssl::rfc2818_verification` → `ssl::host_name_verification`.
- Storage reserved for `fc::fwd<tcp_socket::impl>` increased from 84 to 136 bytes.
- The websocketpp port is based on [zaphoyd/websocketpp#1164](https://github.com/zaphoyd/websocketpp/pull/1164).

**Other libraries and the compiler**
- `endian_buffer::data()` returns `unsigned char*`: a cast to `const char*` was added. The bytes written are unchanged.
- `recursive_directory_iterator::level()` → `depth()`.
- `FIPS_mode_set` is called only with OpenSSL < 3.
- Explicit `#include` of `<cstdint>`, `<algorithm>` and Boost.Range headers.

## Tests

| Suite | 1.0 on the new toolchain | 1.1 |
|---|---:|---:|
| `chain_test` | 5 / 304 | 304 / 304 |
| `fc all_tests` | 60 / 65 | 65 / 65 |
| `cli_test` | 14 / 15 | 15 / 15 |
| `app_test` | 6 / 6 | 6 / 6 |

The tests were inherited from BitShares and assumed its parameters: a 5-second block interval, a share supply of 10¹⁵ and the `BTS` address prefix. In 1.1 the tests are adapted to this chain's parameters (3 seconds, 10¹³, `GPH`); only tests were changed:

- fixture genesis timestamps are rounded down to the block interval;
- `witness_pay_test`, `change_block_interval`: expected values for a 3-second interval;
- `miss_some_blocks`: witnesses that did not miss a slot are taken from the last two blocks;
- `force_settle_test`: settlement delay of 20 block intervals;
- `price_test`, `verify_account_authority`: a constant for a 10¹³ supply, a key with `GRAPHENE_ADDRESS_PREFIX`;
- worker tests and `zero_second_vbo`: witness pay is set to zero so the budget reaches workers; `zero_second_vbo` amounts scaled down 100x;
- `cli_confidential_tx_test`: blinds 10M instead of 100M (the entire supply of this chain);
- fc DH tests use 512-bit parameters.

Commits: graphene-core `e34d05b`, `4d845ba`; graphene-fc `b044859`, `516266e`.

## Performance

Full sync from scratch (default plugins, 2 vCPU, 3.8 GB RAM):

| Build | Time | Speed | Peak memory | Data |
|---|---:|---:|---:|---:|
| Release | 1 h 59 min | ~7,800 blocks/s | 230 MB | 9.0 GB |
| Debug info (before the shutdown fixes) | 1 h 56 min | ~7,900 blocks/s | 233 MB | 9.0 GB |

The Release `witness_node` binary is 27 MB.

## Compatibility

- No protocol changes and no hardforks. The wallet change only affects how `cli_wallet` builds blind transactions; the network's validation rules are unchanged.
- 1.1 binaries link against the system `libssl.so.3` and `libcurl.so.4` and do not run on Ubuntu 18.04–20.04.
- Running 1.1 on a data directory created by a 1.0 node has not been tested. Back up the data directory before upgrading.

## Known limitations

- `delayed_node`, `genesis_update`, `network_mapper`, `js_operation_serializer`, `get_dev_key`, `convert_address`, `size_checker` and `member_enumerator` build, but have only been checked by running them with `--help`.
- Block production by a witness has not been tested on 1.1.
- At release time, 2 of the 14 seed nodes in `libraries/egenesis/seed-nodes.txt` were reachable.
- `witness_node --version` shows a `git describe` string based on an old BitShares tag (`2.0.171025-minor-fix-1-…`), not a Graphene version number.

## Components

| Repository | Branch | Commit |
|---|---|---|
| [carbon-witness/graphene-core](https://github.com/carbon-witness/graphene-core/tree/fix/modern-toolchain) | `fix/modern-toolchain` | `e34d05b` |
| [carbon-witness/graphene-fc](https://github.com/carbon-witness/graphene-fc/tree/fix/modern-toolchain) | `fix/modern-toolchain` | `a108c38` |
| [carbon-witness/websocketpp](https://github.com/carbon-witness/websocketpp/tree/fix/modern-toolchain) | `fix/modern-toolchain` | `c8a7a54` |
| [carbon-witness/editline](https://github.com/carbon-witness/editline/tree/fix/modern-toolchain) | `fix/modern-toolchain` | `a92d593` |

## Commits

**graphene-core**
- [`88905cb`](https://github.com/carbon-witness/graphene-core/commit/88905cb19231c06084ac3ffea098bce3f4fbaf61) build: support CMake 4 and Boost >= 1.89
- [`914ed91`](https://github.com/carbon-witness/graphene-core/commit/914ed91f92e1b4bf07e3cdf448c4b08c6557e2b1) build: GCC 14+ template-body errors, submodules on carbon-witness forks
- [`343df6b`](https://github.com/carbon-witness/graphene-core/commit/343df6bf5b2beef74b48c4a0f9461f377c9516a0) fc: bump to uint128 endian buffer cast fix
- [`091e494`](https://github.com/carbon-witness/graphene-core/commit/091e4949dc9e77ec84fd73ca2263028f642a95e0) app: include Boost.Range headers explicitly
- [`119a45b`](https://github.com/carbon-witness/graphene-core/commit/119a45b093aba8060fb42e9da3a8135c599b0dea) fc: bump to Boost.Test static link fix
- [`e897485`](https://github.com/carbon-witness/graphene-core/commit/e8974855804b90d03310e5e07e29c166e60f9a41) fc: bump to OpenSSL 3 DH fixes and symbolized stack traces
- [`1702df2`](https://github.com/carbon-witness/graphene-core/commit/1702df2f73bbd47f8a85b49d5283ec000e1b45f9) app: stop RPC servers before plugins, P2P and chain database on shutdown
- [`4d845ba`](https://github.com/carbon-witness/graphene-core/commit/4d845ba0b42218009647a6fc907141d9ebde99dc) wallet: size range proofs to GRAPHENE_MAX_SHARE_SUPPLY; fix cli_confidential_tx_test
- [`e34d05b`](https://github.com/carbon-witness/graphene-core/commit/e34d05b422e3484aadd1d67637e616576a5460d2) tests: adapt chain_test to this chain's parameters

**graphene-fc**
- [`a4de83e`](https://github.com/carbon-witness/graphene-fc/commit/a4de83e95594bbd1a65a54e324b1dbce83dce540) build: support Ubuntu 26.04 toolchain (GCC 15, CMake 4, Boost 1.90, OpenSSL 3.5)
- [`37db5a8`](https://github.com/carbon-witness/graphene-fc/commit/37db5a8d7e39aba0e208029d08ced27449f18be1) raw: cast uint128 endian buffer data() to const char*
- [`b044859`](https://github.com/carbon-witness/graphene-fc/commit/b044859318889aaf562bfb439056cf2c30ff5552) tests: fix Boost.Test build with static Boost 1.90
- [`516266e`](https://github.com/carbon-witness/graphene-fc/commit/516266e0bc727b0d20801d99f6a6ebb009ec236d) crypto, stacktrace: OpenSSL 3 DH and symbolized stack traces
- [`a108c38`](https://github.com/carbon-witness/graphene-fc/commit/a108c3802dd5fa061b7c2f279d363e9e5be9c0dd) websocket: fix hang and crash when a server is destroyed during requests

**websocketpp**
- [`c8a7a54`](https://github.com/carbon-witness/websocketpp/commit/c8a7a5495c52a7f38d5cd9e492e358b186eef29b) asio: support Boost >= 1.87

**editline**
- [`a92d593`](https://github.com/carbon-witness/editline/commit/a92d593a79fb3ff46949d265e9031a120a34b426) build: quote AS_IF bodies for autoconf 2.72
