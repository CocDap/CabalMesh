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
use crate::guardian::{ReleasedShare, RestoreMatch, RestoreSession};
use cabal_guardian::protocol::{EnrollRequest, GuardianMessage, RestoreRequest, RestoreStatus, TAG_LEN};
use chrono::{DateTime, Utc};
use cabal_guardian::sealed::GuardianPublicKey;
use cabal_guardian::Share;
use cabal_vault::DataKey;
use std::collections::{HashMap, HashSet};
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

/// Something waiting on a human, held back until they decide.
enum PendingApproval {
    /// An everyday unlock: the replies are computed already.
    Unlock { from: PeerId, replies: Vec<GuardianMessage> },
    /// A restore onto a new device: approving starts the delay.
    Restore { from: PeerId, request: RestoreRequest },
}

/// What kind of request a prompt is about. Restore is the riskier one —
/// a new device, possibly not the owner — and the prompt says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalKind {
    Unlock,
    Restore,
}

/// Approvals waiting on a human, keyed by an id the frontend can act on.
/// Shared between the background listener that discovers a match and the
/// `guardian_approve_unlock`/`guardian_deny_unlock` commands a screen calls
/// once the person taps something.
///
/// This is the whole reason `respond_to_guardian_traffic` does not simply
/// send a guardian's share back the moment a request matches: the design
/// doc is explicit that a reply is not automatic — "a human presses
/// approve" — because the one scenario this scheme genuinely does not
/// defend (a stolen-but-still-genuine device) depends entirely on that human
/// noticing something is wrong before tapping.
#[derive(Clone, Default)]
pub struct PendingApprovals {
    next_id: Arc<AtomicU32>,
    pending: Arc<Mutex<HashMap<u32, PendingApproval>>>,
    /// Restore nonces already prompted for or denied. A new device repeats
    /// its request while it waits; without this, every repeat would raise
    /// another prompt.
    seen_restores: Arc<Mutex<HashSet<[u8; TAG_LEN]>>>,
}

impl PendingApprovals {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    async fn insert(&self, approval: PendingApproval) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.pending.lock().await.insert(id, approval);
        id
    }

    /// Removes and returns a pending approval, so approving or denying the
    /// same id twice is a no-op the second time rather than a double-send.
    async fn take(&self, id: u32) -> Option<PendingApproval> {
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

/// What the app should surface after a guardian packet was handled.
#[derive(Debug, Clone)]
pub enum GuardianNotice {
    /// A human must approve or deny `id` — see [`approve`].
    Approval { id: u32, kind: ApprovalKind, from: PeerId, enrolled_at: Option<DateTime<Utc>> },
    /// Owner side: a guardian is holding a restore of *this* device's vault,
    /// completing at `release_at` unless vetoed with
    /// [`broadcast_restore_cancel`].
    RestorePending { release_at: DateTime<Utc> },
}

/// Reacts to guardian traffic arriving at this device in its *passive*
/// roles — a guardian for someone else, and an owner listening for restores
/// of its own vault. Call this for every
/// `BleEvent::Received { kind: PacketKind::Guardian, .. }` the app observes;
/// it silently ignores replies that belong to the *active* flows below.
///
/// Enrollment, backup refreshes and cancels are handled here to completion:
/// the design doc does not call for a human gate on *becoming* or *staying*
/// a guardian, only on *answering for* one.
pub async fn respond_to_guardian_traffic(
    service: &Mutex<GuardianService>,
    ble: &BleHandle,
    approvals: &PendingApprovals,
    from: PeerId,
    message: GuardianMessage,
) -> Option<GuardianNotice> {
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
            let id = approvals.insert(PendingApproval::Unlock { from, replies }).await;
            Some(GuardianNotice::Approval { id, kind: ApprovalKind::Unlock, from, enrolled_at: None })
        }
        GuardianMessage::RestoreRequest(request) => {
            let matched = {
                let service = service.lock().await;
                service.guardian_match_restore_request(&request, Utc::now())
            };
            match matched {
                RestoreMatch::NotMine => None,
                RestoreMatch::Reply(reply) => {
                    let _ = ble.send_to(from, PacketKind::Guardian, reply.to_bytes()).await;
                    None
                }
                RestoreMatch::NeedsApproval { enrolled_at } => {
                    if !approvals.seen_restores.lock().await.insert(request.nonce) {
                        return None;
                    }
                    let id = approvals.insert(PendingApproval::Restore { from, request }).await;
                    Some(GuardianNotice::Approval { id, kind: ApprovalKind::Restore, from, enrolled_at: Some(enrolled_at) })
                }
            }
        }
        GuardianMessage::BackupUpdate(update) => {
            let _ = service.lock().await.guardian_receive_backup_update(&update);
            None
        }
        GuardianMessage::RestoreCancel(cancel) => {
            let _ = service.lock().await.guardian_receive_cancel(&cancel);
            None
        }
        GuardianMessage::RestorePending(pending) => {
            let release_at = service.lock().await.owner_match_pending(&pending)?;
            Some(GuardianNotice::RestorePending { release_at })
        }
        // These flow back to *this* device's own active calls below, not to
        // the passive responder.
        GuardianMessage::EnrollAccept(_) | GuardianMessage::UnlockReply(_) | GuardianMessage::RestoreReply(_) => None,
    }
}

