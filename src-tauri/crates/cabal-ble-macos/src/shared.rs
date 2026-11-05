//! The only thing the two sides both touch.
//!
//! # Why a mutex over bytes rather than a channel to the radio
//!
//! The radio lives on a dispatch queue and owns Objective-C objects that are
//! neither `Send` nor `Sync`. If the Rust side had to hand it anything richer
//! than bytes, every one of those objects would need a thread-safety argument.
//!
//! Instead: Rust appends to an outbox, the queue drains it. Rust reads events
//! from a channel, the queue writes them. Nothing else crosses. The unsafe in
//! this crate is then confined to talking to CoreBluetooth, rather than also
//! covering how two threads share a peripheral.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;

/// A link to one peer. Allocated by the radio, opaque to everyone else.
pub type LinkId = u64;

/// Something the radio observed.
#[derive(Debug)]
pub enum Event {
    /// An L2CAP channel opened.
    Up(LinkId),
    /// A channel closed, or the peer went away.
    Down(LinkId),
    /// Bytes arrived.
    Bytes { link: LinkId, bytes: Vec<u8> },
    /// The radio will not start, or stopped being usable.
    ///
    /// Carries what the OS said: "powered off" and "the user declined the
    /// permission" are different problems and only one of them is fixable by
    /// turning Bluetooth on.
    Unavailable(String),
}

/// What the Rust side and the dispatch queue both reach.
#[derive(Debug)]
pub struct Shared {
    outbox: Mutex<HashMap<LinkId, VecDeque<Vec<u8>>>>,
    closing: Mutex<Vec<LinkId>>,
    events: Sender<Event>,
    next_link: AtomicU64,
    stopped: AtomicBool,
}

impl Shared {
    /// Creates the shared state and the receiving end of its event stream.
    #[must_use]
    pub fn new() -> (std::sync::Arc<Self>, Receiver<Event>) {
        let (events, receiver) = std::sync::mpsc::channel();
        (
            std::sync::Arc::new(Self {
                outbox: Mutex::new(HashMap::new()),
                closing: Mutex::new(Vec::new()),
                events,
                next_link: AtomicU64::new(1),
                stopped: AtomicBool::new(false),
            }),
            receiver,
        )
    }

    /// Queues bytes for a link.
    ///
    /// Returns whether the link is still open. A write to a peer that walked
    /// out of range is ordinary rather than exceptional, so this reports it
    /// instead of erroring.
    pub fn queue(&self, link: LinkId, bytes: Vec<u8>) -> bool {
        if self.stopped.load(Ordering::Relaxed) {
            return false;
        }
        let mut outbox = self.lock_outbox();
        match outbox.get_mut(&link) {
            Some(queue) => {
                queue.push_back(bytes);
                true
            }
            None => false,
        }
    }

    /// Registers a new link, returning its identifier.
    pub(crate) fn open(&self) -> LinkId {
        let link = self.next_link.fetch_add(1, Ordering::Relaxed);
        self.lock_outbox().insert(link, VecDeque::new());
        let _ = self.events.send(Event::Up(link));
        link
    }

    /// Removes a link and tells the Rust side.
    pub(crate) fn closed(&self, link: LinkId) {
        if self.lock_outbox().remove(&link).is_some() {
            let _ = self.events.send(Event::Down(link));
        }
    }

    /// Hands received bytes up.
    pub(crate) fn received(&self, link: LinkId, bytes: Vec<u8>) {
        let _ = self.events.send(Event::Bytes { link, bytes });
    }

    /// Reports that the radio is not usable.
    pub(crate) fn unavailable(&self, why: impl Into<String>) {
        let _ = self.events.send(Event::Unavailable(why.into()));
    }

    /// Takes the next chunk queued for a link.
    pub(crate) fn take(&self, link: LinkId) -> Option<Vec<u8>> {
        self.lock_outbox().get_mut(&link)?.pop_front()
    }

    /// Puts a partially written chunk back at the front.
    ///
    /// L2CAP writes are allowed to be short. Without this the tail of a frame
    /// is silently dropped, the peer's length prefix no longer matches, and
    /// the link is torn down for what looks like corruption.
    pub(crate) fn put_back(&self, link: LinkId, rest: Vec<u8>) {
        if let Some(queue) = self.lock_outbox().get_mut(&link) {
            queue.push_front(rest);
        }
    }

