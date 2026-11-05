//! Async orchestration wiring [`GuardianService`] to the BLE plane.
//!
//! Split from `guardian.rs` the same way `mesh_handle.rs` is split from
//! `mesh.rs`: the sync core holds persistence and protocol logic and is
//! cheap to test exhaustively; this file is the network glue, tested against
//! a real (loopback) transport in `tests/guardian_loopback.rs` rather than
//! with a mock, for the same reason `tests/ble_loopback.rs` exists — "the
//! protocol is correct but nothing happens" is a real failure mode that
//! in-memory tests cannot catch.

use crate::ble::{BleEvent, BleHandle};
use crate::guardian::{self, GuardianService, GuardianServiceError};
use cabal_ble::wire::PacketKind;
use cabal_ble::PeerId;
use cabal_guardian::protocol::{EnrollRequest, GuardianMessage};
use cabal_guardian::sealed::GuardianPublicKey;
use cabal_guardian::Share;
use cabal_vault::DataKey;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex};
use tokio::time::Instant;

/// How long an enrollment or unlock waits for replies before giving up on
/// whoever has not answered. Generous: BLE discovery and jittered relay
/// delays are already measured in single-digit seconds in the worst case
/// (`docs/ble-mesh-design.md`), and a guardian is a human who has to notice
/// a prompt and tap something.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(20);

/// A guardian's reply to an unlock request, computed and ready to send —
/// held back until a human explicitly approves it.
struct PendingUnlockApproval {
    from: PeerId,
    replies: Vec<GuardianMessage>,
}

/// Unlock replies waiting on a human, keyed by an id the frontend can act
/// on. Shared between the background listener that discovers a match and
/// the `guardian_approve_unlock`/`guardian_deny_unlock` commands a screen
/// calls once the person taps something.
///
/// This is the whole reason `respond_to_guardian_traffic` does not simply
/// send a guardian's share back the moment a request matches: the design
/// doc is explicit that an unlock reply is not automatic — "a human presses
/// approve" — because the one scenario this scheme genuinely does not
/// defend (a stolen-but-still-genuine device) depends entirely on that human
/// noticing something is wrong before tapping.
#[derive(Clone, Default)]
pub struct PendingApprovals {
    next_id: Arc<AtomicU32>,
    pending: Arc<Mutex<HashMap<u32, PendingUnlockApproval>>>,
}