/// Acts on a pending approval, once a human has agreed to it: sends unlock
/// replies, or starts a restore's delay and tells the new device so.
///
/// # Errors
///
/// [`ActorError::NoSuchApproval`] if `id` was already approved, denied, or
/// never existed (a stale prompt, or a double tap).
pub async fn approve(
    approvals: &PendingApprovals,
    service: &Mutex<GuardianService>,
    ble: &BleHandle,
    id: u32,
) -> Result<(), ActorError> {
    match approvals.take(id).await.ok_or(ActorError::NoSuchApproval)? {
        PendingApproval::Unlock { from, replies } => {
            for reply in replies {
                let _ = ble.send_to(from, PacketKind::Guardian, reply.to_bytes()).await;
            }
        }
        PendingApproval::Restore { from, request } => {
            let (reply, announcements) = {
                let mut service = service.lock().await;
                let reply = service.guardian_approve_restore(&request, Utc::now())?;
                (reply, service.guardian_pending_announcements(Utc::now()))
            };
            let _ = ble.send_to(from, PacketKind::Guardian, reply.to_bytes()).await;
            // Tell the original device straight away, not only on the
            // periodic re-announcement.
            for announcement in announcements {
                let _ = ble.broadcast(PacketKind::Guardian, announcement.to_bytes()).await;
            }
        }
    }
    Ok(())
}

/// Discards a pending approval without sending anything.
///
/// # Errors
///
/// [`ActorError::NoSuchApproval`] if `id` was already approved, denied, or
/// never existed.
pub async fn deny(approvals: &PendingApprovals, id: u32) -> Result<(), ActorError> {
    approvals.take(id).await.ok_or(ActorError::NoSuchApproval)?;
    Ok(())
}

/// Re-announces every restore this device is holding, so an original
/// device that comes into range during the delay still hears about it.
pub async fn announce_pending_restores(service: &Mutex<GuardianService>, ble: &BleHandle) {
    let announcements = service.lock().await.guardian_pending_announcements(Utc::now());
    for announcement in announcements {
        let _ = ble.broadcast(PacketKind::Guardian, announcement.to_bytes()).await;
    }
}

/// The veto, from the original device.
///
/// # Errors
///
/// [`GuardianServiceError::NotEnrolled`] without a restore-capable enrollment.
pub async fn broadcast_restore_cancel(service: &Mutex<GuardianService>, ble: &BleHandle) -> Result<(), GuardianServiceError> {
    let message = service.lock().await.owner_build_cancel()?;
    let _ = ble.broadcast(PacketKind::Guardian, message.to_bytes()).await;
    Ok(())
}

