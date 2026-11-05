# BLE mesh — the offline core

**Design.** What the app does when there is no network at all, and how the relay becomes the way out to the internet rather than the way peers find each other.

---

## 1. What is wrong today

The README says "physical Mesh Network" and "offline peer-to-peer communication". The code says something narrower. `MeshNetwork::new` builds a libp2p swarm over TCP and QUIC, discovers with mDNS, and dials bootstrap relays — `src-tauri/src/mesh.rs:185-214`. Every one of those needs an IP network. mDNS needs a shared LAN. The relay needs internet.

So two phones in the same room with no Wi-Fi and no cell service cannot see each other. For a product whose first claim is offline mesh, that is not a missing feature; it is the missing product.

`BootstrapConfig::default_relays()` returns an empty list — `src-tauri/src/bootstrap_config.rs:48` — so today there is not even the internet path. The app is LAN-only and says so honestly, which was the right call while nothing better existed.

This design adds the thing that was always supposed to be there.

## 2. The shape: two planes and one bridge

```
        ┌──────────────────────── BLE plane ────────────────────────┐
        │  no infrastructure. no IP. no internet.                   │
        │                                                           │
        │   phone A ◄──L2CAP──► phone B ◄──L2CAP──► phone C         │
        │      flood TTL 7 · dedup · Noise XX per link              │
        └───────────────────────────┬───────────────────────────────┘
                                    │
                             bridge │ (gateway nodes only)
                                    │
        ┌───────────────────────────┴───────────────────────────────┐
        │  IP plane — the existing libp2p swarm                     │
        │  gossipsub · mDNS · circuit relay · dcutr · QUIC/TCP      │
        │                                                           │
        │                    relay host ──► NEAR RPC                │
        └───────────────────────────────────────────────────────────┘
```

**The BLE plane is the core.** It carries intents between people standing near each other, and it works with the radios turned off on everything else. It does not use libp2p. It is its own protocol, because libp2p has no BLE transport and building one would mean carrying the whole stack's assumptions — multiaddrs, dialling, connection upgrades — onto a link that has none of them.

**The IP plane is unchanged.** Everything in `mesh.rs` stays exactly as it is. gossipsub, mDNS, relay client, dcutr. It is now understood as the *internet* plane rather than the mesh: the way a node reaches the chain and reaches peers who are far away.

**The relay is a gateway, not a directory.** Its job in this design is what a relay is actually good at — carrying traffic to the internet for a node that cannot get there itself. It stops being the only answer to "how do two users on different networks meet", because for two users in the same room the answer is now the radio in their pocket.

## 3. Why not a libp2p transport over BLE

It was the first thing considered and it is the wrong trade here.

Making BLE a `libp2p::Transport` would let gossipsub, Noise and yamux run over it unchanged, which is genuinely attractive — `mesh.rs` would gain one line. But gossipsub's mesh maintenance assumes a link budget BLE does not have: it heartbeats every second (`mesh.rs:149`), grafts and prunes toward a target degree, and gossips message IDs it has seen to peers that might not have them. On a link that carries 100–300 kbit/s shared across every neighbour, protocol overhead designed for a datacentre link becomes the traffic.

A standalone mesh lets every parameter be chosen for the radio: flood instead of gossip, one announce every 15–30 seconds instead of a heartbeat every second, a fanout subset instead of a maintained mesh degree.

The cost is real and should be stated: **we write and own a routing layer, a deduplication layer, a fragment-free framing layer and a session layer that libp2p would have given us.** Section 9 is how that cost is kept payable.

## 4. Link layer

### 4.1 Both roles, always

Every device runs as GATT **peripheral** and GATT **central** at the same time. There is no client and server; a node that only scanned would never be found, and a node that only advertised would never find anyone. This is the one structural thing bitchat gets unambiguously right and there is no reason to differ.

### 4.2 Discovery, then a real stream

BLE advertising has room for a service UUID and very little else, so discovery happens in two stages.

**Stage 1 — GATT, for rendezvous only.**

