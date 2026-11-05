//! Guardian enrollment and unlock over the real (loopback) BLE stack.
//!
//! Same shape as `ble_loopback.rs`: real tokio tasks, real sockets, no
//! radio. What this proves beyond `guardian.rs`'s own unit tests — which
//! never touch a network — is the thing that file cannot: that the actual
//! message bytes cross the wire and land correctly, and that ephemeral peer
//! ids changing between "sessions" (simulated here by spawning a fresh BLE
//! identity against the same, persisted, guardian store) still lets an
//! owner be recognised by the same guardians it enrolled earlier.

use cabal_ble::peers::Capabilities;
use cabal_ble::wire::PacketKind;
use cabal_ble::PeerId;
use cabal_guardian::protocol::GuardianMessage;
use cabal_vault::DataKey;
use cabalmesh_lib::ble::{self, link::LoopbackTransport, BleEvent, BleHandle};
use cabalmesh_lib::guardian::GuardianService;
use cabalmesh_lib::guardian_actor::{approve_unlock, enroll_guardians, request_unlock, respond_to_guardian_traffic, PendingApprovals};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::Mutex;

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("addr").port()
}

fn address(port: u16) -> SocketAddr {
    format!("127.0.0.1:{port}").parse().expect("addr")
}

/// A node with its own BLE identity and a `GuardianService` opened on
/// `guardian_dir` — pass the same directory across two calls to simulate the
/// same device across a restart, with a fresh ephemeral peer id both times.
///
/// Spawns a background responder for the lifetime of the test, so this node
/// always answers guardian traffic addressed to it as a guardian for someone
/// else, exactly like a real device with the app open.
fn spawn_node(port: u16, peers: &[u16], guardian_dir: &Path) -> (BleHandle, Arc<Mutex<GuardianService>>) {
    let transport = LoopbackTransport::new(address(port), peers.iter().copied().map(address).collect());
    let (handle, _initial_events) = ble::spawn(Arc::new(transport), ble::fresh_identity(), Capabilities::none());
    let service = Arc::new(Mutex::new(GuardianService::open(guardian_dir)));

    let mut responder_events = handle.subscribe();
    let responder_ble = handle.clone();
    let responder_service = Arc::clone(&service);
    let approvals = PendingApprovals::new();
    tokio::spawn(async move {
        loop {
            match responder_events.recv().await {
                Ok(BleEvent::Received { from, kind: PacketKind::Guardian, payload }) => {
                    if let Ok(message) = GuardianMessage::from_bytes(&payload) {
                        // Stands in for the human tap the real app requires
                        // (see `PendingApprovals`'s docs): this harness has
                        // no UI, so it approves immediately, which is enough
                        // to prove the wire protocol and the gate's plumbing
                        // both work — a human declining is not a networking
                        // question this file is responsible for testing.
                        if let Some(id) =
                            respond_to_guardian_traffic(&responder_service, &responder_ble, &approvals, from, message).await
                        {
                            let _ = approve_unlock(&approvals, &responder_ble, id).await;
                        }
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    (handle, service)
}

#[tokio::test]
async fn enrolling_and_unlocking_across_a_simulated_session_restart() {
    let owner_dir = TempDir::new().unwrap();
    let guardian_dirs: Vec<TempDir> = (0..3).map(|_| TempDir::new().unwrap()).collect();
    let vault_key = DataKey::from_bytes([77; 32]);

    // --- Session 1: enroll. ---
    let port_owner = free_port();
    let guardian_ports: Vec<u16> = (0..3).map(|_| free_port()).collect();

    let (owner_ble, owner_service) = spawn_node(port_owner, &[], owner_dir.path());
    for (dir, &port) in guardian_dirs.iter().zip(&guardian_ports) {
        spawn_node(port, &[port_owner], dir.path());
    }

    tokio::time::sleep(Duration::from_secs(1)).await;

    let candidates: Vec<PeerId> = owner_ble.peers().await.expect("owner peers").iter().map(|p| p.id).collect();
    assert_eq!(candidates.len(), 3, "owner should see all three guardian candidates");

    let owner_events = owner_ble.subscribe();
    let outcome = enroll_guardians(&owner_service, &owner_ble, owner_events, &candidates, 2, &vault_key)
        .await
        .expect("enrollment should succeed");

    assert_eq!(outcome.enrolled.len(), 3, "all three guardians should have accepted: {outcome:?}");
    assert!(outcome.no_reply.is_empty());

    // --- Session 2: fresh ephemeral identities for everyone, same disk state. ---
    let port_owner_2 = free_port();
    let guardian_ports_2: Vec<u16> = (0..3).map(|_| free_port()).collect();

    let (owner_ble_2, owner_service_2) = spawn_node(port_owner_2, &[], owner_dir.path());
    // Only two of the three guardians are reachable this time — the normal
    // case, per the design doc: some guardians being unreachable at any
    // given unlock is expected, not an edge case.
    for (dir, &port) in guardian_dirs.iter().take(2).zip(&guardian_ports_2) {
        spawn_node(port, &[port_owner_2], dir.path());
    }

    tokio::time::sleep(Duration::from_secs(1)).await;

    let owner_events_2 = owner_ble_2.subscribe();
    let candidate_key = request_unlock(&owner_service_2, &owner_ble_2, owner_events_2)
        .await
        .expect("unlock should reconstruct the key from two of three guardians");

    assert_eq!(candidate_key.expose_for_storage(), vault_key.expose_for_storage());

    // Nothing about the owner's own identity survived the "restart" except
    // what was actually persisted to disk.
    let owner_status_1 = owner_ble.status().await.unwrap();
    let owner_status_2 = owner_ble_2.status().await.unwrap();
    assert_ne!(owner_status_1.peer_id, owner_status_2.peer_id, "the ephemeral identity should not have survived");
}

#[tokio::test]
async fn unlock_fails_when_fewer_than_the_threshold_are_reachable() {
    let owner_dir = TempDir::new().unwrap();
    let guardian_dirs: Vec<TempDir> = (0..3).map(|_| TempDir::new().unwrap()).collect();
    let vault_key = DataKey::from_bytes([9; 32]);

    let port_owner = free_port();
    let guardian_ports: Vec<u16> = (0..3).map(|_| free_port()).collect();
    let (owner_ble, owner_service) = spawn_node(port_owner, &[], owner_dir.path());
    for (dir, &port) in guardian_dirs.iter().zip(&guardian_ports) {
        spawn_node(port, &[port_owner], dir.path());
    }
    tokio::time::sleep(Duration::from_secs(1)).await;

    let candidates: Vec<PeerId> = owner_ble.peers().await.expect("owner peers").iter().map(|p| p.id).collect();
    let owner_events = owner_ble.subscribe();
    enroll_guardians(&owner_service, &owner_ble, owner_events, &candidates, 3, &vault_key)
        .await
        .expect("enrollment should succeed");

    // A later session where only one of the three (needing three) is up.
    let port_owner_2 = free_port();
    let guardian_port_2 = free_port();
    let (owner_ble_2, owner_service_2) = spawn_node(port_owner_2, &[], owner_dir.path());
    spawn_node(guardian_port_2, &[port_owner_2], guardian_dirs[0].path());
    tokio::time::sleep(Duration::from_secs(1)).await;

    let owner_events_2 = owner_ble_2.subscribe();
    let result = request_unlock(&owner_service_2, &owner_ble_2, owner_events_2).await;
    assert!(result.is_err(), "unlock must not succeed with fewer than the threshold answering");
}
