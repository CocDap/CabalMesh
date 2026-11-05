//! The CabalMesh BLE mesh protocol.
//!
//! # What this crate is
//!
//! The offline core: what the app does when there is no Wi-Fi, no cell
//! service, and no infrastructure of any kind — two phones in a room, talking
//! over the radio in the user's pocket.
//!
//! It is deliberately **not** libp2p. gossipsub maintains a mesh with a
//! one-second heartbeat, grafts and prunes toward a target degree, and gossips
//! message identifiers to peers that might be missing them. On a link carrying
//! 100–300 kbit/s shared across a room, that maintenance is the traffic. So
//! routing here is a flood with four suppressions, sized for the radio. See
//! [`router`].
//!
//! The existing libp2p swarm in `src/mesh.rs` is untouched and keeps its job:
//! reaching the internet, and reaching the chain through the relay.
//!
//! # The constraint that shapes everything
//!
//! **No I/O.** No `tokio`, no `tauri`, no sockets, no clock. Time arrives as a
//! parameter; the radio leaves as a return value. See [`engine`].
//!
//! Bluetooth cannot be driven from CI and cannot be driven from an iOS
//! simulator at all. A protocol testable only on two physical phones would be
//! a protocol tested roughly never. Written this way, a twenty-node mesh with
//! a virtual clock is an ordinary unit test that runs in milliseconds on every
//! commit, and the radio-shaped remainder is small enough to read.
//!
//! # Modules
//!
//! - [`wire`] — the packet, decoded from bytes an unauthenticated stranger
//!   sent over the air.
//! - [`framing`] — packet boundaries on an L2CAP stream. No fragmentation
//!   layer, and [`framing`] says why that is the payoff for the OS floor.
//! - [`identity`] — this session's keys, which never touch disk.
//! - [`peers`] — who is out there, and what an announcement deliberately omits.
//! - [`router`] — flood, deduplicate, clamp, jitter, thin.
//! - [`service`] — the identifiers every node must agree on, whatever radio
//!   it is using.
//! - [`engine`] — all of the above as one state machine.

#![forbid(unsafe_code)]

pub mod engine;
pub mod framing;
pub mod identity;
pub mod peers;
pub mod router;
pub mod service;
pub mod wire;

pub use engine::{Action, Engine, Event, MeshStatus};
pub use identity::Ephemeral;
pub use peers::{Announce, Capabilities, KnownPeer, PeerTable};
pub use router::{Decision, LinkId, Router};
pub use service::{is_valid_uuid, PSM_UUID, SERVICE_UUID};
pub use wire::{DedupKey, Packet, PacketKind, PeerId, WireError};