```
service        <CabalMesh service UUID, one constant per network>
characteristic PSM        read    2 bytes   L2CAP PSM this node is listening on
characteristic EPHEMERAL  read    64 bytes  this session's X25519 key ‖ Ed25519 key
```

A central scans filtered on the service UUID, connects, reads two characteristics, and disconnects. Nothing else ever crosses GATT.

**Stage 2 — L2CAP CoC, for everything.**

The central opens an L2CAP connection-oriented channel to the advertised PSM. `CBL2CAPChannel` on iOS gives an `InputStream`/`OutputStream` pair; `BluetoothDevice.createL2capChannel()` on Android gives a `BluetoothSocket`. Both are reliable, ordered, flow-controlled byte streams with SDUs up to 64 KiB.

**That is the reason for the OS floor**, and it deletes an entire subsystem. bitchat fragments every packet into ~469-byte pieces with an 8-byte fragment ID, and maintains 128 concurrent reassembly buffers with a 30-second timeout and a 1 MiB cap. Over L2CAP, none of that exists: frames are length-prefixed on an ordered stream, and a dropped link drops a partial frame with no state to clean up.

The floor this buys costs `minSdkVersion` 24 → **29** (Android 10, September 2019). iOS is already at 14.0 in `tauri.conf.json` and `CBL2CAPChannel` has existed since iOS 11.

### 4.3 Frame

```
┌────────┬───────────────────────────────────┐
│ len    │ packet                            │
│ u32 BE │ len bytes                         │
└────────┴───────────────────────────────────┘
```

Length-prefixed, big-endian, `len ≤ 65_629` — the largest a packet can be, given a `u16` payload length plus at most 94 bytes of header, identifiers and signature. A frame claiming more is refused **before allocating**, which is the point: the cap is not the radio's, it is a bound on what one peer in radio range can make another allocate.

## 5. Packet

Network byte order throughout.

```
┌─────────┬──────┬─────┬───────────┬───────┬────────────┐
│ version │ type │ TTL │ timestamp │ flags │ payload_len│
│  u8     │  u8  │ u8  │  u64 BE   │  u8   │   u16 BE   │
└─────────┴──────┴─────┴───────────┴───────┴────────────┘
┌──────────┬─────────────┬─────────┬───────────┐
│ sender   │ recipient   │ payload │ signature │
│ 8 bytes  │ 8 bytes  *  │ n bytes │ 64 bytes *│
└──────────┴─────────────┴─────────┴───────────┘
* present per flags

flags: bit0 has_recipient   bit1 has_signature   bit2 compressed (zstd)
       bits 3-7 reserved, must be zero, refused if not
```

Header is 14 bytes. Minimum packet 22 bytes.

**The signature covers every byte except the TTL.** Relays decrement TTL in place; excluding that one byte is what lets them do it without invalidating the signature or re-signing. Reserved flag bits are *refused* rather than ignored, so a future version cannot be silently mis-parsed by an old node.

**Compression is defined and not implemented.** Bit 2 has its meaning fixed so a later version can compress without a version bump, and this build *refuses* a packet that sets it rather than ignoring the flag. The compressor was dropped from v1 on the grounds that it is a dependency and an untested code path in exchange for saving bytes on payloads that are already kilobytes; `wire.rs` refuses it with `WireError::CompressionUnsupported` and a test pins that behaviour.

### 5.1 Types

| Type | Name | Meaning |
|---|---|---|
| `0x01` | `announce` | signed presence + ephemeral key + neighbour list |
| `0x02` | `handshake` | Noise XX message |
| `0x03` | `sealed` | ciphertext inside an established session |
| `0x10` | `intent` | a `PrivacyIntent`, flooded to the whole mesh |
| `0x11` | `intent_ack` | delivery acknowledgement, directed |
| `0x20` | `gateway_request` | "someone with internet, please carry this" |
| `0x21` | `gateway_result` | outcome, flooded back |

