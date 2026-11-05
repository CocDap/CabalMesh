//! A handle onto the mesh actor.
//!
//! # Why an actor
//!
//! `libp2p::Swarm` is not `Sync`, so it must live in exactly one task. That is
//! not a limitation to work around — it is the correct shape for a component
//! driving a single event loop. Everything else talks to it by message.
//!
//! # Why the channel is bounded
//!
//! The previous channel was unbounded. A UI that spams an action — a user
//! holding "refresh nodes", or a retry loop that never backs off — would grow
//! the queue until the process is killed, and on a 2 GB phone that arrives
//! quickly. A bounded channel turns the same situation into backpressure: the
//! caller waits, or gets a typed error, and the device stays alive.
//!
//! # What lives here and what does not
//!
//! The handle carries *requests*. Events flowing the other way — peers
//! discovered, intents received — stay on the existing event channel to the
//! frontend, because they are broadcast rather than request/response.

use crate::mesh::PrivacyIntent;
use tokio::sync::{mpsc, oneshot};

/// Queue depth for requests to the mesh actor.
///
/// Small on purpose. This is a request queue, not a buffer: if more than this
/// many requests are outstanding, the actor is wedged or the caller is looping,
/// and blocking is the useful response to both.
const COMMAND_QUEUE: usize = 32;

/// Why a mesh request failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MeshError {
    /// The actor task has terminated. Every later request fails the same way.
    #[error("mesh actor is not running")]
    ActorGone,

    /// The actor accepted the request and then died without answering.
    #[error("mesh actor dropped the request without answering")]
    NoReply,

    /// Publishing failed at the gossipsub layer.
    #[error("publishing to the mesh failed")]
    Publish,
}

/// A request to the mesh actor.
///
/// Each variant carries its own reply channel, so a caller awaits exactly its
/// own answer rather than a shared response stream it has to demultiplex.
#[derive(Debug)]
pub enum MeshCommand {
    /// Broadcast an intent to the topic.
    Publish {
        intent: Box<PrivacyIntent>,
        reply: oneshot::Sender<Result<(), MeshError>>,
    },
    /// Current mesh status for the home screen.
    Snapshot { reply: oneshot::Sender<MeshSnapshot> },
    /// Stop or resume participating, without tearing the swarm down.
    SetOffline {
        offline: bool,
        reply: oneshot::Sender<()>,
    },
}

/// What the home screen needs to render mesh status.
#[derive(Debug, Clone, Default)]
pub struct MeshSnapshot {
    /// This node's libp2p peer id.
    ///
    /// Taken from the swarm rather than parsed out of a listen address: listen
    /// addresses carry no `/p2p/` component, so deriving it from them yields
    /// the address itself.
    pub peer_id: String,
    pub peer_count: usize,
    pub listening_on: Vec<String>,
    pub offline: bool,
    /// Whether the IP plane currently has at least one connected peer —
    /// see [`crate::mesh::MeshEvent::ConnectivityChanged`].
    pub online: bool,
    pub relay_bytes: u64,
}

/// Cheap, clonable access to the mesh actor.
#[derive(Clone, Debug)]
pub struct MeshHandle {
    tx: mpsc::Sender<MeshCommand>,
}

impl MeshHandle {
    /// Creates a handle and the receiver the actor loop should drain.
    #[must_use]
    pub fn channel() -> (Self, mpsc::Receiver<MeshCommand>) {
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
        (Self { tx }, rx)
    }

