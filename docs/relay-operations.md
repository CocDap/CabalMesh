# Running the relay

**Ticket 23.** The relay is what makes the mesh larger than a room. Without one, mDNS is the whole of discovery and two users on different networks never meet.

This document is both the runbook and the honest account of what the relay costs. The section on privacy is written to be quotable in what the app says about itself, not to be reassuring.

---

## What the relay costs

Two things, and neither is engineered away.

**It is a single point of failure for off-LAN discovery.** When it is down, peers on the same network still find each other and the app still works; peers on different networks do not meet at all. Nothing degrades gracefully across that line, because there is no other mechanism.

**It observes which peer identifiers are online together.** Every reservation tells the relay that a peer exists, when it connected, and from what address. Every circuit tells it that two specific peers are talking. For a product whose thesis is zero identity, that is a real surface, and it is worth stating exactly how far it goes:

- The relay **cannot read intents**. Noise encrypts every hop end to end; the relay carries ciphertext and has no key.
- The relay **does** learn peer identifiers, IP addresses, connection times, and who is connected to whom.
- Peer identifiers are ephemeral — regenerated each launch — so they do not link sessions to each other. IP addresses are not ephemeral and do.
- Successful hole punching removes the relay from the path after the handshake, so it sees that two peers met but not how long they talked. Where hole punching fails, the whole conversation stays relayed.

The app should say this plainly rather than describing itself as though the relay were not there. "No identity is attached" remains true of what the relay can read. It is not true of what it can observe about connection patterns, and the difference matters.

Mitigations available to an operator, none of which are a substitute for the disclosure: run the relay with logging at `info` (the default in the unit file) so per-connection identify lines are not journalled; do not enable access logging in anything in front of it; and do not put it behind a CDN or reverse proxy that keeps its own logs.

---

## Provisioning

### 1. Build

```sh
cargo build --release -p cabal-relay
install -m 0755 target/release/cabal-relay /usr/local/bin/cabal-relay
```

The relay lives in the app's own workspace, and that is deliberate: it pins the same libp2p version the phone uses. A relay on a different protocol revision does not fail loudly — it refuses reservations, and from the phone that is indistinguishable from the relay being unreachable.

### 2. Create the account and directory

```sh
useradd --system --no-create-home --shell /usr/sbin/nologin cabal-relay
install -d -o cabal-relay -g cabal-relay -m 0750 /etc/cabal-relay
```

### 3. Generate the identity — once, ever

```sh
sudo -u cabal-relay cabal-relay --generate-key /etc/cabal-relay/relay.key
```

It prints the peer id on stdout. **Write it down and back the key file up off-host before going any further.**

This key is load-bearing in a way most keys are not. Its peer id is compiled into every shipped build as part of the bootstrap address:

- **Rotating it** strands every installed app until those users update.
- **Losing it** strands them permanently — there is no way to reconstruct it.

Treat it like a signing key. The binary helps: it refuses to overwrite an existing key, refuses to start when the key is missing rather than generating a fresh one, and writes the file `0600`.

> A relay that silently generates a new identity on a missing key file is the worst failure available here. It starts cleanly, logs nothing alarming, and every phone in the world quietly stops being able to reserve.

### 4. Open the firewall

Both transports, both protocols. TCP **and** UDP on 4001:

```sh
ufw allow 4001/tcp
ufw allow 4001/udp
```

The app dials both. A relay reachable on only one silently halves the peers that can use it. QUIC is UDP — a firewall rule that opens "port 4001" meaning TCP only is a common and quiet way to lose half the mesh.

**No NAT in front.** The relay must be directly reachable. If the host's public address differs from what it binds — a 1:1 NAT, or a DNS name — pass `--announce` explicitly:

```sh
cabal-relay --key /etc/cabal-relay/relay.key \
  --announce /ip4/203.0.113.7/tcp/4001 \
  --announce /ip4/203.0.113.7/udp/4001/quic-v1
```

Without `--announce` the relay advertises the non-loopback addresses it actually bound.

### 5. Install the unit

```sh
install -m 0644 deploy/relay/cabal-relay.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now cabal-relay
journalctl -u cabal-relay -f
```

