//! Bounded application boundary. Only the pump thread touches net::Server.
//! Dependency-internal queues remain unbounded/opaque; see docs/04.
use crate::replication::{Budget, SnapshotBuffer};
use protocol::{ClientEnvelope, Delivery, ServerEnvelope, decode_client, encode_server};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

const MAX_PEERS: usize = 16;
const RELIABLE_MESSAGES: usize = 256;
const RELIABLE_BYTES: usize = 512 * 1024;
const CONTROLS: usize = 128;
const ACTIONS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PeerKey {
    pub id: u32,
    pub generation: u64,
}
#[derive(Debug)]
pub enum TransportEvent {
    Opened(PeerKey),
    Message {
        peer: PeerKey,
        message: ClientEnvelope,
    },
    Closed {
        peer: PeerKey,
        reason: &'static str,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub enum SendOutcome {
    Queued,
    ReplacedStale,
    Full,
    Closed,
}
pub trait GameTransport {
    fn drain(&mut self, max: usize) -> Vec<TransportEvent>;
    fn send(&mut self, peer: PeerKey, delivery: Delivery, message: ServerEnvelope) -> SendOutcome;
    fn disconnect(&mut self, peer: PeerKey, reason: &'static str);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum UnreliableStream {
    Owner,
    Snapshot,
    Pong,
    Cosmetic,
}
fn stream(message: &protocol::ServerMessage) -> UnreliableStream {
    match message {
        protocol::ServerMessage::OwnerState(_) => UnreliableStream::Owner,
        protocol::ServerMessage::Snapshot(_) => UnreliableStream::Snapshot,
        protocol::ServerMessage::Pong(_) => UnreliableStream::Pong,
        _ => UnreliableStream::Cosmetic,
    }
}
#[derive(Default)]
struct Peer {
    generation: u64,
    ingress: VecDeque<ClientEnvelope>,
    reliable: VecDeque<Vec<u8>>,
    reliable_bytes: usize,
    snapshots: SnapshotBuffer,
    // Independent fixed streams; snapshots are coalesced per entity above.
    latest: BTreeMap<UnreliableStream, Vec<u8>>,
    close: Option<&'static str>,
    wire_budget: Budget,
}
#[derive(Default)]
struct Queues {
    peers: BTreeMap<u32, Peer>,
    lifecycle: VecDeque<TransportEvent>,
    generation: u64,
    failed: bool,
}
impl Queues {
    fn open(&mut self, id: u32) -> bool {
        if self.peers.contains_key(&id)
            || self.peers.len() >= MAX_PEERS
            || self.lifecycle.len() >= 32
        {
            return false;
        }
        self.generation = self
            .generation
            .checked_add(1)
            .expect("connection generation exhausted");
        let generation = self.generation;
        self.peers.insert(
            id,
            Peer {
                generation,
                ..Peer::default()
            },
        );
        self.lifecycle
            .push_back(TransportEvent::Opened(PeerKey { id, generation }));
        true
    }
    fn remove(&mut self, key: PeerKey, reason: &'static str) -> bool {
        if self.peer(key).is_none() {
            return false;
        }
        self.peers.remove(&key.id);
        // Never silently lose a terminal event: fail the boundary if consumer stalls.
        if self.lifecycle.len() >= 32 {
            self.failed = true;
        } else {
            self.lifecycle
                .push_back(TransportEvent::Closed { peer: key, reason });
        }
        true
    }
    fn peer(&mut self, key: PeerKey) -> Option<&mut Peer> {
        self.peers
            .get_mut(&key.id)
            .filter(|p| p.generation == key.generation)
    }
    fn receive(&mut self, id: u32, bytes: &[u8]) {
        let Some(peer) = self.peers.get_mut(&id) else {
            return;
        }; // Discard pre-open data.
        if peer.close.is_some() {
            return;
        }
        match decode_client(bytes) {
            Ok(message) => {
                let action = matches!(message.message, protocol::ClientMessage::Action(_));
                let count = peer
                    .ingress
                    .iter()
                    .filter(|m| matches!(m.message, protocol::ClientMessage::Action(_)) == action)
                    .count();
                if count >= if action { ACTIONS } else { CONTROLS } {
                    peer.close = Some("ingress full");
                } else {
                    peer.ingress.push_back(message);
                }
            }
            Err(_) => peer.close = Some("invalid protocol"),
        }
    }
}

pub struct NetCrateTransport {
    queues: Arc<Mutex<Queues>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl NetCrateTransport {
    pub fn start(address: &str) -> std::io::Result<Self> {
        // Detect ordinary bind errors synchronously. The crate still has a bind race;
        // only the end-to-end browser probe establishes actual RTC readiness.
        drop(std::net::TcpListener::bind(address)?);
        let queues = Arc::new(Mutex::new(Queues::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let shared = queues.clone();
        let stopping = stop.clone();
        let address = address.to_owned();
        let worker = thread::Builder::new()
            .name("net-pump".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| {
                    let (mut server, mut events) = net::Server::new(&address);
                    let origin = std::time::Instant::now();
                    while !stopping.load(Ordering::Relaxed) {
                        let now = origin.elapsed().as_millis() as u64;
                        // Do not hold our mutex across dependency blocking_send calls.
                        let mut kills = Vec::new();
                        let mut sends = Vec::new();
                        {
                            let mut q = shared.lock().expect("queue lock");
                            for event in events.pop_all() {
                                match event {
                                    net::Event::Open(id) => {
                                        if !q.open(id) {
                                            kills.push(id);
                                        }
                                    }
                                    net::Event::Received(id, bytes) => q.receive(id, &bytes),
                                    net::Event::Closed(id) => {
                                        if let Some(p) = q.peers.get(&id) {
                                            let key = PeerKey {
                                                id,
                                                generation: p.generation,
                                            };
                                            q.remove(key, "remote closed");
                                        }
                                    }
                                }
                            }
                            let keys: Vec<_> = q
                                .peers
                                .iter()
                                .map(|(&id, p)| PeerKey {
                                    id,
                                    generation: p.generation,
                                })
                                .collect();
                            for key in keys {
                                let p = q.peer(key).expect("existing peer");
                                // Fair bounded work per peer, not an unbounded FIFO drain.
                                for _ in 0..16 {
                                    let Some(front) = p.reliable.front() else {
                                        break;
                                    };
                                    if !p.wire_budget.allow(front.len(), now, 0) {
                                        break;
                                    }
                                    let Some(bytes) = p.reliable.pop_front() else {
                                        break;
                                    };
                                    p.reliable_bytes -= bytes.len();
                                    sends.push((key.id, Delivery::Reliable, bytes));
                                }
                                if let Some(reason) = p.close {
                                    p.latest.clear();
                                    p.snapshots.clear();
                                    if p.reliable.is_empty() {
                                        q.remove(key, reason);
                                        kills.push(key.id);
                                    }
                                } else {
                                    if let Some(bytes) = p.latest.remove(&UnreliableStream::Owner)
                                        && p.wire_budget.allow(bytes.len(), now, 0)
                                    {
                                        sends.push((key.id, Delivery::Unreliable, bytes));
                                    }
                                    match p.snapshots.drain() {
                                        Ok(packets) => {
                                            for bytes in packets {
                                                if p.wire_budget.allow(bytes.len(), now, 1100) {
                                                    sends.push((
                                                        key.id,
                                                        Delivery::Unreliable,
                                                        bytes,
                                                    ));
                                                }
                                            }
                                        }
                                        Err(_) => {
                                            p.close = Some("invalid snapshot queue");
                                        }
                                    }
                                    for (_, bytes) in std::mem::take(&mut p.latest) {
                                        if p.wire_budget.allow(bytes.len(), now, 0) {
                                            sends.push((key.id, Delivery::Unreliable, bytes));
                                        }
                                    }
                                }
                            }
                            if q.failed {
                                break;
                            }
                        }
                        for (id, delivery, bytes) in sends {
                            match delivery {
                                Delivery::Reliable => {
                                    server.send_reliable(id, bytes::Bytes::from(bytes))
                                }
                                Delivery::Unreliable => {
                                    server.send_unreliable(id, bytes::Bytes::from(bytes))
                                }
                            }
                        }
                        // Reliable Reject/Disconnect must enter the actor FIFO before kill.
                        for id in kills {
                            server.kill(id);
                        }
                        thread::sleep(Duration::from_millis(2));
                    }
                    // Dropping Server shuts its actor down. No dependency join API exists.
                });
                if result.is_err() {
                    shared.lock().expect("queue lock").failed = true;
                }
            })?;
        Ok(Self {
            queues,
            stop,
            worker: Some(worker),
        })
    }
    pub fn healthy(&self) -> bool {
        !self.queues.lock().expect("queue lock").failed
            && self.worker.as_ref().is_some_and(|h| !h.is_finished())
    }
}
impl GameTransport for NetCrateTransport {
    fn drain(&mut self, max: usize) -> Vec<TransportEvent> {
        let mut q = self.queues.lock().expect("queue lock");
        let mut out = Vec::new();
        while out.len() < max {
            let Some(event) = q.lifecycle.pop_front() else {
                break;
            };
            out.push(event);
        }
        // Round-robin one message/peer/pass prevents one noisy peer monopolizing drain.
        while out.len() < max {
            let before = out.len();
            for (&id, p) in &mut q.peers {
                if out.len() == max {
                    break;
                }
                if p.close.is_none()
                    && let Some(message) = p.ingress.pop_front()
                {
                    out.push(TransportEvent::Message {
                        peer: PeerKey {
                            id,
                            generation: p.generation,
                        },
                        message,
                    });
                }
            }
            if out.len() == before {
                break;
            }
        }
        out
    }
    fn send(&mut self, key: PeerKey, delivery: Delivery, message: ServerEnvelope) -> SendOutcome {
        let Ok(bytes) = encode_server(&message, delivery) else {
            self.disconnect(key, "invalid output");
            return SendOutcome::Full;
        };
        let mut q = self.queues.lock().expect("queue lock");
        let Some(p) = q.peer(key).filter(|p| p.close.is_none()) else {
            return SendOutcome::Closed;
        };
        match delivery {
            Delivery::Reliable => {
                match &message.message {
                    protocol::ServerMessage::BaselineBegin(_)
                    | protocol::ServerMessage::ControlChanged(_) => {
                        p.snapshots.clear();
                        p.latest.remove(&UnreliableStream::Owner);
                    }
                    protocol::ServerMessage::EntityHide(hide) => {
                        p.snapshots.hide(hide.entity_id.0, hide.visibility_id.0)
                    }
                    _ => {}
                }
                if p.reliable.len() >= RELIABLE_MESSAGES
                    || p.reliable_bytes + bytes.len() > RELIABLE_BYTES
                {
                    p.close = Some("reliable output full");
                    return SendOutcome::Full;
                }
                p.reliable_bytes += bytes.len();
                p.reliable.push_back(bytes);
                SendOutcome::Queued
            }
            Delivery::Unreliable => {
                if matches!(message.message, protocol::ServerMessage::Snapshot(_)) {
                    return match p.snapshots.push(&message) {
                        Ok(true) => SendOutcome::ReplacedStale,
                        Ok(false) => SendOutcome::Queued,
                        Err(_) => {
                            p.close = Some("snapshot queue full");
                            SendOutcome::Full
                        }
                    };
                }
                if p.latest.insert(stream(&message.message), bytes).is_some() {
                    SendOutcome::ReplacedStale
                } else {
                    SendOutcome::Queued
                }
            }
        }
    }
    fn disconnect(&mut self, key: PeerKey, reason: &'static str) {
        // Admission stops immediately; pump owns the eventual kill. Duplicate requests harmless.
        if let Some(p) = self.queues.lock().expect("queue lock").peer(key) {
            p.close.get_or_insert(reason);
        }
    }
}
impl Drop for NetCrateTransport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // A dependency blocking_send can stall indefinitely. Never block the host's drop.
        if self.worker.as_ref().is_some_and(|h| h.is_finished()) {
            let _ = self.worker.take().expect("worker").join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn boundary() -> (NetCrateTransport, PeerKey) {
        let mut q = Queues::default();
        assert!(q.open(0));
        let key = PeerKey {
            id: 0,
            generation: q.generation,
        };
        (
            NetCrateTransport {
                queues: Arc::new(Mutex::new(q)),
                stop: Arc::new(AtomicBool::new(false)),
                worker: None,
            },
            key,
        )
    }
    fn pong(nonce: u32) -> ServerEnvelope {
        crate::world::test_envelope(
            "diagnostic",
            Some("session"),
            "pong",
            serde_json::json!({"nonce":nonce,"client_time_ms":0,"server_tick":0,"server_time_ms":0}),
        )
    }
    #[test]
    fn reliable_full_disconnects_and_unreliable_replaces_stale() {
        let (mut transport, key) = boundary();
        assert_eq!(
            transport.send(key, Delivery::Unreliable, pong(1)),
            SendOutcome::Queued
        );
        assert_eq!(
            transport.send(key, Delivery::Unreliable, pong(2)),
            SendOutcome::ReplacedStale
        );
        let bytes = transport
            .queues
            .lock()
            .unwrap()
            .peer(key)
            .unwrap()
            .latest
            .get(&UnreliableStream::Pong)
            .unwrap()
            .clone();
        assert_eq!(
            protocol::decode_server(&bytes, Delivery::Unreliable).unwrap(),
            pong(2)
        );
        for _ in 0..RELIABLE_MESSAGES {
            assert_eq!(
                transport.send(key, Delivery::Reliable, pong(3)),
                SendOutcome::Queued
            );
        }
        assert_eq!(
            transport.send(key, Delivery::Reliable, pong(4)),
            SendOutcome::Full
        );
        assert_eq!(
            transport.send(key, Delivery::Reliable, pong(5)),
            SendOutcome::Closed
        );
        assert!(
            transport
                .queues
                .lock()
                .unwrap()
                .peer(key)
                .unwrap()
                .reliable_bytes
                <= RELIABLE_BYTES
        );
    }
    #[test]
    fn stalled_ingress_is_bounded_and_stale_sends_cannot_reach_reconnect() {
        let (mut transport, key) = boundary();
        let bytes = protocol::encode_client(&serde_json::from_value(serde_json::json!({"v":1,"world_id":"diagnostic","session_id":"session","message":{"kind":"ping","data":{"nonce":1,"client_time_ms":0}}})).unwrap()).unwrap();
        {
            let mut q = transport.queues.lock().unwrap();
            for _ in 0..=CONTROLS {
                q.receive(0, &bytes);
            }
            assert_eq!(q.peer(key).unwrap().ingress.len(), CONTROLS);
            assert_eq!(q.peer(key).unwrap().close, Some("ingress full"));
            q.remove(key, "full");
            q.open(0);
        }
        assert_eq!(
            transport.send(key, Delivery::Reliable, pong(1)),
            SendOutcome::Closed
        );
    }
    #[test]
    fn preopen_duplicate_close_and_recycled_ids() {
        let mut q = Queues::default();
        q.receive(7, &[0]);
        assert!(q.peers.is_empty());
        assert!(q.open(7));
        let old = PeerKey {
            id: 7,
            generation: q.generation,
        };
        assert!(!q.open(7));
        assert!(q.remove(old, "test"));
        assert!(!q.remove(old, "duplicate"));
        assert!(q.open(7));
        assert!(q.peer(old).is_none());
        assert_eq!(q.lifecycle.len(), 3);
    }
    #[test]
    fn stalled_lifecycle_fails_without_unbounded_growth() {
        let mut q = Queues::default();
        for _ in 0..16 {
            assert!(q.open(0));
            let key = PeerKey {
                id: 0,
                generation: q.generation,
            };
            q.remove(key, "test");
        }
        assert_eq!(q.lifecycle.len(), 32);
        assert!(!q.open(0));
    }
    #[test]
    fn invalid_packet_closes_without_retaining_payload() {
        let mut q = Queues::default();
        q.open(0);
        q.receive(0, &[0]);
        assert_eq!(q.peers[&0].close, Some("invalid protocol"));
        assert!(q.peers[&0].ingress.is_empty());
    }
    #[test]
    fn owner_updates_cannot_overwrite_rtc_pongs_in_the_same_tick() {
        let (mut transport, key) = boundary();
        let fixture = protocol::fixtures::positive()
            .unwrap()
            .into_iter()
            .find(|f| {
                f.valid
                    && f.direction == "server"
                    && f.value
                        .as_ref()
                        .is_some_and(|v| v["message"]["kind"] == "owner_state")
            })
            .unwrap();
        let owner: ServerEnvelope = serde_json::from_value(fixture.value.unwrap()).unwrap();
        assert_eq!(
            transport.send(key, Delivery::Unreliable, pong(1)),
            SendOutcome::Queued
        );
        assert_eq!(
            transport.send(key, Delivery::Unreliable, owner.clone()),
            SendOutcome::Queued
        );
        assert_eq!(
            transport.send(key, Delivery::Unreliable, pong(2)),
            SendOutcome::ReplacedStale
        );
        let mut queues = transport.queues.lock().unwrap();
        let pending = std::mem::take(&mut queues.peer(key).unwrap().latest);
        assert_eq!(pending.len(), 2);
        assert_eq!(
            protocol::decode_server(&pending[&UnreliableStream::Pong], Delivery::Unreliable)
                .unwrap(),
            pong(2)
        );
        assert_eq!(
            protocol::decode_server(&pending[&UnreliableStream::Owner], Delivery::Unreliable)
                .unwrap(),
            owner
        );
    }
    #[test]
    fn split_snapshot_output_retains_every_entity_and_baseline_clears_stale_pending_state() {
        let (mut transport, key) = boundary();
        let state = protocol::EntityState::Character(protocol::CharacterState {
            motor: sim_core::spawn_character([0.0, 0.0, 0.0], 0.0).unwrap(),
            health: protocol::CharacterHealth(100),
            mode: protocol::CharacterMode::OnFoot,
            vehicle_id: None,
        });
        let records = (1..=16).map(|id| protocol::StateRecord {
            entity_id: protocol::EntityId(id),
            visibility_id: protocol::VisibilityId(1),
            state: state.clone(),
        });
        let packets = crate::replication::packets("world", "session", 3, records).unwrap();
        assert!(packets.len() > 1);
        for message in packets {
            assert_eq!(
                transport.send(key, Delivery::Unreliable, message),
                SendOutcome::Queued
            );
        }
        let mut queues = transport.queues.lock().unwrap();
        let bytes = queues.peer(key).unwrap().snapshots.drain().unwrap();
        let mut ids = std::collections::BTreeSet::new();
        for bytes in bytes {
            let message = protocol::decode_server(&bytes, Delivery::Unreliable).unwrap();
            let protocol::ServerMessage::Snapshot(s) = message.message else {
                panic!()
            };
            for r in s.records {
                ids.insert(r.entity_id.0);
            }
        }
        assert_eq!(ids.len(), 16);
        drop(queues);
        let message = crate::replication::snapshot(
            "world",
            "session",
            6,
            vec![protocol::StateRecord {
                entity_id: protocol::EntityId(1),
                visibility_id: protocol::VisibilityId(1),
                state,
            }],
        );
        transport.send(key, Delivery::Unreliable, message);
        let begin = crate::world::test_envelope(
            "world",
            Some("session"),
            "baseline_begin",
            serde_json::json!({"baseline_id":2,"server_tick":7}),
        );
        transport.send(key, Delivery::Reliable, begin);
        assert!(
            transport
                .queues
                .lock()
                .unwrap()
                .peer(key)
                .unwrap()
                .snapshots
                .drain()
                .unwrap()
                .is_empty()
        );
    }
}
