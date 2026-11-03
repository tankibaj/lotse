//! Connected clients and their outbound queues. Everything a client receives (responses, events,
//! PTY frames) goes through one ordered queue, so an `attached` response always precedes the replay.

use lotse_proto::{Event, Frame, Message, PermDecision, PermVia, SessionId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{Notify, mpsc};

/// Encoded frame, shared between clients.
pub type Bytes = Arc<[u8]>;

/// A client that falls this far behind is dropped instead of growing the daemon without bound.
const MAX_QUEUED_BYTES: usize = 64 * 1024 * 1024;

pub struct ClientTx {
    pub id: u64,
    /// `app`, `hook`, `cli`, … from `hello`.
    pub name: String,
    tx: mpsc::UnboundedSender<Bytes>,
    queued: AtomicUsize,
    kick: Notify,
}

impl ClientTx {
    /// Queue bytes; `false` when the client is gone or too slow (it is then disconnected).
    pub fn send_bytes(&self, bytes: Bytes) -> bool {
        let len = bytes.len();
        if self.queued.fetch_add(len, Ordering::AcqRel) + len > MAX_QUEUED_BYTES {
            self.queued.fetch_sub(len, Ordering::AcqRel);
            self.kick.notify_one();
            return false;
        }
        if self.tx.send(bytes).is_err() {
            self.queued.fetch_sub(len, Ordering::AcqRel);
            return false;
        }
        true
    }

    pub fn send_frame(&self, frame: &Frame) -> bool {
        match lotse_proto::encode(frame) {
            Ok(b) => self.send_bytes(b.into()),
            Err(e) => {
                tracing::warn!("cannot encode frame: {e}");
                false
            }
        }
    }

    /// Called by the connection's writer after the bytes hit the socket.
    pub fn written(&self, len: usize) {
        self.queued.fetch_sub(len, Ordering::AcqRel);
    }

    /// Bytes queued but not yet written to the socket.
    pub fn pending(&self) -> usize {
        self.queued.load(Ordering::Acquire)
    }

    /// Resolves when the client must be disconnected (queue overflow).
    pub async fn kicked(&self) {
        self.kick.notified().await;
    }

    pub fn kick(&self) {
        self.kick.notify_one();
    }
}

/// Sees every broadcast event, on the thread that broadcasts it: keep it quick, never block, and
/// never call back into the hub.
pub type Observer = Box<dyn Fn(&Event) + Send + Sync>;

/// A permission answer the daemon typed into a pane. In-process only: never sent to clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermDecisionRecord {
    pub session_id: SessionId,
    /// `PermRequest::id` of the prompt that was answered.
    pub prompt_id: Option<String>,
    pub decision: PermDecision,
    pub via: Option<PermVia>,
}

/// Same rules as [`Observer`].
pub type DecisionObserver = Box<dyn Fn(&PermDecisionRecord) + Send + Sync>;

#[derive(Default)]
pub struct Hub {
    clients: Mutex<HashMap<u64, Arc<ClientTx>>>,
    next_id: AtomicU64,
    observers: RwLock<Vec<Observer>>,
    decision_observers: RwLock<Vec<DecisionObserver>>,
}

impl Hub {
    pub fn register(&self, name: String) -> (Arc<ClientTx>, mpsc::UnboundedReceiver<Bytes>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let client = Arc::new(ClientTx {
            id: self.next_id.fetch_add(1, Ordering::Relaxed) + 1,
            name,
            tx,
            queued: AtomicUsize::new(0),
            kick: Notify::new(),
        });
        self.lock().insert(client.id, client.clone());
        (client, rx)
    }

    pub fn remove(&self, id: u64) {
        self.lock().remove(&id);
    }

    /// Register an in-process listener for every event (the history recorder). Lives as long as the hub.
    pub fn observe(&self, observer: Observer) {
        self.observers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(observer);
    }

    /// Register an in-process listener for permission answers (`perm_events.via`). Lives as long as the hub.
    pub fn observe_decisions(&self, observer: DecisionObserver) {
        self.decision_observers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(observer);
    }

    /// Tell the decision observers (not the clients) that a permission answer was typed.
    pub fn publish_decision(&self, record: &PermDecisionRecord) {
        for observer in self
            .decision_observers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            observer(record);
        }
    }

    /// Push an event to every connected client (and every observer, in registration order).
    pub fn broadcast(&self, event: Event) {
        for observer in self
            .observers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            observer(&event);
        }
        let Ok(bytes) = lotse_proto::encode(&Frame::Control(Message::Event(event))) else {
            return;
        };
        let bytes: Bytes = bytes.into();
        for c in self.lock().values() {
            c.send_bytes(bytes.clone());
        }
    }

    pub fn kick_all(&self) {
        for c in self.lock().values() {
            c.kick();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Arc<ClientTx>>> {
        self.clients.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::SessionId;

    fn closed(id: &str) -> Event {
        Event::SessionClosed {
            session_id: SessionId::new(id),
        }
    }

    #[test]
    fn every_observer_sees_every_event_in_order_even_without_clients() {
        let hub = Hub::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        for tag in ["a", "b"] {
            let seen = seen.clone();
            hub.observe(Box::new(move |e| {
                seen.lock().unwrap().push((tag, e.name()));
            }));
        }
        hub.broadcast(closed("s1"));
        hub.broadcast(Event::SettingsChanged(Default::default()));
        assert_eq!(
            *seen.lock().unwrap(),
            [
                ("a", "session_closed"),
                ("b", "session_closed"),
                ("a", "settings_changed"),
                ("b", "settings_changed")
            ]
        );
    }

    #[test]
    fn an_observer_does_not_disturb_the_clients() {
        let hub = Hub::default();
        hub.observe(Box::new(|_| {}));
        let (_client, mut rx) = hub.register("test".into());
        hub.broadcast(closed("s1"));
        assert!(rx.try_recv().is_ok(), "the client still gets the event");
    }
}