    /// Broadcasts an intent.
    ///
    /// Awaits the actor's acknowledgement rather than firing and forgetting, so
    /// a caller learns that publishing failed instead of assuming it worked.
    ///
    /// # Errors
    ///
    /// [`MeshError::ActorGone`] if the actor has stopped, [`MeshError::NoReply`]
    /// if it stopped mid-request, or [`MeshError::Publish`] on a gossipsub
    /// failure.
    pub async fn publish(&self, intent: PrivacyIntent) -> Result<(), MeshError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(MeshCommand::Publish {
                intent: Box::new(intent),
                reply,
            })
            .await
            .map_err(|_| MeshError::ActorGone)?;
        answer.await.map_err(|_| MeshError::NoReply)?
    }

    /// Current mesh status.
    ///
    /// # Errors
    ///
    /// As [`MeshHandle::publish`].
    pub async fn snapshot(&self) -> Result<MeshSnapshot, MeshError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(MeshCommand::Snapshot { reply })
            .await
            .map_err(|_| MeshError::ActorGone)?;
        answer.await.map_err(|_| MeshError::NoReply)
    }

    /// Stops or resumes mesh participation.
    ///
    /// Used by the offline switch and by suspension. Deliberately does not tear
    /// the swarm down: rebuilding it on resume would lose every established
    /// connection and re-run discovery from nothing.
    ///
    /// # Errors
    ///
    /// As [`MeshHandle::publish`].
    pub async fn set_offline(&self, offline: bool) -> Result<(), MeshError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(MeshCommand::SetOffline { offline, reply })
            .await
            .map_err(|_| MeshError::ActorGone)?;
        answer.await.map_err(|_| MeshError::NoReply)
    }

    /// Whether the actor is still accepting requests.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.tx.is_closed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent() -> PrivacyIntent {
        PrivacyIntent {
            id: "test-intent".into(),
            intent_type: "trade".into(),
            payload: "{}".into(),
            encrypted: true,
            relay_path: vec!["origin_node".into()],
            relay_fee: None,
        }
    }

    #[tokio::test]
    async fn a_dead_actor_is_reported_rather_than_hanging() {
        // The failure this replaces: an unbounded send to a dead receiver
        // succeeded silently, so callers believed intents were broadcast.
        let (handle, rx) = MeshHandle::channel();
        drop(rx);

        assert!(matches!(
            handle.publish(intent()).await,
            Err(MeshError::ActorGone)
        ));
        assert!(!handle.is_running());
    }

    #[tokio::test]
    async fn an_actor_that_dies_mid_request_is_reported() {
        let (handle, mut rx) = MeshHandle::channel();
        tokio::spawn(async move {
            // Accept the request, then drop the reply channel without answering.
            let _ = rx.recv().await;
        });

        assert!(matches!(handle.publish(intent()).await, Err(MeshError::NoReply)));
    }

    #[tokio::test]
    async fn requests_are_answered_individually() {
        let (handle, mut rx) = MeshHandle::channel();
        tokio::spawn(async move {
            while let Some(command) = rx.recv().await {
                match command {
                    MeshCommand::Publish { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                    MeshCommand::Snapshot { reply } => {
                        let _ = reply.send(MeshSnapshot {
                            peer_count: 3,
                            ..MeshSnapshot::default()
                        });
                    }
                    MeshCommand::SetOffline { reply, .. } => {
                        let _ = reply.send(());
                    }
                }
            }
        });

        assert!(handle.publish(intent()).await.is_ok());
        assert_eq!(handle.snapshot().await.unwrap().peer_count, 3);
        assert!(handle.set_offline(true).await.is_ok());
    }

    #[tokio::test]
    async fn the_queue_applies_backpressure_rather_than_growing() {
        // The whole reason the channel is bounded: a caller that outruns the
        // actor must wait, not accumulate. With an unbounded channel this
        // would fill memory instead of blocking.
        let (handle, _rx) = MeshHandle::channel();

        // Fill the queue without draining it.
        for _ in 0..COMMAND_QUEUE {
            let (reply, _answer) = oneshot::channel();
            handle
                .tx
                .try_send(MeshCommand::Publish {
                    intent: Box::new(intent()),
                    reply,
                })
                .expect("queue has room");
        }

        let (reply, _answer) = oneshot::channel();
        assert!(
            handle
                .tx
                .try_send(MeshCommand::Publish {
                    intent: Box::new(intent()),
                    reply
                })
                .is_err(),
            "a full queue must refuse rather than grow"
        );
    }

    #[tokio::test]
    async fn clones_address_the_same_actor() {
        let (handle, mut rx) = MeshHandle::channel();
        let clone = handle.clone();
        tokio::spawn(async move {
            while let Some(MeshCommand::Publish { reply, .. }) = rx.recv().await {
                let _ = reply.send(Ok(()));
            }
        });

        assert!(handle.publish(intent()).await.is_ok());
        assert!(clone.publish(intent()).await.is_ok());
    }
}