`intent` deliberately mirrors the existing gossipsub payload — `PrivacyIntent` in `mesh.rs:85-92` — so the bridge in section 7 is a re-encode, not a translation.

## 6. Identity — where this departs from bitchat

bitchat's whitepaper names its own weakest point plainly: the 8-byte sender ID in every packet header derives from a key that never rotates, and announcements broadcast the static public key and nickname in cleartext. A passive listener enumerates who is present and follows a device across places and days.

For an app whose first sentence is "In this network, you are a **Nobody**", shipping that would contradict the product.

**So the durable identity never appears in cleartext.**

- Each app launch generates **two ephemeral keypairs** — X25519 for key agreement, Ed25519 for signing — exactly as the IP plane already generates a fresh identity at `mesh.rs:136`. The 8-byte sender ID is the first 8 bytes of `SHA-256(ephemeral_x25519_public)`. Both die when the app does.
- `announce` carries both ephemeral public keys and is signed by the ephemeral Ed25519 key. It carries no nickname and no wallet address.
- The **durable** identity — the account address the `presence` intent already publishes at `mesh.rs:372-384` — is sent only as a `sealed` payload after a Noise XX handshake has completed. A passive radio listener sees an unfamiliar 8-byte tag and ciphertext.

What this costs, said plainly:

- **No reconnect memory across launches.** A peer you traded with yesterday is a stranger today until you handshake again. Counterparty continuity has to come from the durable identity exchanged inside the session, not from the radio.
- **The service UUID is still a fingerprint.** Anyone scanning knows a CabalMesh node is present, just not which one. That is inherent to being discoverable and is not engineered away.
- **Payload length is observable.** `sealed` and `handshake` packets are padded to 256/512/1024/2048-byte buckets; `announce`, `intent` and gateway packets go out at natural length. Traffic volume and shape leak even when content does not.

These belong in `docs/relay-operations.md`'s sibling section on what the app says about itself.

## 7. Routing

Flood with suppression. The parameters below are bitchat's, adopted rather than re-derived: they are the numbers a 34.9k-star deployment converged on, and inventing different ones without a testbed of comparable size would be worse.

- **TTL 7** at origin. Dense neighbourhoods (≥6 links) clamp broadcast TTL to 5; sparse chains (≤2 links) relay at full incoming depth.
- **Jitter 10–220 ms** before relaying, widening with degree. A duplicate arriving during that window **cancels the send to the link it came from** — and only that link. This is what makes flooding survivable, not the TTL.

  Cancelling the *whole* relay is the obvious implementation and it is wrong. A duplicate proves one neighbour has the packet; it proves nothing about the others. In a line of twenty nodes, a node hearing the same intent from both sides dropped its forward to the chain's far end, and five nodes never saw it. The twenty-node simulation caught it on its first run.

- **Fanout subset, restricted to links somebody else also covers.** A broadcast re-transmits to a deterministic subset of links, seeded by message ID, of size ≈ log₂(degree), with the ingress link always excluded. `announce` and gateway traffic use full fanout.

  A link may only be thinned if the peer behind it appears in another neighbour's advertised neighbour list. Thinning a link nobody else covers is not a saving, it is a partition: in a star, the hub is the sole path to every leaf, and unrestricted subsetting silently dropped a third of them. In a crowded room almost every link is covered several times over and most of the fanout can go, which is where the saving was supposed to come from anyway.
- **Dedup**: LRU of 1000 entries keyed by `(sender, timestamp, type, SHA-256(payload)[..16])`, retained 5 minutes. SHA-256 rather than a faster hash because it is already in the dependency tree — an extra hash crate costs binary size on a platform where `scripts/ios-size.sh` and `scripts/android-size.sh` exist to watch it.
- **Directed** packets (`handshake`, `sealed`, `intent_ack`) relay at TTL−1 with tight jitter along the neighbour that announced the destination.

**Announce cadence**: every 4 s while no peer is connected, backing off to 15–30 s once connected. Each announce carries up to 10 direct-neighbour IDs, which is how a node learns topology two hops out without a routing protocol. A peer stays reachable for 60 s after its last verified announce.