impl PendingApprovals {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    async fn insert(&self, approval: PendingUnlockApproval) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.pending.lock().await.insert(id, approval);
        id
    }

    /// Removes and returns a pending approval, so approving or denying the
    /// same id twice is a no-op the second time rather than a double-send.
    async fn take(&self, id: u32) -> Option<PendingUnlockApproval> {
        self.pending.lock().await.remove(&id)
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ActorError {
    #[error("no pending approval with that id — it was already resolved, or never existed")]
    NoSuchApproval,
    #[error(transparent)]
    Service(#[from] GuardianServiceError),
}

/// Reacts to guardian traffic addressed to this device in its *passive*
/// role — a guardian for someone else. Call this for every
/// `BleEvent::Received { kind: PacketKind::Guardian, .. }` the app observes;
/// it silently ignores message types that belong to the *active* flows below
/// instead (an owner's own device receiving replies to its own request).
///
/// Returns `Some(id)` exactly when an unlock reply is ready and waiting on
/// [`approve_unlock`] — the caller's job is to surface that id to the
/// person, e.g. as a Tauri event driving an "UNLOCK REQUEST" prompt.
/// Enrollment, by contrast, is handled here to completion: the design doc
/// does not call for a human gate on *becoming* a guardian, only on
/// *answering for* one later.
pub async fn respond_to_guardian_traffic(
    service: &Mutex<GuardianService>,
    ble: &BleHandle,
    approvals: &PendingApprovals,
    from: PeerId,
    message: GuardianMessage,
) -> Option<u32> {
    match message {
        GuardianMessage::EnrollRequest(request) => {
            let accept = {
                let mut service = service.lock().await;
                guardian::accept_enrollment(&mut service, &request)
            };
            if let Ok(accept) = accept {
                let _ = ble.send_to(from, PacketKind::Guardian, GuardianMessage::EnrollAccept(accept).to_bytes()).await;
            }
            None
        }
        GuardianMessage::Enroll(sealed) => {
            let mut service = service.lock().await;
            // A failure here (wrong key, malformed payload) means this
            // device was never the intended recipient of this particular
            // packet — nothing to recover from, and nothing to tell anyone,
            // since the sender already believes delivery succeeded.
            let _ = service.guardian_receive_enroll(&sealed);
            None
        }
        GuardianMessage::UnlockRequest(request) => {
            let replies = {
                let service = service.lock().await;
                service.guardian_match_unlock_request(&request)
            };
            if replies.is_empty() {
                return None;
            }
            Some(approvals.insert(PendingUnlockApproval { from, replies }).await)
        }
        // These flow back to *this* device's own active enroll/unlock
        // calls below, not to the passive responder.
        GuardianMessage::EnrollAccept(_) | GuardianMessage::UnlockReply(_) => None,
    }
}

/// Sends a pending unlock reply, once a human has agreed to it.
///
/// # Errors
///
/// [`ActorError::NoSuchApproval`] if `id` was already approved, denied, or
/// never existed (a stale prompt, or a double tap).
pub async fn approve_unlock(approvals: &PendingApprovals, ble: &BleHandle, id: u32) -> Result<(), ActorError> {
    let approval = approvals.take(id).await.ok_or(ActorError::NoSuchApproval)?;
    for reply in approval.replies {
        let _ = ble.send_to(approval.from, PacketKind::Guardian, reply.to_bytes()).await;
    }
    Ok(())
}

/// Discards a pending unlock reply without sending anything.
///
/// # Errors
///
/// [`ActorError::NoSuchApproval`] if `id` was already approved, denied, or
/// never existed.
pub async fn deny_unlock(approvals: &PendingApprovals, id: u32) -> Result<(), ActorError> {
    approvals.take(id).await.ok_or(ActorError::NoSuchApproval)?;
    Ok(())
}

/// Who answered an enrollment invitation, and who did not.
#[derive(Debug, Clone)]
pub struct EnrollmentOutcome {
    pub enrolled: Vec<PeerId>,
    pub no_reply: Vec<PeerId>,
}

/// Invites `candidates` to become guardians, waits for each to accept, and
/// on any acceptances splits `vault_key` and sends each accepted candidate
/// its sealed share.
///
/// Candidates who never accept are reported, not retried — the caller (the
/// `SET UP` screen) decides whether fewer guardians than requested is still
/// enough to proceed.
///
/// # Errors
///
/// Whatever [`GuardianService::owner_prepare_enrollment`] returns, most
/// notably [`GuardianServiceError::InvalidGuardianCount`] if nobody accepted
/// at all.
pub async fn enroll_guardians(
    service: &Mutex<GuardianService>,
    ble: &BleHandle,
    mut events: broadcast::Receiver<BleEvent>,
    candidates: &[PeerId],
    k: u8,
    vault_key: &DataKey,
) -> Result<EnrollmentOutcome, GuardianServiceError> {
    for &peer in candidates {
        let _ = ble.send_to(peer, PacketKind::Guardian, GuardianMessage::EnrollRequest(EnrollRequest).to_bytes()).await;
    }

    let mut accepted: Vec<(PeerId, GuardianPublicKey)> = Vec::new();
    let deadline = Instant::now() + REPLY_TIMEOUT;

    while accepted.len() < candidates.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(BleEvent::Received { from, kind: PacketKind::Guardian, payload })) => {
                if candidates.contains(&from) && !accepted.iter().any(|(peer, _)| *peer == from) {
                    if let Ok(GuardianMessage::EnrollAccept(accept)) = GuardianMessage::from_bytes(&payload) {
                        accepted.push((from, GuardianPublicKey::from_bytes(accept.guardian_public_key)));
                    }
                }
            }
            Ok(Ok(_)) => {}
            // A burst outpaced this subscriber; keep waiting rather than
            // treating a gap in delivery as "nobody answered."
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => break,
        }
    }

    if accepted.is_empty() {
        return Ok(EnrollmentOutcome { enrolled: Vec::new(), no_reply: candidates.to_vec() });
    }

    let keys: Vec<GuardianPublicKey> = accepted.iter().map(|(_, key)| *key).collect();
    let messages = {
        let mut service = service.lock().await;
        service.owner_prepare_enrollment(vault_key, &keys, k)?
    };

    for ((_, message), (peer, _)) in messages.iter().zip(&accepted) {
        let _ = ble.send_to(*peer, PacketKind::Guardian, message.to_bytes()).await;
    }

    let enrolled: Vec<PeerId> = accepted.iter().map(|(peer, _)| *peer).collect();
    let no_reply = candidates.iter().filter(|peer| !enrolled.contains(peer)).copied().collect();
    Ok(EnrollmentOutcome { enrolled, no_reply })
}

/// Broadcasts an unlock request, collects sealed replies until the
/// enrollment threshold is met or the timeout elapses, and reconstructs a
/// candidate vault key.
///
/// **The returned key is a candidate, not a verified one** — see
/// `cabal_guardian::reconstruct`'s docs. The caller must still attempt to
/// open the real vault with it (`IdentityStore::unlock_with_guardian_key`)
/// before trusting it for anything.
///
/// # Errors
///
/// [`GuardianServiceError::NotEnrolled`] if this device never enrolled
/// guardians. [`GuardianServiceError::Crypto`] if fewer than the threshold
/// answered before the timeout.
pub async fn request_unlock(
    service: &Mutex<GuardianService>,
    ble: &BleHandle,
    mut events: broadcast::Receiver<BleEvent>,
) -> Result<DataKey, GuardianServiceError> {
    let (request, threshold) = {
        let service = service.lock().await;
        (service.owner_build_unlock_request()?, service.owner_threshold())
    };
    let _ = ble.broadcast(PacketKind::Guardian, GuardianMessage::UnlockRequest(request).to_bytes()).await;

    let mut shares: Vec<Share> = Vec::new();
    let deadline = Instant::now() + REPLY_TIMEOUT;

    while shares.len() < usize::from(threshold) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(BleEvent::Received { kind: PacketKind::Guardian, payload, .. })) => {
                if let Ok(GuardianMessage::UnlockReply(sealed)) = GuardianMessage::from_bytes(&payload) {
                    let opened = {
                        let service = service.lock().await;
                        service.owner_open_unlock_reply(&sealed)
                    };
                    if let Some(share) = opened {
                        shares.push(share);
                    }
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => break,
        }
    }

    Ok(cabal_guardian::reconstruct(threshold, &shares)?)
}
