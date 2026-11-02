//! Connected clients and their outbound queues. Everything a client receives (responses, events,
//! PTY frames) goes through one ordered queue, so an `attached` response always precedes the replay.

use lotse_proto::{Event, Frame, Message};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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

#[derive(Default)]
pub struct Hub {
    clients: Mutex<HashMap<u64, Arc<ClientTx>>>,
    next_id: AtomicU64,
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

    /// Push an event to every connected client.
    pub fn broadcast(&self, event: Event) {
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