Two things the neighbour list must exclude, both found by running rather than by reading:

- **The announcer itself**, which is obvious.
- **The receiving node.** Every neighbour lists *us* among its neighbours, so a table that does not exclude itself reports a two-node mesh as three: the other node at one hop, and ourselves at two. Two real nodes printed `reachable=2` where one was correct. It is the kind of error that makes the mesh look like it is working better than it is.

A relayed announcement is recorded at two hops rather than one. "Four nodes are in the room with you" and "four nodes exist somewhere in the mesh" are different claims, and only the first is about a radio — the nodes screen shows both, separately.

## 8. The bridge, and the relay as gateway

This is the second half of the goal: the relay is where peers go to reach the internet.

**A gateway is a node that has both planes up.** It advertises a capability bit in its `announce`. Nothing is special about it otherwise — no registration, no trust, no reward beyond what Relay Mode already grants at `src/App.tsx:258-268`.

**BLE → internet.** A node with no connectivity signs its `token_diff` locally and holds it in `near_diffs.json`. It floods the `SignedDiff` on the BLE plane as well as on gossipsub, where nobody offline can hear it. Any gateway that receives it and has Relay Mode on submits the raw signed transaction and floods `gateway_result` back. The origin matches on `queue_id` and marks its own queue entry, exactly as `App.tsx:279+` does today.

The payload is an already-signed transaction. It never contains a key. That property is what makes an untrusted gateway acceptable, and it is already true of the existing `relay_tx` flow.

**Internet → BLE, deliberately narrow.** The bridge does *not* republish gossipsub traffic onto BLE. The IP plane can carry far more than 100–300 kbit/s shared across a room, and mirroring it would saturate the radio and the battery. Only two things cross inward:

1. `gateway_result` for a request that came from BLE.
2. An `intent` addressed to a peer the gateway knows is BLE-reachable.

**Loop prevention.** Every packet that crosses a plane keeps its original message ID, and both planes' dedup sets are consulted before a bridge re-emits. A packet that came from BLE is never sent back to BLE by the same node.

## 9. Code shape

Four units. The dependency arrow points inward, matching the existing rule in `src-tauri/Cargo.toml`: extracted crates go in `crates/` and must not depend on the app package.

### `crates/cabal-ble` — the protocol, with no I/O

Pure. No async, no radio, no clock of its own. A state machine:

```rust
pub enum Event { LinkUp(LinkId, PeerKey), LinkDown(LinkId),
                 Frame(LinkId, Vec<u8>), Submit(Intent), Timer(TimerId) }

pub enum Action { Send(LinkId, Vec<u8>), Schedule(TimerId, Duration),
                  Deliver(Intent), Connect(PeerId), Drop(LinkId) }

impl Engine {
    pub fn handle(&mut self, event: Event, now: Instant) -> Vec<Action>;
}
```

Modules: `wire` (encode/decode), `router` (TTL, dedup, fanout, jitter), `session` (Noise XX), `peers` (announce, reachability, neighbour table).

**This shape is the whole testing strategy.** Time is a parameter and the radio is a return value, so a twenty-node mesh with a virtual clock runs deterministically in a unit test on a laptop. Given that this machine cannot tap an iOS simulator — no `idb`, and `osascript` is refused assistive access — a protocol that can only be exercised on hardware would be a protocol that is never exercised.

### `src-tauri/src/ble/` — the runtime

Owns a tokio task, drives the engine, executes its actions against a `BleLink` trait, and converts platform callbacks into engine events. Mirrors the existing actor pattern in `mesh_handle.rs`: a bounded command channel, requests carrying their own reply channel, `Swarm`-style single ownership.

`BleLink` has two implementations: the Tauri mobile plugin, and a TCP-loopback fake that lets three real app processes talk "BLE" on a desktop.

### `crates/cabal-ble-macos` — the radio, on Apple platforms

