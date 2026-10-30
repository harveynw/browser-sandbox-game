//! Bounded application boundary, polled and flushed by the game loop.
//! Dependency-internal queues remain unbounded/opaque; see docs/networking.md.
use crate::replication::{Budget, LIFECYCLE_RESERVE, SnapshotBuffer};
use protocol::{ClientEnvelope, Delivery, ServerEnvelope, decode_client, encode_server};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Instant,
};

const MAX_PEERS: usize = 16;
const RELIABLE_MESSAGES: usize = 256;
const RELIABLE_BYTES: usize = 512 * 1024;
const CONTROLS: usize = 128;
const ACTIONS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PeerKey {
    /// Numeric connection ID assigned by the net crate; it can be reused.
    pub id: u32,
    /// Application-assigned incarnation, protecting against stale peer operations.
    pub generation: u64,
}
#[derive(Debug)]
pub enum TransportEvent {
    /// A newly admitted transport connection, before game session admission.
    Opened(PeerKey),
    /// A decoded client envelope associated with this connection incarnation.
    Message {
        peer: PeerKey,
        message: ClientEnvelope,
    },
    /// Removal of this incarnation; the world uses it to clean up its session.
    Closed { peer: PeerKey, reason: &'static str },
}
#[derive(Debug, PartialEq, Eq)]
pub enum SendOutcome {
    Queued,
    ReplacedStale,
    Full,
    Closed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum UnreliableStream {
    // Latest owner state, distinct from public snapshots and diagnostic replies.
    Owner,
    // Snapshots use a per-entity buffer instead of the latest-stream map.
    Snapshot,
    Pong,
    // Other unreliable messages share a latest-only slot.
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
    // Incarnation paired with the map's numeric connection ID.
    generation: u64,
    // Decoded client messages waiting for the world, bounded by message category.
    ingress: VecDeque<ClientEnvelope>,
    // Ordered encoded messages awaiting reliable delivery and their total byte count.
    reliable: VecDeque<Vec<u8>>,
    reliable_bytes: usize,
    // Latest pending snapshot records, coalesced per entity and visibility.
    snapshots: SnapshotBuffer,
    // Independent fixed streams; snapshots are coalesced per entity above.
    latest: BTreeMap<UnreliableStream, Vec<u8>>,
    // Stops new traffic; pending reliable messages are submitted before the kill.
    close: Option<&'static str>,
    // Per-peer budget for encoded bytes submitted to the network.
    wire_budget: Budget,
}
/// Networking handles, peer state and bounded queues owned by the game loop.
pub struct Network {
    // Present for live networking; socket-free unit tests construct this without a server.
    server: Option<net::Server>,
    // Incoming notifications from the net crate's actor.
    events: net::EventQueue,
    // Monotonic time base for per-peer output budgets.
    origin: Instant,
    // Live connection IDs and their current application-side state.
    peers: BTreeMap<u32, Peer>,
    // Peer opens and closes waiting for the world, including their generations.
    lifecycle: VecDeque<TransportEvent>,
    // Monotonically increasing source of connection incarnations.
    generation: u64,
    // Set when a terminal lifecycle event cannot be recorded or networking fails.
    failed: bool,
    // Counts local peer removals by reason for diagnostics.
    closed_reasons: BTreeMap<&'static str, u64>,
}
impl Network {
    #[cfg(test)]
    pub(crate) fn test_event(&mut self, event: TransportEvent) {
        match event {
            TransportEvent::Opened(key) => {
                self.peers.insert(
                    key.id,
                    Peer {
                        generation: key.generation,
                        ..Peer::default()
                    },
                );
                self.lifecycle.push_back(TransportEvent::Opened(key));
            }
            TransportEvent::Closed { peer, reason } => {
                if !self.remove(peer, reason) {
                    self.lifecycle
                        .push_back(TransportEvent::Closed { peer, reason });
                }
            }
            TransportEvent::Message { peer, message } => {
                if self.peer(peer).is_some() {
                    self.receive(peer.id, &protocol::encode_client(&message).unwrap());
                } else {
                    // Exercise the world's handling of unknown or stale events.
                    self.lifecycle
                        .push_back(TransportEvent::Message { peer, message });
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn test_fill_reliable(&mut self, key: PeerKey) {
        let peer = self.peer(key).expect("live peer");
        let message = crate::world::test_envelope(
            "diagnostic",
            Some("session"),
            "pong",
            serde_json::json!({"nonce":0,"client_time_ms":0,"server_tick":0,"server_time_ms":0}),
        );
        let bytes = encode_server(&message, Delivery::Reliable).unwrap();
        while peer.reliable.len() < RELIABLE_MESSAGES {
            peer.reliable_bytes += bytes.len();
            peer.reliable.push_back(bytes.clone());
        }
    }

    #[cfg(test)]
    pub(crate) fn test_flush(
        &mut self,
    ) -> (Vec<(PeerKey, Delivery, ServerEnvelope)>, Vec<PeerKey>) {
        let mut sent = Vec::new();
        let mut closed = Vec::new();
        for (&id, peer) in &mut self.peers {
            let key = PeerKey {
                id,
                generation: peer.generation,
            };
            let mut record = |delivery, bytes: Vec<u8>| {
                let message = protocol::decode_server(&bytes, delivery).unwrap();
                sent.push((key, delivery, message));
            };
            for bytes in peer.reliable.drain(..) {
                record(Delivery::Reliable, bytes);
            }
            peer.reliable_bytes = 0;
            for bytes in peer.snapshots.drain().unwrap() {
                record(Delivery::Unreliable, bytes);
            }
            for bytes in std::mem::take(&mut peer.latest).into_values() {
                record(Delivery::Unreliable, bytes);
            }
            if peer.close.is_some() {
                closed.push(key);
            }
        }
        for key in &closed {
            let reason = self.peer(*key).unwrap().close.unwrap();
            self.remove(*key, reason);
        }
        (sent, closed)
    }

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
        *self.closed_reasons.entry(reason).or_default() += 1;
        eprintln!("Local peer {} closed: {reason}", key.id);
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

impl Network {
    pub fn start(address: &str) -> std::io::Result<Self> {
        // Detect ordinary bind errors synchronously; the net crate still has a bind race.
        let listener = std::net::TcpListener::bind(address)?;
        let webrtc_host = listener.local_addr()?.ip();
        drop(listener);
        let (server, events) = net::Server::new(address, webrtc_host);
        Ok(Self {
            server: Some(server),
            events,
            origin: Instant::now(),
            peers: BTreeMap::new(),
            lifecycle: VecDeque::new(),
            generation: 0,
            failed: false,
            closed_reasons: BTreeMap::new(),
        })
    }

    /// Consume networking notifications before the world processes its next step.
    pub fn poll(&mut self) {
        let mut kills = Vec::new();
        for event in self.events.pop_all() {
            match event {
                net::Event::Open(id) => {
                    if !self.open(id) {
                        kills.push(id);
                    }
                }
                net::Event::Received(id, bytes) => self.receive(id, &bytes),
                net::Event::Closed(id) => {
                    if let Some(p) = self.peers.get(&id) {
                        let key = PeerKey {
                            id,
                            generation: p.generation,
                        };
                        self.remove(key, "remote closed");
                    }
                }
            }
        }
        // Rejected connections are not present in the peer queues.
        self.submit(Vec::new(), kills);
    }

    /// Submit bounded pending output and close requests after the world has stepped.
    pub fn flush(&mut self) {
        let now = self.origin.elapsed().as_millis() as u64;
        let mut kills = Vec::new();
        let mut sends = Vec::new();
        let q = &mut *self;
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
                            if p.wire_budget.allow(bytes.len(), now, LIFECYCLE_RESERVE) {
                                sends.push((key.id, Delivery::Unreliable, bytes));
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
        if !self.failed {
            self.submit(sends, kills);
        }
    }

    fn submit(&mut self, sends: Vec<(u32, Delivery, Vec<u8>)>, kills: Vec<u32>) {
        // The dependency uses blocking_send; a stalled actor can stall the game loop.
        if sends.is_empty() && kills.is_empty() {
            return;
        }
        let server = self.server.as_mut().expect("live network server");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for (id, delivery, bytes) in sends {
                match delivery {
                    Delivery::Reliable => server.send_reliable(id, bytes::Bytes::from(bytes)),
                    Delivery::Unreliable => server.send_unreliable(id, bytes::Bytes::from(bytes)),
                }
            }
            // Reliable Reject/Disconnect must enter the actor FIFO before kill.
            for id in kills {
                server.kill(id);
            }
        }));
        if result.is_err() {
            self.failed = true;
        }
    }

    /// Application boundary only; dependency-internal queues are opaque.
    pub fn queue_metrics(&self) -> serde_json::Value {
        let q = self;
        serde_json::json!({"peers":q.peers.len(),"lifecycle":q.lifecycle.len(),"ingress_messages":q.peers.values().map(|p|p.ingress.len()).sum::<usize>(),"reliable_messages":q.peers.values().map(|p|p.reliable.len()).sum::<usize>(),"reliable_bytes":q.peers.values().map(|p|p.reliable_bytes).sum::<usize>(),"snapshot_records":q.peers.values().map(|p|p.snapshots.pending_records()).sum::<usize>(),"latest_streams":q.peers.values().map(|p|p.latest.len()).sum::<usize>(),"failed":q.failed,"closed_reasons":q.closed_reasons})
    }
    /// Reports observed boundary failures; the net crate exposes no actor health check.
    pub fn healthy(&self) -> bool {
        !self.failed
    }

    pub fn drain(&mut self, max: usize) -> Vec<TransportEvent> {
        let q = self;
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
    pub fn send(
        &mut self,
        key: PeerKey,
        delivery: Delivery,
        message: ServerEnvelope,
    ) -> SendOutcome {
        let Ok(bytes) = encode_server(&message, delivery) else {
            self.disconnect(key, "invalid output");
            return SendOutcome::Full;
        };
        let q = self;
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
    pub fn disconnect(&mut self, key: PeerKey, reason: &'static str) {
        // Admission stops immediately; flush issues the eventual kill. Duplicate requests are harmless.
        if let Some(p) = self.peer(key) {
            p.close.get_or_insert(reason);
        }
    }
}
// Unit tests exercise the real queues without opening sockets or starting the net actor.
#[cfg(test)]
impl Default for Network {
    fn default() -> Self {
        Self {
            server: None,
            events: net::EventQueue::default(),
            origin: Instant::now(),
            peers: BTreeMap::new(),
            lifecycle: VecDeque::new(),
            generation: 0,
            failed: false,
            closed_reasons: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn boundary() -> (Network, PeerKey) {
        let mut q = Network::default();
        assert!(q.open(0));
        let key = PeerKey {
            id: 0,
            generation: q.generation,
        };
        (q, key)
    }
    fn pong(nonce: u32) -> ServerEnvelope {
        crate::world::test_envelope(
            "diagnostic",
            Some("session"),
            "pong",
            serde_json::json!({"nonce":nonce,"client_time_ms":0,"server_tick":0,"server_time_ms":0}),
        )
    }
    // Checks that reliable queue overflow marks the peer for closure, while newer unreliable messages replace stale ones.
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
        assert!(transport.peer(key).unwrap().reliable_bytes <= RELIABLE_BYTES);
    }
    // Checks incoming queue limits and rejects sends using an old connection generation after reconnect.
    #[test]
    fn stalled_ingress_is_bounded_and_stale_sends_cannot_reach_reconnect() {
        let (mut transport, key) = boundary();
        let bytes = protocol::encode_client(&serde_json::from_value(serde_json::json!({"v":3,"world_id":"diagnostic","session_id":"session","message":{"kind":"ping","data":{"nonce":1,"client_time_ms":0}}})).unwrap()).unwrap();
        {
            let q = &mut transport;
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
    // Checks that pre-open data is ignored, duplicate opens/closes are harmless, and reused IDs receive a fresh generation.
    #[test]
    fn preopen_duplicate_close_and_recycled_ids() {
        let mut q = Network::default();
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
    // Checks that the lifecycle queue stops at 32 events and rejects further connections.
    #[test]
    fn stalled_lifecycle_fails_without_unbounded_growth() {
        let mut q = Network::default();
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
    // Checks that malformed input marks the peer for closure without queuing the payload.
    #[test]
    fn invalid_packet_closes_without_retaining_payload() {
        let mut q = Network::default();
        q.open(0);
        q.receive(0, &[0]);
        assert_eq!(q.peers[&0].close, Some("invalid protocol"));
        assert!(q.peers[&0].ingress.is_empty());
    }
    // Checks that owner updates and Pong replies occupy separate unreliable slots.
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
        let queues = &mut transport;
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
    // Checks that split snapshots preserve every entity and a new baseline clears pending snapshot state.
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
        let queues = &mut transport;
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
                .peer(key)
                .unwrap()
                .snapshots
                .drain()
                .unwrap()
                .is_empty()
        );
    }
    // Checks that Pongs collapse to the latest reply while distinct snapshot records survive for measuring packet delivery.
    #[test]
    fn loss_probes_need_independent_records_instead_of_the_latest_pong_stream() {
        let (mut transport, key) = boundary();
        let state = protocol::EntityState::Character(protocol::CharacterState {
            motor: sim_core::spawn_character([0.0, 0.0, 0.0], 0.0).unwrap(),
            health: protocol::CharacterHealth(100),
            mode: protocol::CharacterMode::OnFoot,
            vehicle_id: None,
        });
        for id in 1..=50 {
            assert_eq!(
                transport.send(key, Delivery::Unreliable, pong(id)),
                if id == 1 {
                    SendOutcome::Queued
                } else {
                    SendOutcome::ReplacedStale
                }
            );
            let packet = crate::replication::snapshot(
                "diagnostic",
                "session",
                0,
                vec![protocol::StateRecord {
                    entity_id: protocol::EntityId(id),
                    visibility_id: protocol::VisibilityId(1),
                    state: state.clone(),
                }],
            );
            assert_eq!(
                transport.send(key, Delivery::Unreliable, packet),
                SendOutcome::Queued
            );
        }
        let q = &mut transport;
        let p = q.peer(key).unwrap();
        assert_eq!(p.latest.len(), 1);
        let last =
            protocol::decode_server(&p.latest[&UnreliableStream::Pong], Delivery::Unreliable)
                .unwrap();
        assert_eq!(last, pong(50));
        let mut ids = std::collections::BTreeSet::new();
        for packet in p.snapshots.drain().unwrap() {
            assert!(packet.len() <= protocol::limits::UNRELIABLE_BYTES);
            let message = protocol::decode_server(&packet, Delivery::Unreliable).unwrap();
            let protocol::ServerMessage::Snapshot(snapshot) = message.message else {
                panic!("snapshot")
            };
            ids.extend(snapshot.records.into_iter().map(|r| r.entity_id.0));
        }
        assert_eq!(ids, (1..=50).collect());
    }
}