    /// Every link with something waiting to go out.
    ///
    /// Used by the tests; the pump walks its own channel map instead, because
    /// a link with no channel behind it has nothing to write to.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn pending(&self) -> Vec<LinkId> {
        self.lock_outbox()
            .iter()
            .filter(|(_, queue)| !queue.is_empty())
            .map(|(link, _)| *link)
            .collect()
    }

    /// Asks for a link to be torn down.
    pub fn close(&self, link: LinkId) {
        self.lock_closing().push(link);
    }

    /// Links the Rust side asked to close since the last call.
    pub(crate) fn drain_closing(&self) -> Vec<LinkId> {
        std::mem::take(&mut *self.lock_closing())
    }

    /// Stops the radio.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    /// Whether the radio has been asked to stop.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    /// A poisoned mutex here means a panic while holding bytes, which is not a
    /// reason to bring the radio down — the data is plain and the invariant is
    /// "a map of queues", which a panic cannot break.
    fn lock_outbox(&self) -> std::sync::MutexGuard<'_, HashMap<LinkId, VecDeque<Vec<u8>>>> {
        self.outbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_closing(&self) -> std::sync::MutexGuard<'_, Vec<LinkId>> {
        self.closing.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_link_announces_it() {
        let (shared, events) = Shared::new();
        let link = shared.open();

        assert!(matches!(events.recv(), Ok(Event::Up(up)) if up == link));
    }

    #[test]
    fn identifiers_are_not_reused_while_the_radio_runs() {
        // Two links with the same identifier means one peer's writes go to the
        // other. The loopback transport had exactly this bug, from two
        // counters instead of one.
        let (shared, _events) = Shared::new();
        let first = shared.open();
        let second = shared.open();
        shared.closed(first);
        let third = shared.open();

        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);
    }

    #[test]
    fn bytes_queue_in_order_and_come_back_in_order() {
        let (shared, _events) = Shared::new();
        let link = shared.open();

        assert!(shared.queue(link, b"one".to_vec()));
        assert!(shared.queue(link, b"two".to_vec()));

        assert_eq!(shared.take(link), Some(b"one".to_vec()));
        assert_eq!(shared.take(link), Some(b"two".to_vec()));
        assert_eq!(shared.take(link), None);
    }

    #[test]
    fn a_short_write_keeps_its_tail_at_the_front() {
        // L2CAP writes are allowed to be short. Dropping the tail desynchronises
        // the peer's length prefix and the link is torn down for what looks
        // like corruption.
        let (shared, _events) = Shared::new();
        let link = shared.open();
        shared.queue(link, b"frame".to_vec());
        shared.queue(link, b"next".to_vec());

        let chunk = shared.take(link).expect("queued");
        shared.put_back(link, chunk[2..].to_vec());

        assert_eq!(shared.take(link), Some(b"ame".to_vec()));
        assert_eq!(shared.take(link), Some(b"next".to_vec()));
    }

    #[test]
    fn writing_to_a_closed_link_is_refused_rather_than_buffered() {
        // Otherwise a peer that walked away accumulates a queue nobody drains.
        let (shared, _events) = Shared::new();
        let link = shared.open();
        shared.closed(link);

        assert!(!shared.queue(link, b"gone".to_vec()));
        assert!(shared.pending().is_empty());
    }

    #[test]
    fn closing_a_link_twice_announces_it_once() {
        // The peer disconnecting and the stream erroring are two paths to the
        // same event, and the engine must not see a link go down twice.
        let (shared, events) = Shared::new();
        let link = shared.open();
        let _ = events.recv();

        shared.closed(link);
        shared.closed(link);

        assert!(matches!(events.recv(), Ok(Event::Down(_))));
        assert!(events.try_recv().is_err(), "a link went down twice");
    }

    #[test]
    fn a_stopped_radio_accepts_nothing_further() {
        let (shared, _events) = Shared::new();
        let link = shared.open();
        shared.stop();

        assert!(!shared.queue(link, b"after".to_vec()));
        assert!(shared.is_stopped());
    }

    #[test]
    fn only_links_with_something_to_send_are_pending() {
        let (shared, _events) = Shared::new();
        let quiet = shared.open();
        let busy = shared.open();
        shared.queue(busy, b"x".to_vec());

        assert_eq!(shared.pending(), vec![busy]);
        let _ = quiet;
    }

    #[test]
    fn close_requests_are_drained_once() {
        let (shared, _events) = Shared::new();
        shared.close(7);
        shared.close(9);

        assert_eq!(shared.drain_closing(), vec![7, 9]);
        assert!(shared.drain_closing().is_empty());
    }
}