Written. CoreBluetooth through `objc2`, and the **only crate in the workspace that does not inherit `unsafe_code = "forbid"`** — isolating the unsafe in a crate that does nothing else is the point.

One delegate object implements all three CoreBluetooth protocols and owns every Objective-C pointer. It is created on a serial dispatch queue, never leaves it, and CoreBluetooth delivers its callbacks there. A repeating block on the same queue moves bytes between the L2CAP streams and a mutex over plain `Vec<u8>` — which is all the Rust side ever touches. No `Retained` pointer crosses a thread boundary, so the crate needs no `Send`/`Sync` argument about `CBPeripheral` at all.

Two details that are load-bearing and were not obvious:

- **A peripheral being connected to must be retained.** CoreBluetooth does not hold a strong reference during connection; drop it and the attempt is abandoned with no callback and no error. It presents as "discovery works but nothing ever connects".
- **CoreBluetooth holds its delegate weakly.** Nothing here can own it — it may not leave the queue and the queue has no storage — so it is leaked deliberately, one object for the life of the process. A dropped delegate is a radio that starts, logs nothing, and never calls back.

Verification status is in `docs/mobile-build-verification.md` and is short: the delegate is reached and reports state; the machine it was run on has Bluetooth switched off, so everything past `PoweredOn` is unverified.

### `tauri-plugin-cabal-ble` — iOS and Android

Not written. Swift and Kotlin, per Tauri 2's mobile plugin model, with the same responsibilities as the macOS crate and no more: advertise, scan, expose the two GATT characteristics, open and accept L2CAP channels, pump bytes, report link up/down. No protocol logic — a bug in routing must be fixable in Rust with a test, not in Swift with two phones.

### Integration into the existing app

- `MeshCommand` gains `BleSnapshot`; `MeshSnapshot` gains BLE peer count and link count.
- `MeshEvent` gains `BlePeerDiscovered` / `BlePeerLost`, so the nodes screen shows radio peers beside network peers and can say which is which.
- `set_offline` gates **both** planes. The kill switch promises nothing leaves the device; a BLE radio still flooding would make that promise false.

## 10. Failure, and what it must not do

| Situation | Behaviour |
|---|---|
| Bluetooth off, or permission refused | BLE plane disabled, IP plane unaffected, UI says which. Follows the `Toggle` precedent for mDNS at `mesh.rs:167-176` — a refused permission must never prevent the node starting. |
| L2CAP unsupported on the peer | That peer is skipped and logged once. No GATT fallback: two code paths neither of which can be tested on a simulator is worse than one that sometimes declines. |
| Link drops mid-frame | Partial frame discarded, link torn down, peer marked unreachable after its 60 s window. No reassembly state to leak. |
| Handshake failure | Exponential backoff per peer, capped. Repeated failures from one peer are rate-limited before they cost battery. |
| Malformed packet | Dropped, counted, logged at debug with a rate limiter. Never panics — this is attacker-controlled input arriving over the air from anyone in range. |
| Frame claims >64 KiB | Refused before allocation. |
| Dedup set full | LRU eviction. A full set degrades to more duplicate relays, never to unbounded memory. |

## 11. Platform declarations

**iOS** — `src-tauri/Info.plist`, which is merged into the generated plist and lives outside `gen/apple/` precisely because that tree is regenerated:

```xml
<key>NSBluetoothAlwaysUsageDescription</key>
<string>CabalMesh finds nearby nodes over Bluetooth when no network is available. No identity is attached.</string>
<key>UIBackgroundModes</key>
<array>
  <string>bluetooth-central</string>
  <string>bluetooth-peripheral</string>
</array>
```

Background advertising on iOS moves the service UUID into the advertisement's overflow area, where it is discoverable only by an iOS device explicitly scanning for that exact UUID. Backgrounded iOS-to-iOS discovery therefore works; a backgrounded iOS node is invisible to a scanning Android node. This is a platform property, not a bug to fix, and the UI should not imply otherwise.

**Android** — `AndroidManifest.xml`:

