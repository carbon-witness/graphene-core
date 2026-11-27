# graphene-core 1.2.2

**Date:** not released yet (draft; the date is set with the tag)
**Branch:** `release/1.2.2` ([carbon-witness/graphene-core](https://github.com/carbon-witness/graphene-core/tree/release/1.2.2)), to be merged into `graphene`
**Tag:** `graphene-1.2.2`
**Previous version:** 1.2.1 — tag `graphene-1.2.1` (October 7, 2026), [release notes](RELEASE_NOTES_1.2.1.md)

## Summary

graphene-core 1.2.2 is a P2P release:

- **IPv6.** The node listens on IPv4 and IPv6 on one port, tells peers all the endpoints it can be reached at, and
  connects to a node that has both over IPv6. New nodes exchange IPv6 addresses and check whether their IPv6 endpoint
  can be reached from outside; old nodes keep talking to them over IPv4.
- **TLS for P2P connections**, on the same port and compatibly with old nodes, for networks whose DPI cuts the plain
  P2P protocol.
- **Reconnecting after an outage of our own network:** the node used to stay isolated for up to hours after losing all
  of its peers at once; now every peer is retried within minutes.
- **Docker:** the examples and `compose.yml` run the node in the host network, which IPv6 needs.

Consensus rules, block format and the chain database are unchanged. New nodes and old nodes work together in one
network; the new P2P messages are ones old nodes skip.

## Getting the node

### Docker

```
docker run -d --name graphene --stop-timeout 300 --network host \
    -e GRAPHENED_RPC_ENDPOINT=127.0.0.1:8090 -v graphene-data:/var/lib/graphene \
    carbonwitness/graphene-core:1.2.2
```

or the same image from `ghcr.io/carbon-witness/graphene-core:1.2.2`. In the host network ports are not published, so
the RPC must be bound to localhost with `GRAPHENED_RPC_ENDPOINT`; the entry point warns when it is not. A node that keeps
the bridge network works over IPv4 only. Other setups, Docker Desktop included, are described in
[README-docker.md](README-docker.md#networking).

### Windows

As in 1.2.1: `graphene-node-win64-1.2.2.zip` from the GitHub release, unpacked into one folder. `witness_node.exe` has
all the P2P changes of this release.

The app is **Graphene Node GUI 1.0.1** (was 1.0.0):
- Seeds and Peers: a **Transport** column (`TLSv1.3` or `plain`); the column "Peer's block" is renamed **Last block**:
  it is the newest block the node knows the peer has, not the peer's head.
- A status bar at the bottom: the app's version, the node's build, peers and how many are under TLS, IPv6.
- The window title has both versions: "Graphene Node GUI 1.0.1 · witness_node 1.2.2-…".
- Zoom: [−] 100% [+] at the right of the status bar, and Ctrl +, Ctrl −, Ctrl 0; the window opens at the last zoom.
- A seed resolves to its IPv6 addresses too: `ipv6seed.graphene.carbon-witness.net` (an AAAA record only) showed
  "name not resolved" although the node was connected to it.
- **Add seed** takes `tls://host:port` and `?sni=name`, as `seed-node` does; it took only `host:port`. Why an address
  is not taken is shown under the field, not at the top of the window.
- The versions moved from the bottom of Settings to the window title and the status bar.

The Windows node told its peers `platform: other`: the platform was detected by `_MSC_VER`, and the build is MinGW. It
now says `win32` (with `bitness: 64`), as MSVC builds did.

### From source

See [README.md](README.md#getting-started); the build is unchanged.

## IPv6

### Listening

On a wildcard address (`0.0.0.0`, the default of the Docker image, or `[::]`) the node listens on `[::]`, which takes
IPv4 connections on the same port as well. On a system without IPv6 it falls back to `0.0.0.0`. IPv4 peers are seen at
their IPv4 addresses, not as `::ffff:…`. A specific address, IPv4 or IPv6, is used as given:

```
p2p-endpoint = 0.0.0.0:1776                # IPv4 and IPv6
p2p-endpoint = [2001:db8::1]:1776          # this IPv6 address only
```

`--p2p-endpoint` may be given more than once, e.g. for IPv4 and IPv6 on different ports; each listener has its own
accept loop, and an outbound connection goes out from the listening port of the peer's family.

### Telling peers where to connect

The address field of the P2P hello holds one IPv4 address. A new node also lists the endpoints it accepts connections
on, `listen_endpoints` in the user data of its hello, which old nodes ignore. On a wildcard address these are:

- the public IPv4 endpoint the firewall check found (behind NAT), or the IPv4 address of the host if it is public;
- the global IPv6 address of the host (on Linux and Windows a stable one rather than a temporary privacy address,
  which the system changes daily).

A peer that lists an endpoint at the address we see it connect from counts as reachable and goes into the peer
database. Without this an IPv6 peer, whose hello can only carry `0.0.0.0`, was always taken for firewalled.

`--p2p-advertise` (more than once) replaces the automatic list, e.g. for a node behind NAT or in a Docker network with
a private IPv6 subnet. `--p2p-advertise-v6=false` leaves out the IPv6 endpoints and their check; connections to IPv6
peers still work.

### A node with IPv4 and IPv6

The endpoints a node lists in its hello are kept together in the peer database (`alternate_endpoints`). The connect
loop tries IPv6 records first and skips a record while a connection to another endpoint of the same node is open or
opening. A node with both is connected once, over IPv6; its IPv4 address is tried too when the IPv6 connection is not
up after 3 s (in practice at the next pass of the connect loop, every 10 s). An open IPv4 connection is not dropped to
switch to IPv6.

A rejection "already connected" no longer counts against an address: the node is connected at another one, or from
both sides at once.

### When our IPv6 does not work

IPv6 peers on the internet are not tried while the system has no global IPv6 address (checked every minute; a new
address counts at once), and for 10 minutes after connections to 3 different peers failed over IPv6 in a row while
IPv4 worked. A successful IPv6 connection, ours or a peer's to us, ends the pause. Peers on the loopback or a local
network (`fd00::/8`, `fe80::/10`) need only a route. The node says at start when it has no global IPv6 address, with a
hint about the host network when it runs in Docker.

### Address exchange: `address_v2_message`

Nodes that announce `p2p_ipv6` in their hello answer an address request with `address_v2_message` (type 5018) instead
of `address_message`. It lists the endpoints of each connected node, IPv6 first: the one a connection was made to, and
the other endpoints of its group that worked. A group told by another node only orders the connection attempts; the
hello of the node itself confirms it, so a node cannot keep us from connecting to another one by claiming its address.

Old nodes get `address_message` as before. A peer connected over IPv6 goes there with the IPv4 endpoint it listed, or
not at all: before, an IPv6 endpoint went there as `0.0.0.0:0` and ended up in the peer database of old nodes. Loopback
endpoints go only to a peer that is itself on the loopback.

### Can our IPv6 be reached: `check_firewall_v2`

`check_firewall_v2_message` and its reply (types 5019, 5020) carry an endpoint of either family. A node asks with the
endpoint it wants checked, which must be one it listed in its hello; a helper has another node connect to it, and a
helper without working IPv6 answers "unable to check". The old and the new check share one code path. The old IPv4
check now goes over IPv4 connections only: over IPv6 the helper sees an address the old message cannot carry.

An IPv6 endpoint found unreachable (a router dropping inbound IPv6) is not advertised, with a log line:

```
our IPv6 endpoint [2001:db8::1]:1776 cannot be reached from outside; not advertising it. Allow inbound connections to port 1776 in the router or firewall
```

It is checked again after 30 minutes or when the IPv6 address changes. An inbound IPv6 connection from a public address
marks the endpoint reachable without a check.

### Peer database

IPv6 records go to `p2p/peers_v6.json`, next to `peers.json`. A node of 1.2.1 or older drops the whole `peers.json` on
the first address it cannot parse, so `peers.json` keeps IPv4 records only and stays readable after a downgrade. Both
files are now read record by record: a bad record is skipped alone instead of emptying the database.

### Network info

`network_node_api.get_info` gains `listen_endpoints`, `advertised_endpoints`, `ipv6_usable` and `firewalled_v6`.
`get_connected_peers` shows the `listen_endpoints` a peer gave, and `via_proxy` for a peer that lists only public
endpoints but comes from a private address (e.g. Docker passing IPv6 connections to a bridge network over IPv4).

### Seed nodes

- An IPv6 seed is given in brackets: `[2001:db8::1]:1776`, also `tls://[2001:db8::1]:1776?sni=name`.
- A seed host name resolves to its A and AAAA records.
- `ipv6seed.graphene.carbon-witness.net` (an AAAA record only) joins the built-in seed list.
- Fixed: the host of a seed was cut at the first colon, so for `[2001:db8::1]:1776` the string `[2001` went into TLS as
  the server name. An IP address, IPv4 or IPv6, is never sent as SNI.

## TLS for P2P connections

Some networks with DPI cut the plain P2P protocol, whose bytes look random from the first one, while they let TLS
through. P2P connections can now run over TLS on the same port:

- TLS goes under the existing stcp encryption, so the stcp layer, the hello signature and all messages stay as they
  are. Certificates are not verified (the hello authenticates the peer); a node without a configured certificate makes
  a self-signed one at startup.
- A server reads the first byte: `0x16` (ClientHello) starts TLS, `0x02`/`0x03` (an stcp key) the plain protocol.
- A client tries TLS; an old server answers with its stcp key, and the client reconnects plainly and remembers the
  address as plain. The hello carries `"p2p_transports": ["tls"]`, so such a peer is connected over TLS again once it
  upgrades.

```
p2p-tls = auto            # default; always: TLS only; off: like an old node
p2p-tls-cert = cert.pem   # optional, with p2p-tls-key
seed-node = tls://seed.example.org:1776
seed-node = tls://1.2.3.4:1776?sni=example.org
```

`get_connected_peers` shows `"transport": "plain"` or the TLS version.

## Reconnecting after a network outage

- **Retry from an OS-selected port.** The node binds its outbound connections to its listening port. While an earlier
  connection between the same two endpoints is still in TIME_WAIT or FIN_WAIT, `connect()` fails with "Cannot assign
  requested address" (EADDRNOTAVAIL). After losing all of its peers at once the node could reconnect to none of them.
  A connection that fails before the TCP connect is now retried once from a port the OS picks. A connect that the node
  aborted itself (the peer gave no answer within the handshake timeout) is not retried: such a retried connection
  belonged to no peer and stayed open (seen on Windows: one more socket every 30 seconds for each unresponsive peer).
- **The retry delay is capped.** It was (failed attempts + 1) × 30 s, and the count was never reset, so a peer that
  had failed a few hundred times over weeks was retried every one to two and a half hours. The count is reset after a
  successful handshake, and the multiplier is capped at 11: every peer is retried at least every 5.5 minutes.
- The peer database record of a connection we opened is the address we connected to, so the reset of failed attempts,
  the TLS support learned from a hello and connection errors reach the record the connect loop retries.

## Docker

`compose.yml` and the `docker run` examples use the host network (`network_mode: host`, `--network host`) with
`GRAPHENED_RPC_ENDPOINT=127.0.0.1:8090`. In a bridge network without IPv6, Docker passes inbound IPv6 connections to the
container over IPv4 through its proxy, and the peers show up as `172.17.0.1`. The entry point warns when the container
runs in the host network with the RPC on a wildcard address. [README-docker.md](README-docker.md#networking) describes
the host network, the bridge network with and without IPv6, and Docker Desktop.

## Other fixes

- **Witness:** the minority fork message printed a participation of 28.12% as "2812%".
- **Firewall check:** a request to check `0.0.0.0` (what an IPv6 endpoint becomes in the old message) is answered
  "unable to check"; a connection to `0.0.0.0` reaches the helper itself on Linux. The answer "unable to check" was built
  but never sent. A successful check put the endpoint of the helper, not of the checked node, into the peer database.
- An inbound endpoint `0.0.0.0` from a hello is not taken as the address of the peer.
- **Sync from a distant peer:** a peer that did not answer `fetch_blockchain_item_ids` within 1 second was
  disconnected, so during sync over a long route (RTT ~250 ms) the only good peer was dropped and reconnected every
  10–30 seconds. The answer now has 5 seconds (`GRAPHENE_NET_SYNC_ITEM_IDS_TIMEOUT`); blocks and transactions keep 1 second.

## CI

- The tests workflow builds and runs `app_test`, `cli_test` and fc's `any_ip_test` on pushes to work branches and on
  pull requests; `chain_test` runs on manual starts and for release tags. The docker and windows workflows build only
  the binaries.
- A push to a `release/` branch publishes the image to ghcr.io as `release-X.Y.Z` and `sha-<commit>`, to try it on test
  nodes before the tag. Docker Hub, `latest` and the GitHub release stay with the tags.

## Testing

- `app_test`: P2P over `[::1]` with TLS; a node on `[::]` with an IPv4 and an IPv6 client; two listeners; the advertise
  options as a peer sees them; address exchange over `address_v2_message`; the IPv6 records in `peers_v6.json`; seed
  addresses with IPv6; TLS between new and old nodes; the reconnect test over IPv4 and, on a machine with a global IPv6
  address, over IPv6.
- fc `any_ip_test`: IPv4/IPv6 addresses and endpoints, their strings and serialization, dual-stack and IPv6-only
  sockets on the loopback, resolving.
- A test network of six nodes on its own genesis, in four networks: IPv4 and IPv6 nodes in Moscow (two providers) and
  Germany, an IPv4-only node in Hong Kong and a node of 1.2.1. Checked: connections over IPv6 with TLS between new
  nodes and no second connection over IPv4; IPv4 with TLS between the IPv4-only node and the others; plain connections
  between 1.2.1 and 1.2.2; `address_v2_message`; `check_firewall_v2` across networks, with the port open and blocked;
  no IPv6 attempts from the IPv4-only node.
- The witness node `carbon-witness` ran the release branch on mainnet before the tag.

## Compatibility

- No protocol changes in consensus, no hardforks; the chain database format is unchanged.
- **P2P with old nodes:** the new message types 5018–5020 are in the range old nodes skip when unknown, and are sent
  only to peers that announced `p2p_ipv6`. New keys in the user data of the hello (`p2p_ipv6`, `listen_endpoints`,
  `p2p_transports`) are ignored by old nodes. TLS falls back to the plain protocol with an old peer.
- **Peer database:** new fields of the records (`transport`, `tls_server_name`, `alternate_endpoints`,
  `alternate_endpoints_confirmed`) are ignored by old nodes; IPv6 records are in `peers_v6.json`. A downgrade keeps
  `peers.json` readable.
- **Options:** `p2p-endpoint`, `seed-node` and `p2p-advertise` take IPv6 endpoints in brackets; `p2p-endpoint` may be
  repeated.
- **Docker:** the documented setup moves to the host network. A container started the old way (bridge network,
  published ports) keeps working over IPv4.
- **Listening on `0.0.0.0` now also takes IPv6** where the system has it. To keep a node IPv4-only, give it a specific
  IPv4 address or turn off IPv6 on the host.

## Known limitations

- **No switch of an open connection.** A node connected to a peer over IPv4 (e.g. because the IPv6 address was in its
  retry pause at the time) stays on IPv4 until the next reconnect.
- **Firewall checks run on outbound connections** and need a helper that is not connected to the checked node. In a
  small network where every node is connected to every other the result can stay "unknown"; an inbound IPv6 connection
  from the internet settles it.
- **Mainnet has no other IPv6 nodes yet.** IPv6 helps once more nodes run 1.2.2: until then the node talks to the
  network over IPv4, as before.
- `get_current_connections_reply` (a diagnostic message) still carries IPv6 peers as `0.0.0.0`.

## Components

| Repository | Branch | Commit |
|---|---|---|
| [carbon-witness/graphene-core](https://github.com/carbon-witness/graphene-core/tree/release/1.2.2) | `release/1.2.2` | `graphene-1.2.2` |
| [carbon-witness/graphene-fc](https://github.com/carbon-witness/graphene-fc/tree/release/1.2.2) | `release/1.2.2` | `475b778` |
| [carbon-witness/websocketpp](https://github.com/carbon-witness/websocketpp/tree/graphene) | `graphene` | `571a7b0` |
| [carbon-witness/editline](https://github.com/carbon-witness/editline/tree/graphene) | `graphene` | `224e256` |
| [carbon-witness/secp256k1-zkp](https://github.com/carbon-witness/secp256k1-zkp/tree/graphene) | `graphene` | `bd06794` |

## Commits

Since 1.2.1 (the `graphene` branch), in order; the commit that last updated this list is not in it.

**graphene-core**
- [`881b8eae`](https://github.com/carbon-witness/graphene-core/commit/881b8eaee37503fb51b2bb565de6a2ad0812e7ce) docs: what graphene-node-gui.lock holds, in the Windows GUI guide
- [`250c29b1`](https://github.com/carbon-witness/graphene-core/commit/250c29b10a7693d671874377d4b7441e0b73b949) docs: what graphene-node-gui-api.json holds, in the Windows GUI guide
- [`12a79fd0`](https://github.com/carbon-witness/graphene-core/commit/12a79fd09fcaad82e0ba9b846b29ab8987209eaa) docs: what the Run registry entry is, in the Windows GUI guide
- [`4192e92c`](https://github.com/carbon-witness/graphene-core/commit/4192e92c6636c9983df81b1150dbf516d0f589aa) docs: the Building section of the Windows GUI guide in more detail
- [`f4469842`](https://github.com/carbon-witness/graphene-core/commit/f4469842502f5e005313c5011bf63c41ac43dfb1) net: retry an outbound connection from an OS-selected port
- [`b4f7f34b`](https://github.com/carbon-witness/graphene-core/commit/b4f7f34bbde5af88d45b3a85f5789bc63e1bade0) net: reset a peer's failed attempts on success and cap the retry delay
- [`2a551b19`](https://github.com/carbon-witness/graphene-core/commit/2a551b19cbaab9fbc97282e2f5cbdafffac52f7f) witness: participation percent in the minority fork message
- [`d07286e3`](https://github.com/carbon-witness/graphene-core/commit/d07286e3509dd9d09893e29e68938d4d25057946) ci: build and run app_test and cli_test
- [`699d0000`](https://github.com/carbon-witness/graphene-core/commit/699d0000721fe437defcc9c877de1ec8dc63efe3) tests: keep the options of the reconnect test's nodes alive
- [`aa8271d8`](https://github.com/carbon-witness/graphene-core/commit/aa8271d8cc164b9f603812919af4df756d19d68e) tests: reconnect test with an earlier connection that is not closed
- [`c03b3829`](https://github.com/carbon-witness/graphene-core/commit/c03b3829a019777da0658988090634ad41773281) net: the outbound connect retry comment names the actual cause
- [`a1542d1b`](https://github.com/carbon-witness/graphene-core/commit/a1542d1b720fd77b51c352da29d9a3b6d6151415) net: TLS for P2P connections on the same port
- [`9e2f20c7`](https://github.com/carbon-witness/graphene-core/commit/9e2f20c7d36f6b9f5f0fb04d5f02f1140559999d) tests: P2P TLS between new and old nodes, seed node addresses
- [`4e0767da`](https://github.com/carbon-witness/graphene-core/commit/4e0767da7a02f32a90ed37a82d09dcfd2c75febf) docker: run the node in the host network by default
- [`1adc8b53`](https://github.com/carbon-witness/graphene-core/commit/1adc8b53345567270c4ce8fea1f90edac3e633c3) fc: addresses, sockets and resolving for IPv6
- [`8d4f8191`](https://github.com/carbon-witness/graphene-core/commit/8d4f81914814c81c2956ccd8f115b340bedcdc1a) net: carry addresses as any_endpoint, IPv4 behaviour unchanged
- [`65a414c3`](https://github.com/carbon-witness/graphene-core/commit/65a414c3041d44b273dace0e21459ee1ba2f29c4) p2p: seed nodes and peer records for IPv6 in the 1.2.2 changes
- [`62573f02`](https://github.com/carbon-witness/graphene-core/commit/62573f02dd17d77b38886ea20295a4769c83c647) p2p: listen on IPv6, tell peers the endpoints to connect to
- [`13fac1b3`](https://github.com/carbon-witness/graphene-core/commit/13fac1b36522163e6e5b83bd3ff5b108460eb883) p2p: prefer IPv6 for a node with both, stop trying IPv6 when it does not work
- [`948212eb`](https://github.com/carbon-witness/graphene-core/commit/948212ebfc2fbf0efbf34af3e1cce24f24b1e9a1) p2p: address_v2 and check_firewall_v2 for IPv6 endpoints
- [`5ca52619`](https://github.com/carbon-witness/graphene-core/commit/5ca52619cb0e9cb1b44242f4acb9362d54f466fd) p2p: options for listening and advertising, IPv6 in network info
- [`909be34b`](https://github.com/carbon-witness/graphene-core/commit/909be34bfd2051e8ad39d16079bd03c63d822468) p2p: IPv6 peers on a local network or the loopback without a global address
- [`01177e73`](https://github.com/carbon-witness/graphene-core/commit/01177e7359d394f64f430964362ffac58505119f) p2p: cancel the accept loops before closing their servers
- [`676be98c`](https://github.com/carbon-witness/graphene-core/commit/676be98c5e6b5fd8cf246590799135e379e9707d) ci: push the image of a release branch to ghcr.io for test nodes
- [`6227dd25`](https://github.com/carbon-witness/graphene-core/commit/6227dd25c60c04258bb184ada070494ee147c0fc) p2p: the IPv4 firewall check only over an IPv4 connection
- [`15a43331`](https://github.com/carbon-witness/graphene-core/commit/15a433318212be91977f220a7b2a64aadecced3e) p2p: no 0.0.0.0:0 for old nodes in address_message
- [`6e82b10b`](https://github.com/carbon-witness/graphene-core/commit/6e82b10ba26e0559943410fb9c31b2d309e16ae6) p2p: no loopback endpoints in the addresses for a peer elsewhere
- [`3c091a08`](https://github.com/carbon-witness/graphene-core/commit/3c091a08b63fb673a6ad559f289ea8e73d3db4be) p2p: an inbound IPv6 connection from the internet shows our endpoint can be reached
- [`1bbfd7bc`](https://github.com/carbon-witness/graphene-core/commit/1bbfd7bceb864935cb259b5f941091d7e03f0286) Version 1.2.2
- [`cf374600`](https://github.com/carbon-witness/graphene-core/commit/cf37460062cbd9105c2166e5bf38107ad765f796) docs: release notes for 1.2.2 (draft)
- [`1f06e1af`](https://github.com/carbon-witness/graphene-core/commit/1f06e1afffbbeecaadccba12e254b8fe661a09c7) docs: README for 1.2.2, with a section on the P2P network

**graphene-fc**
- [`57dc6c6`](https://github.com/carbon-witness/graphene-fc/commit/57dc6c638bef576040cb9a90cff1093677415b7c) ip: addresses and endpoints of either family, IPv4 or IPv6
- [`0d1c824`](https://github.com/carbon-witness/graphene-fc/commit/0d1c82439253bfdec5965218ca116e66383ce1e1) tcp: sockets, listening and resolving for IPv6
- [`475b778`](https://github.com/carbon-witness/graphene-fc/commit/475b7781840e7347e5ebb88059bf748488fcf07e) network: the local address the system would use towards a host