/// Refreshes every guardian's backup copy and value flag. Guardians out of
/// range miss it and catch up on a later refresh.
///
/// # Errors
///
/// A store failure persisting the new sequence number.
pub async fn broadcast_backup_update(
    service: &Mutex<GuardianService>,
    ble: &BleHandle,
    backup_plaintext: &[u8],
    has_value: bool,
) -> Result<bool, GuardianServiceError> {
    let Some(message) = service.lock().await.owner_build_backup_update(backup_plaintext, has_value)? else {
        return Ok(false);
    };
    let _ = ble.broadcast(PacketKind::Guardian, message.to_bytes()).await;
    Ok(true)
}

/// Where one candidate is in enrollment, for `DISTRIBUTING SHARES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollStage {
    Invited,
    Accepted,
    Delivered,
    NoReply,
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
    backup_plaintext: &[u8],
    has_value: bool,
    progress: &(dyn Fn(PeerId, EnrollStage) + Send + Sync),
) -> Result<EnrollmentOutcome, GuardianServiceError> {
    for &peer in candidates {
        let _ = ble.send_to(peer, PacketKind::Guardian, GuardianMessage::EnrollRequest(EnrollRequest).to_bytes()).await;
        progress(peer, EnrollStage::Invited);
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
                        progress(from, EnrollStage::Accepted);
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

    for peer in candidates.iter().filter(|peer| !accepted.iter().any(|(p, _)| p == *peer)) {
        progress(*peer, EnrollStage::NoReply);
    }
    if accepted.is_empty() {
        return Ok(EnrollmentOutcome { enrolled: Vec::new(), no_reply: candidates.to_vec() });
    }

    let keys: Vec<GuardianPublicKey> = accepted.iter().map(|(_, key)| *key).collect();
    let messages = {
        let mut service = service.lock().await;
        service.owner_prepare_enrollment(vault_key, &keys, k, backup_plaintext, has_value)?
    };

    for ((_, message), (peer, _)) in messages.iter().zip(&accepted) {
        if ble.send_to(*peer, PacketKind::Guardian, message.to_bytes()).await.is_ok() {
            progress(*peer, EnrollStage::Delivered);
        }
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

    let reconstructed = cabal_guardian::reconstruct(threshold, &shares)?;
    service
        .lock()
        .await
        .owner_vault_key_from_recovery(reconstructed)
        .ok_or(GuardianServiceError::Malformed)
}

/// What one round of a restore heard back.
#[derive(Default, Clone)]
pub struct RestoreRound {
    pub released: Vec<ReleasedShare>,
    /// One entry per guardian still inside its delay: when it releases.
    pub waiting: Vec<DateTime<Utc>>,
    pub cancelled: usize,
}

/// One round of restoring onto a new device: broadcasts `session`'s request
/// and collects replies for `window`. Repeat until enough shares are
/// released; the same session must be reused each time (see
/// [`RestoreSession`]).
pub async fn restore_round(
    session: &RestoreSession,
    ble: &BleHandle,
    mut events: broadcast::Receiver<BleEvent>,
    window: Duration,
) -> RestoreRound {
    let _ = ble.broadcast(PacketKind::Guardian, session.request().to_bytes()).await;

    let mut round = RestoreRound::default();
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let deadline = Instant::now() + window;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(BleEvent::Received { from, kind: PacketKind::Guardian, payload })) => {
                let Ok(GuardianMessage::RestoreReply(sealed)) = GuardianMessage::from_bytes(&payload) else { continue };
                let Some(status) = session.open_reply(&sealed) else { continue };
                // One answer per guardian per round.
                if !seen.insert(from.to_string().into_bytes()) {
                    continue;
                }
                match status {
                    RestoreStatus::Released { share, threshold, backup, backup_seq } => {
                        if let Ok(share) = Share::from_bytes(&share) {
                            round.released.push(ReleasedShare { share, threshold, backup, backup_seq });
                        }
                    }
                    RestoreStatus::Waiting { release_at } => {
                        if let Some(at) = DateTime::from_timestamp(release_at, 0) {
                            round.waiting.push(at);
                        }
                    }
                    RestoreStatus::Cancelled => round.cancelled += 1,
                }
            }
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => break,
        }
    }
    round
}