```xml
<uses-permission android:name="android.permission.BLUETOOTH_SCAN"
                 android:usesPermissionFlags="neverForLocation" />
<uses-permission android:name="android.permission.BLUETOOTH_ADVERTISE" />
<uses-permission android:name="android.permission.BLUETOOTH_CONNECT" />
<uses-feature android:name="android.hardware.bluetooth_le" android:required="true" />
```

`neverForLocation` is what avoids requesting location permission for a scan on API 31+. Without it the app asks for location to find a peer, which is both a worse prompt and a worse claim.

`minSdkVersion` moves 24 → 29 in `tauri.conf.json` and `gen/android/app/build.gradle.kts`.

## 12. Battery

- Scanning is duty-cycled, not continuous, and gated on RSSI so a peer at the edge of range does not cause a connect/drop loop.
- Announce backs off from 4 s to 15–30 s once connected.
- The radio stops entirely when `set_offline(true)`.
- An L2CAP link with no traffic for 10 minutes is closed. Reopening costs one GATT read.

## 13. Testing

Stated precisely, because the verification limits here are real and a design that pretends otherwise produces claims nobody checked.

**Proven by deterministic test, on a laptop, in CI:**

- wire round-trip, including every flag combination and refusal of reserved bits — property tests, matching `crates/cabal-core/tests/properties.rs`
- flood convergence: N nodes, virtual clock, virtual links — every node receives every intent
- dedup: a packet injected on k links relays at most once
- TTL exhaustion: a chain longer than 7 does not deliver, and does not loop
- cancellation: a relay scheduled behind jitter is cancelled by an earlier duplicate
- partition healing: two components merged by one new link converge
- Noise XX handshake, and refusal of a `sealed` packet with no session
- signature verification, and specifically that a TTL decrement does **not** invalidate it

**Proven by three processes on one desktop, over the TCP-loopback `BleLink`:**

- runtime wiring: engine actions actually reach a link and come back as events
- an intent composed in one app instance appears in another
- the gateway path end to end against NEAR testnet

**Proven only on two physical devices, manually:**

- advertising and scanning
- L2CAP PSM exchange and channel open
- iOS↔Android interop
- background behaviour
- battery cost

That last group cannot be automated here and must be recorded as such in `docs/mobile-build-verification.md`. A screenshot of a screen does not imply its interactions were checked, and a passing test suite does not imply a radio was ever switched on.

## 14. Order of work

1. `cabal-ble`: wire format + property tests. No radio, no runtime.
2. `cabal-ble`: router — flood, dedup, TTL, jitter, fanout — with the virtual-clock multi-node harness.
3. `cabal-ble`: Noise XX session, ephemeral identity, announce.
4. `BleLink` trait + TCP-loopback implementation. Three desktop processes exchanging intents.
5. Tauri plugin: Android first — Kotlin, and an emulator can at least be driven.
6. Tauri plugin: iOS — Swift. Two physical devices from here on.
7. Bridge + gateway path. Reuses `relay_tx` / `relay_confirmed` wholesale.
8. UI: BLE peers on the nodes screen, distinguished from network peers.

Steps 1–4 are the majority of the protocol and none of them need a radio. That ordering is deliberate.

## 15. What this does not do

- **No internet-scale routing.** The BLE plane is a room, a building, a queue of people. Reaching someone far away is the IP plane's job, and the relay's.
- **No store-and-forward courier.** bitchat carries sealed mail for offline recipients with copy budgets and daily-rotating tags. Useful, and a separate design; the ephemeral identity in section 6 makes it harder, since there is no stable key to seal to. Out of scope here.
- **No voice, no files.** `intent` payloads are kilobytes. The frame cap is 64 KiB and the design assumes nothing approaches it.
- **No GATT fallback path.** Section 10 says why.
- **It does not make the relay unnecessary.** The relay is how a signed diff reaches a gateway across the internet, and `BootstrapConfig::default_relays()` being empty is still the open item it was — see `docs/relay-operations.md` §6.