Startup logs the peer id, the limits, and one `listening:` line per bound address — each already in the form the app's bootstrap list wants.

### 6. Compile the address in

Take a `listening:` line for the host's public address and add it to `default_relays()` in `src-tauri/src/bootstrap_config.rs`:

```rust
pub fn default_relays() -> Vec<String> {
    vec![
        "/ip4/203.0.113.7/udp/4001/quic-v1/p2p/12D3KooW…".into(),
        "/ip4/203.0.113.7/tcp/4001/p2p/12D3KooW…".into(),
    ]
}
```

`no_placeholder_relay_ships` in that file's tests asserts the list is empty. **Deleting that test is part of this step** — it exists to stop a made-up address shipping before a real relay exists, and once one does its job is done.

Ship both transports. QUIC first: it survives the Wi-Fi-to-cellular handoff a phone does constantly, and TCP is the fallback for networks that block UDP.

---

## Limits

Set in `crates/cabal-relay/src/limits.rs`, with the reasoning beside each. Summarised:

| Limit | Value | Why |
|---|---|---|
| `max_reservations` | 512 | Each is a held connection; a few thousand is where file descriptors matter more than bandwidth |
| `max_reservations_per_peer` | 4 | Phone, tablet, laptop, plus one for a reconnect racing an expiry |
| `reservation_duration` | 1 h | Long enough that a phone is not spending battery renewing; short enough that a vanished device frees its slot the same day |
| `max_circuits` | 256 | |
| `max_circuits_per_peer` | 8 | |
| `max_circuit_duration` | 10 min | A circuit should carry a handshake and be replaced by a direct connection. Still open after ten minutes means the hole punch failed or someone is tunnelling |
| `max_circuit_bytes` | 8 MiB | An intent and its negotiation are kilobytes. Three orders of magnitude of headroom, still too small to move a file |

**An unbounded relay is an open proxy** — anyone can reserve, push traffic at the operator's bandwidth cost, with the operator's IP as the apparent source. libp2p's defaults are written for a general audience and are not these.

The per-peer caps are what make the global ones safe. A global cap alone is a denial-of-service surface: one peer taking the global maximum locks everyone else out. `no_single_peer_can_take_the_whole_pool` asserts the ratio holds.

---

## Version pinning

Recorded, because "the relay stopped working" and "the relay is running a different protocol revision" look identical from a phone:

| Component | Version |
|---|---|
| libp2p | 0.54 |
| libp2p-swarm | 0.45 |
| Protocol name | `/cabalmesh/1.0.0` |

The first two are pinned by the workspace lockfile — the relay is a member of the app's workspace, so they cannot drift apart without a lockfile change. The protocol name is in `crates/cabal-relay/src/main.rs`. Bump it when the wire format changes, so a mismatch shows up in the identify exchange instead of as unexplained reservation failures.

---

## Verifying it works

`crates/cabal-relay/tests/reservation.rs` runs a real client against a real relay carrying the shipped limits, and asserts the reservation is accepted.

That test earned its place immediately: the first version of the relay **never advertised an external address**, so every reservation was accepted by the relay and then rejected by the client with `NoAddressesInReservation` — after a successful dial, a successful connection, and no error at all on the relay side. It looked like it worked.

On a live relay:

```
INFO cabal_relay: reservation accepted peer=12D3KooW…
INFO cabal_relay: reservation denied — at limit peer=12D3KooW…
```

The denial line is at `info` on purpose: a relay at its limits looks exactly like an unreachable one from the phone, and this is the only place the difference is visible.

---

## Still to do on a real host

Everything above is code and configuration, and all of it is committed. What is not done, because it needs a host this was written on:

- [ ] Relay running on a reachable host with both transports open and no NAT in front
- [ ] Identity generated **on that host** and backed up off-host
- [ ] Its address compiled into `default_relays()`, and `no_placeholder_relay_ships` deleted
- [ ] Two devices on different networks connect through it and upgrade to direct where possible

The last one is the only acceptance criterion that cannot be met without two networks. Reservation is covered by the test; the direct upgrade after it is DCUtR, which needs real NATs on both sides to mean anything.
