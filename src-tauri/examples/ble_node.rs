//! A BLE-plane node with no app around it.
//!
//! # What it is for
//!
//! Two things that are otherwise hard:
//!
//! 1. **A second device.** Verifying the mesh needs two nodes, and one of them
//!    does not have to be a phone. Run this on a laptop, point the app at it,
//!    and the app has a real peer running the real protocol — announcements,
//!    flooding, deduplication, the lot.
//!
//! 2. **Seeing what the mesh is doing**, in a terminal, without a UI in the
//!    way. When the nodes screen shows nothing, this says whether the problem
//!    is the mesh or the screen.
//!
//! ```sh
//! cargo run -p cabalmesh --example ble_node -- listen=0.0.0.0:9701
//! cargo run -p cabalmesh --example ble_node -- listen=0.0.0.0:9702,dial=127.0.0.1:9701
//! ```
//!
//! The transport is the loopback one, so this is the protocol over TCP rather
//! than over a radio. It is labelled `loopback` everywhere it surfaces, for
//! the same reason the nodes screen prints the transport verbatim: calling it
//! Bluetooth would claim a capability it does not have.

use cabal_ble::peers::Capabilities;
use cabal_ble::wire::PacketKind;
use cabalmesh_lib::ble::{self, link::LoopbackTransport, BleEvent};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,cabalmesh_lib::ble=debug".into()),
        )
        .init();

    // With an argument, the loopback transport on the given addresses. Without
    // one, whatever this platform's real radio is — which on a Mac is
    // CoreBluetooth, and is the only way to exercise it outside the app.
    let transport: Arc<dyn cabalmesh_lib::ble::link::BleTransport> = match std::env::args().nth(1) {
        Some(spec) => match parse(&spec) {
            Ok((listen, dial)) => Arc::new(LoopbackTransport::new(listen, dial)),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        },
        None => match ble::backend::choose() {
            Some(transport) => transport,
            None => {
                eprintln!("no radio on this platform, and no listen=... given");
                std::process::exit(2);
            }
        },
    };

    println!("transport: {}", transport.describe());

    let (handle, mut events) = ble::spawn(transport, ble::fresh_identity(), Capabilities::none());

    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(BleEvent::PeerAppeared(peer)) => println!("+ peer {peer}"),
                Ok(BleEvent::PeerGone(peer)) => println!("- peer {peer}"),
                Ok(BleEvent::Received { from, kind, payload }) => {
                    let text = String::from_utf8_lossy(&payload);
                    println!("< {kind:?} from {from}: {text}");
                }
                Ok(BleEvent::Unavailable(reason)) => println!("! radio unavailable: {reason}"),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Something to see on the other end, on a slow enough cadence that the
    // output stays readable.
    let sender = handle.clone();
    tokio::spawn(async move {
        let mut counter = 0u32;
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            counter += 1;
            let payload = format!("hello from the terminal #{counter}");
            if sender
                .broadcast(PacketKind::Intent, payload.clone().into_bytes())
                .await
                .is_ok()
            {
                println!("> {payload}");
            }
        }
    });

    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        match handle.status().await {
            Ok(status) => println!(
                "{}  links={}  in-range={}  reachable={}  gateways={}  relayed={}  suppressed={}",
                status.peer_id,
                status.links,
                status.direct_peers,
                status.reachable_peers,
                status.gateways,
                status.relayed,
                status.suppressed
            ),
            Err(error) => {
                eprintln!("status failed: {error}");
                return;
            }
        }
    }
}

/// Parses `listen=HOST:PORT,dial=HOST:PORT`.
fn parse(spec: &str) -> Result<(SocketAddr, Vec<SocketAddr>), String> {
    let mut listen = None;
    let mut dial = Vec::new();

    for field in spec.split(',').map(str::trim).filter(|f| !f.is_empty()) {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| format!("`{field}` is not key=value"))?;
        let address: SocketAddr = value
            .parse()
            .map_err(|_| format!("`{value}` is not a host:port address"))?;
        match key {
            "listen" => listen = Some(address),
            "dial" => dial.push(address),
            other => return Err(format!("unknown field `{other}`")),
        }
    }

    listen
        .map(|listen| (listen, dial))
        .ok_or_else(|| "no `listen=` address".to_string())
}
