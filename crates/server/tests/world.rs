use protocol::*;
use server::{admission::MemoryProfiles, content_loader::WorldContent, transport::*, world::*};
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
};
#[derive(Default)]
struct FakeTransport {
    events: VecDeque<TransportEvent>,
    sent: Vec<(PeerKey, Delivery, ServerEnvelope)>,
    closed: BTreeSet<PeerKey>,
    full: BTreeSet<PeerKey>,
}
impl GameTransport for FakeTransport {
    fn drain(&mut self, max: usize) -> Vec<TransportEvent> {
        (0..max).filter_map(|_| self.events.pop_front()).collect()
    }
    fn send(&mut self, peer: PeerKey, delivery: Delivery, message: ServerEnvelope) -> SendOutcome {
        // Exercise the real outbound codec/limits instead of accepting arbitrary fake DTOs.
        let bytes = encode_server(&message, delivery).unwrap();
        assert_eq!(decode_server(&bytes, delivery).unwrap(), message);
        if self.full.contains(&peer) {
            return SendOutcome::Full;
        }
        self.sent.push((peer, delivery, message));
        SendOutcome::Queued
    }
    fn disconnect(&mut self, peer: PeerKey, _: &'static str) {
        self.closed.insert(peer);
    }
}
fn content() -> WorldContent {
    WorldContent::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
    )
    .unwrap()
}
fn setup() -> (World, MemoryProfiles, FakeTransport) {
    (
        World::new(content()).unwrap(),
        MemoryProfiles::default(),
        FakeTransport::default(),
    )
}
fn peer(id: u32) -> PeerKey {
    PeerKey {
        id,
        generation: u64::from(id) + 1,
    }
}
fn step(w: &mut World, a: &mut MemoryProfiles, t: &mut FakeTransport) {
    w.step(t, a, u64::from(w.tick) * 34, true).unwrap();
}
fn send(w: &World, t: &mut FakeTransport, p: PeerKey, message: ClientMessage) {
    let sid = w
        .sessions
        .get(&p)
        .and_then(|s| s.id.as_ref())
        .map(|s| OpaqueId(s.clone()));
    t.events.push_back(TransportEvent::Message {
        peer: p,
        message: ClientEnvelope {
            v: ProtocolVersion(1),
            world_id: Some(OpaqueId(w.id.clone())),
            session_id: sid,
            message,
        },
    });
}
fn join(
    w: &mut World,
    a: &mut MemoryProfiles,
    t: &mut FakeTransport,
    p: PeerKey,
    cookie: Option<&str>,
) -> String {
    let (_, new) = a.session(cookie).unwrap();
    let cookie = new.or_else(|| cookie.map(str::to_owned)).unwrap();
    let ticket = a.issue(Some(&cookie), u64::from(w.tick) * 34).unwrap();
    t.events.push_back(TransportEvent::Opened(p));
    step(w, a, t);
    let build = w.build();
    send(
        w,
        t,
        p,
        ClientMessage::ClientHello(ClientHello {
            protocol_version: ProtocolVersion(1),
            build_id: build.build_id,
            content_hash: build.content_hash,
            sim_config_hash: build.sim_config_hash,
            join_ticket: OpaqueId(ticket),
            display_name: DisplayName("Test Guest".into()),
        }),
    );
    step(w, a, t);
    cookie
}
fn ready(w: &mut World, a: &mut MemoryProfiles, t: &mut FakeTransport, p: PeerKey) {
    let baseline_id = BaselineId(w.sessions[&p].baseline);
    send(
        w,
        t,
        p,
        ClientMessage::ClientReady(ClientReady { baseline_id }),
    );
    step(w, a, t);
}
fn batch(w: &World, t: &mut FakeTransport, p: PeerKey, tick: u32, x: i16, z: i16, epoch: u32) {
    send(
        w,
        t,
        p,
        ClientMessage::InputBatch(InputBatch {
            control_epoch: ControlEpoch(epoch),
            frames: vec![InputFrame {
                tick,
                move_x: InputAxis(x),
                move_z: InputAxis(z),
                look_yaw: Yaw(0.0),
                look_pitch: Pitch(0.0),
                buttons: Buttons(0),
            }],
        }),
    );
}
#[test]
fn loading_is_inert_and_ready_starts_once_with_ordered_baseline_and_status() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    let id = w.sessions[&p].player.unwrap().0;
    let initial = w.players[&id].state.clone();
    for _ in 0..5 {
        batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(w.players[&id].state, initial);
    assert_eq!(w.sessions[&p].phase, SessionPhase::Loading);
    send(
        &w,
        &mut t,
        p,
        ClientMessage::ClientReady(ClientReady {
            baseline_id: BaselineId(99),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions[&p].phase, SessionPhase::Loading);
    ready(&mut w, &mut a, &mut t, p);
    ready(&mut w, &mut a, &mut t, p);
    let messages: Vec<_> = t.sent.iter().map(|(_, _, m)| &m.message).collect();
    assert!(matches!(messages[0], ServerMessage::ServerHello(_)));
    assert!(matches!(messages[1], ServerMessage::Welcome(_)));
    assert!(matches!(messages[2], ServerMessage::BaselineBegin(_)));
    assert!(matches!(messages[3], ServerMessage::EntityShow(_)));
    assert!(
        messages[3..6]
            .iter()
            .all(|m| matches!(m, ServerMessage::EntityShow(_)))
    );
    assert!(matches!(messages[6], ServerMessage::BaselineEnd(_)));
    let starts: Vec<_> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(m, ServerMessage::StartPlay(_)))
        .collect();
    assert_eq!(starts.len(), 1);
    assert!(matches!(
        messages[starts[0].0 + 1],
        ServerMessage::PlayerStatus(_)
    ));
    assert_eq!(w.sessions[&p].phase, SessionPhase::Playing);
}
#[test]
fn duplicates_conflicts_stale_future_and_epoch_inputs_cannot_accelerate_the_actor() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let id = w.sessions[&p].player.unwrap().0;
    let before = w.players[&id].state.motor.clone();
    let tick = w.tick + 1;
    for _ in 0..10 {
        batch(&w, &mut t, p, tick, 0, 32767, 1);
    }
    batch(&w, &mut t, p, tick, 32767, 0, 1);
    batch(&w, &mut t, p, w.tick, 0, 32767, 1);
    batch(&w, &mut t, p, tick + 9, 0, 32767, 1);
    batch(&w, &mut t, p, tick, 0, 32767, 2);
    step(&mut w, &mut a, &mut t);
    let after = &w.players[&id].state.motor;
    assert!((after.position.0[0].0 - before.position.0[0].0).abs() < 0.001);
    assert!(before.position.0[2].0 - after.position.0[2].0 > 0.0);
    assert!(before.position.0[2].0 - after.position.0[2].0 < 0.05);
    assert_eq!(w.metrics.conflicting_inputs, 1);
    assert_eq!(w.metrics.late_inputs, 1);
    assert_eq!(w.metrics.future_inputs, 1);
    assert_eq!(w.metrics.stale_epochs, 1);
    assert!(w.sessions[&p].inputs.is_empty());
    for _ in 0..10 {
        step(&mut w, &mut a, &mut t);
    }
    assert!(
        w.players[&id]
            .state
            .motor
            .velocity
            .0
            .iter()
            .all(|v| v.0.abs() < 0.001)
    );
}
#[test]
fn future_ring_is_nine_ticks_and_one_frame_is_consumed_per_step() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let start = w.tick + 1;
    for tick in start..=start + 8 {
        batch(&w, &mut t, p, tick, 0, 32767, 1);
    }
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions[&p].inputs.len(), 8);
    for expected in (0..8).rev() {
        step(&mut w, &mut a, &mut t);
        assert_eq!(w.sessions[&p].inputs.len(), expected);
    }
}
#[test]
fn resync_increments_epoch_and_visibility_without_moving_until_new_readiness() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let id = w.sessions[&p].player.unwrap().0;
    batch(&w, &mut t, p, w.tick + 2, 0, 32767, 1);
    send(
        &w,
        &mut t,
        p,
        ClientMessage::ResyncRequest(ResyncRequest {
            reason: ResyncReason::Stale,
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions[&p].epoch, 2);
    assert_eq!(w.sessions[&p].baseline, 2);
    assert!(w.sessions[&p].inputs.is_empty());
    let position = w.players[&id].state.motor.position.clone();
    batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.players[&id].state.motor.position, position);
    let vis: Vec<_> = t
        .sent
        .iter()
        .filter_map(|(_, _, m)| {
            if let ServerMessage::EntityShow(s) = &m.message {
                Some(s.visibility_id.0)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(vis, vec![1, 2, 3, 4, 5, 6]);
    ready(&mut w, &mut a, &mut t, p);
    batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.metrics.stale_epochs, 1);
}
#[test]
fn guest_replacement_disconnect_cleanup_and_generation_reuse_are_idempotent() {
    let (mut w, mut a, mut t) = setup();
    let old = peer(0);
    let cookie = join(&mut w, &mut a, &mut t, old, None);
    ready(&mut w, &mut a, &mut t, old);
    let first = w.sessions[&old].player.unwrap();
    let profile = w.sessions[&old]
        .profile
        .as_ref()
        .unwrap()
        .profile_id
        .clone();
    let sid = w.sessions[&old].id.clone();
    let new = peer(1);
    join(&mut w, &mut a, &mut t, new, Some(&cookie));
    assert!(!w.sessions.contains_key(&old));
    assert!(t.closed.contains(&old));
    assert!(!w.players.contains_key(&first.0));
    assert_eq!(w.players.len(), 1);
    assert_eq!(
        w.sessions[&new].profile.as_ref().unwrap().profile_id,
        profile
    );
    assert_ne!(w.sessions[&new].id, sid);
    assert!(t.sent.iter().any(|(peer, _, m)| *peer == old
        && m.session_id.as_ref().map(|id| &id.0) == sid.as_ref()
        && matches!(m.message, ServerMessage::Disconnect(_))));
    assert!(w.sessions[&new].player.unwrap().0 > first.0);
    t.events.push_back(TransportEvent::Closed {
        peer: old,
        reason: "late close",
    });
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.players.len(), 1);
    send(&w, &mut t, new, ClientMessage::Leave(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert!(w.players.is_empty());
    assert!(w.sessions.is_empty());
    let reused = PeerKey {
        id: new.id,
        generation: 99,
    };
    join(&mut w, &mut a, &mut t, reused, Some(&cookie));
    t.events.push_back(TransportEvent::Closed {
        peer: new,
        reason: "old generation",
    });
    step(&mut w, &mut a, &mut t);
    assert!(w.sessions.contains_key(&reused));
}
#[test]
fn other_observers_receive_show_then_hide_and_reliable_full_removes_ownership() {
    let (mut w, mut a, mut t) = setup();
    let first = peer(0);
    let second = peer(1);
    join(&mut w, &mut a, &mut t, first, None);
    ready(&mut w, &mut a, &mut t, first);
    join(&mut w, &mut a, &mut t, second, None);
    let id = w.sessions[&second].player.unwrap();
    assert!(t.sent.iter().any(|(p,_,m)|*p==first && matches!(&m.message,ServerMessage::EntityShow(s) if s.descriptor.id==id && s.baseline_id.is_none())));
    t.full.insert(second);
    ready(&mut w, &mut a, &mut t, second);
    assert!(!w.sessions.contains_key(&second));
    assert!(!w.players.contains_key(&id.0));
    assert!(t.sent.iter().any(|(p, _, m)| *p == first
        && matches!(&m.message,ServerMessage::EntityHide(s) if s.entity_id==id)));
}
#[test]
fn loading_hello_and_playing_timeouts_remove_all_state() {
    for phase in [
        SessionPhase::AwaitHello,
        SessionPhase::Loading,
        SessionPhase::Playing,
    ] {
        let (mut w, mut a, mut t) = setup();
        let p = peer(0);
        if phase == SessionPhase::AwaitHello {
            t.events.push_back(TransportEvent::Opened(p));
            step(&mut w, &mut a, &mut t);
        } else {
            join(&mut w, &mut a, &mut t, p, None);
            if phase == SessionPhase::Playing {
                ready(&mut w, &mut a, &mut t, p);
            }
        }
        w.step(&mut t, &mut a, 40_000, true).unwrap();
        assert!(w.sessions.is_empty());
        assert!(w.players.is_empty());
        assert!(t.closed.contains(&p));
    }
}
#[test]
fn unknown_peers_wrong_scope_and_invalid_tickets_cannot_allocate_actors() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    send(&w, &mut t, p, ClientMessage::Leave(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert!(w.players.is_empty());
    t.events.push_back(TransportEvent::Opened(p));
    step(&mut w, &mut a, &mut t);
    let build = w.build();
    send(
        &w,
        &mut t,
        p,
        ClientMessage::ClientHello(ClientHello {
            protocol_version: ProtocolVersion(1),
            build_id: build.build_id,
            content_hash: build.content_hash,
            sim_config_hash: build.sim_config_hash,
            join_ticket: OpaqueId("bad-ticket".into()),
            display_name: DisplayName("Test Name".into()),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert!(w.players.is_empty());
    assert!(w.sessions.is_empty());
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::Reject(r) if r.code==RejectCode::InvalidTicket)));
    join(&mut w, &mut a, &mut t, peer(1), None);
    let p = peer(1);
    t.events.push_back(TransportEvent::Message {
        peer: p,
        message: ClientEnvelope {
            v: ProtocolVersion(1),
            world_id: Some(OpaqueId("wrong-boot".into())),
            session_id: w.sessions[&p].id.clone().map(OpaqueId),
            message: ClientMessage::Leave(Empty {}),
        },
    });
    step(&mut w, &mut a, &mut t);
    assert!(w.players.is_empty());
}
#[test]
fn actions_are_unavailable_and_duplicate_requests_return_the_cached_result() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let request = ClientMessage::Action(ActionRequest {
        request_id: RequestId(1),
        control_epoch: ControlEpoch(1),
        action: Action::Reload(Empty {}),
    });
    send(&w, &mut t, p, request.clone());
    step(&mut w, &mut a, &mut t);
    send(&w, &mut t, p, request);
    step(&mut w, &mut a, &mut t);
    let results: Vec<_> = t
        .sent
        .iter()
        .filter_map(|(_, _, m)| {
            if let ServerMessage::ActionResult(r) = &m.message {
                Some(r)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], results[1]);
    assert_eq!(results[0].code, ActionCode::Unavailable);
}

#[test]
fn loading_sessions_reserve_capacity_and_unknown_peers_cannot_allocate_a_seventeenth_player() {
    let (mut w, mut a, mut t) = setup();
    for id in 0..16 {
        join(&mut w, &mut a, &mut t, peer(id), None);
    }
    assert_eq!(w.players.len(), 16);
    assert_eq!(w.sessions.len(), 16);
    assert!(
        w.sessions
            .values()
            .all(|s| s.phase == SessionPhase::Loading)
    );
    t.events.push_back(TransportEvent::Opened(peer(16)));
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.players.len(), 16);
    assert!(t.closed.contains(&peer(16)));
    let positions: Vec<_> = w
        .players
        .values()
        .map(|p| p.state.motor.position.clone())
        .collect();
    for (i, p) in positions.iter().enumerate() {
        assert!(positions.iter().skip(i + 1).all(|other| other != p));
    }
}

#[test]
fn playing_observer_receives_split_movement_snapshots_and_fresh_visibility_after_aoi_return() {
    let (mut w, mut a, mut t) = setup();
    let observer = peer(0);
    join(&mut w, &mut a, &mut t, observer, None);
    ready(&mut w, &mut a, &mut t, observer);
    for id in 1..8 {
        join(&mut w, &mut a, &mut t, peer(id), None);
    }
    let mover = w.sessions[&peer(1)].player.unwrap().0;
    t.sent.clear();
    for _ in 0..3 {
        step(&mut w, &mut a, &mut t);
    }
    let snapshots: Vec<_> = t
        .sent
        .iter()
        .filter_map(|(p, d, m)| {
            if *p == observer && *d == Delivery::Unreliable {
                if let ServerMessage::Snapshot(s) = &m.message {
                    return Some(s);
                }
                None
            } else {
                None
            }
        })
        .collect();
    assert!(snapshots.len() > 1);
    let ids: BTreeSet<_> = snapshots
        .iter()
        .flat_map(|s| s.records.iter().map(|r| r.entity_id.0))
        .collect();
    assert_eq!(ids.len(), 7);
    let visibility = snapshots
        .iter()
        .flat_map(|s| &s.records)
        .find(|r| r.entity_id.0 == mover)
        .unwrap()
        .visibility_id
        .0;
    let origin = w.players[&w.sessions[&observer].player.unwrap().0]
        .state
        .motor
        .position
        .clone();
    w.players.get_mut(&mover).unwrap().state.motor.position.0[0].0 = origin.0[0].0 + 170.0;
    step(&mut w, &mut a, &mut t);
    assert!(!t.sent.iter().any(|(p, _, m)| *p == observer
        && matches!(&m.message, ServerMessage::EntityHide(h) if h.entity_id.0 == mover)));
    w.players.get_mut(&mover).unwrap().state.motor.position.0[0].0 = origin.0[0].0 + 200.0;
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(p, _, m)| *p == observer && matches!(&m.message, ServerMessage::EntityHide(h) if h.entity_id.0 == mover && h.reason == HideReason::OutOfRange)));
    w.players.get_mut(&mover).unwrap().state.motor.position = origin;
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(p, _, m)| *p == observer && matches!(&m.message, ServerMessage::EntityShow(s) if s.descriptor.id.0 == mover && s.visibility_id.0 > visibility)));
    assert!(w.metrics.max_snapshot_bytes <= 1100);
    assert!(w.metrics.snapshot_packets > 1);
    assert!(w.metrics.max_refresh_age_ticks <= 3);
}

#[test]
fn input_flood_cannot_expand_the_ring_or_keep_an_actor_after_rate_rejection() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    for _ in 0..61 {
        batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
    }
    step(&mut w, &mut a, &mut t);
    assert!(!w.sessions.contains_key(&p));
    assert!(w.players.is_empty());
    assert!(t.closed.contains(&p));
}

fn vehicle_action(
    w: &World,
    t: &mut FakeTransport,
    p: PeerKey,
    request: u32,
    epoch: u32,
    action: Action,
) {
    send(
        w,
        t,
        p,
        ClientMessage::Action(ActionRequest {
            request_id: RequestId(request),
            control_epoch: ControlEpoch(epoch),
            action,
        }),
    );
}
fn at_door(w: &mut World, p: PeerKey, car: u32) {
    let position = w.vehicles[&car].entry();
    let id = w.sessions[&p].player.unwrap().0;
    w.players.get_mut(&id).unwrap().state.motor = sim_core::spawn_character(position, 0.0).unwrap();
}
#[test]
fn simultaneous_entry_is_atomic_epoch_scoped_idempotent_and_disconnect_releases_car() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    let q = peer(2);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    join(&mut w, &mut a, &mut t, q, None);
    ready(&mut w, &mut a, &mut t, q);
    let car = *w.vehicles.keys().next().unwrap();
    at_door(&mut w, p, car);
    at_door(&mut w, q, car);
    batch(&w, &mut t, p, w.tick + 2, 0, 32767, 1);
    for contender in [q, p] {
        vehicle_action(
            &w,
            &mut t,
            contender,
            1,
            1,
            Action::EnterVehicle(EnterVehicle {
                vehicle_id: EntityId(car),
            }),
        );
    }
    step(&mut w, &mut a, &mut t);
    let driver = w.sessions[&p].player.unwrap();
    assert_eq!(w.vehicles[&car].state.driver_id, Some(driver));
    assert_eq!(w.players[&driver.0].state.mode, CharacterMode::Driving);
    assert_eq!(w.sessions[&p].epoch, 2);
    assert_eq!(w.sessions[&q].epoch, 1);
    assert!(w.sessions[&p].inputs.is_empty());
    assert!(t.sent.iter().any(|(target, _, m)| *target == q
        && matches!(&m.message,ServerMessage::ActionResult(v) if v.code==ActionCode::Occupied)));
    assert!(t.sent.iter().any(|(target,delivery,m)| *target==p && *delivery==Delivery::Reliable && matches!(&m.message,ServerMessage::ControlChanged(v) if v.controlled_entity_id.0==car && v.control_epoch.0==2)));
    // An exact duplicate returns its cached success without taking control again.
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(car),
        }),
    );
    batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions[&p].epoch, 2);
    assert_eq!(w.vehicles[&car].state.motor.speed.0, 0.0);
    for _ in 0..15 {
        batch(&w, &mut t, p, w.tick + 1, 0, 32767, 2);
        step(&mut w, &mut a, &mut t);
    }
    assert!(w.vehicles[&car].state.motor.speed.0 > 3.0);
    vehicle_action(&w, &mut t, p, 2, 2, Action::ExitVehicle(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions[&p].epoch, 2);
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ActionResult(v) if v.request_id.0==2 && v.code==ActionCode::TooFast)));
    w.disconnect(p, &mut a);
    assert_eq!(w.vehicles[&car].state.driver_id, None);
    assert_eq!(w.vehicles[&car].state.motor.speed.0, 0.0);
}
#[test]
fn exit_reclaims_player_with_fresh_epoch_and_old_car_inputs_cannot_move_it() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let car = *w.vehicles.keys().next().unwrap();
    at_door(&mut w, p, car);
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(car),
        }),
    );
    step(&mut w, &mut a, &mut t);
    vehicle_action(&w, &mut t, p, 2, 2, Action::ExitVehicle(Empty {}));
    step(&mut w, &mut a, &mut t);
    let id = w.sessions[&p].player.unwrap();
    assert_eq!(w.sessions[&p].epoch, 3);
    assert_eq!(w.players[&id.0].state.mode, CharacterMode::OnFoot);
    assert_eq!(w.vehicles[&car].state.driver_id, None);
    let position = w.players[&id.0].state.motor.position.clone();
    batch(&w, &mut t, p, w.tick + 1, 32767, 32767, 2);
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.players[&id.0].state.motor.position.0[0], position.0[0]);
    assert_eq!(w.players[&id.0].state.motor.position.0[2], position.0[2]);
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ControlChanged(v) if v.controlled_entity_id==id && v.control_epoch.0==3)));
}
#[test]
fn all_blocked_exit_candidates_preserve_occupancy_and_epoch() {
    let mut package = content();
    for (i, position) in [
        [-153.5, 1.0, 176.0],
        [-150.5, 1.0, 176.0],
        [-152.0, 1.0, 179.0],
    ]
    .into_iter()
    .enumerate()
    {
        package.collision.colliders.push(content::StaticCollider {
            id: 10000 + i as u32,
            shape: content::BoxShape::Box,
            role: content::ColliderRole::Building,
            center: position,
            half_extents: [0.45, 1.0, 0.4],
            yaw: 0.0,
            flags: 3,
        });
    }
    let mut w = World::new(package).unwrap();
    let mut a = MemoryProfiles::default();
    let mut t = FakeTransport::default();
    let p = peer(1);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let car = *w.vehicles.keys().next().unwrap();
    let id = w.sessions[&p].player.unwrap();
    w.vehicles.get_mut(&car).unwrap().state.driver_id = Some(id);
    let player = w.players.get_mut(&id.0).unwrap();
    player.state.mode = CharacterMode::Driving;
    player.state.vehicle_id = Some(EntityId(car));
    player.state.motor.position = Vec3(w.vehicles[&car].seat().map(Scalar));
    vehicle_action(&w, &mut t, p, 1, 1, Action::ExitVehicle(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions[&p].epoch, 1);
    assert_eq!(w.vehicles[&car].state.driver_id, Some(id));
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ActionResult(v) if v.code==ActionCode::ExitBlocked)));
}

fn courier_action(w: &World, t: &mut FakeTransport, p: PeerKey, request: u32) {
    vehicle_action(
        w,
        t,
        p,
        request,
        w.sessions[&p].epoch,
        Action::AcceptMission(AcceptMission {
            template_id: TemplateId("courier_quay_market".into()),
        }),
    );
}
fn place(w: &mut World, p: PeerKey, position: [f32; 3]) {
    let id = w.sessions[&p].player.unwrap().0;
    w.players.get_mut(&id).unwrap().state.motor = sim_core::spawn_character(position, 0.0).unwrap();
}
#[test]
fn courier_acceptance_is_private_scoped_cached_and_survives_resync() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    let q = peer(2);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    join(&mut w, &mut a, &mut t, q, None);
    ready(&mut w, &mut a, &mut t, q);
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    assert!(w.sessions[&p].mission.is_none());
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ActionResult(v) if v.code==ActionCode::OutOfRange)));
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    courier_action(&w, &mut t, p, 2);
    step(&mut w, &mut a, &mut t);
    let instance = w.sessions[&p].mission.as_ref().unwrap().id.clone();
    courier_action(&w, &mut t, p, 2);
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions[&p].mission.as_ref().unwrap().id, instance);
    courier_action(&w, &mut t, p, 3);
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ActionResult(v) if v.code==ActionCode::AlreadyActive)));
    send(
        &w,
        &mut t,
        p,
        ClientMessage::ResyncRequest(ResyncRequest {
            reason: ResyncReason::Stale,
        }),
    );
    step(&mut w, &mut a, &mut t);
    ready(&mut w, &mut a, &mut t, p);
    assert_eq!(w.sessions[&p].mission.as_ref().unwrap().id, instance);
    assert!(t.sent.iter().any(|(target,delivery,m)| *target==p && *delivery==Delivery::Reliable && matches!(&m.message,ServerMessage::PlayerStatus(v) if v.revision.0>1 && v.mission.as_ref().is_some_and(|m|m.instance_id.0==instance))));
    assert!(!t.sent.iter().any(|(target,_,m)| *target==q && matches!(&m.message,ServerMessage::PlayerStatus(v) if v.mission.is_some() || v.credits.0!=0)));
}
#[test]
fn courier_requires_continuous_slow_dwell_and_awards_once_across_retry_and_rejoin() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    let cookie = join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    let instance = w.sessions[&p].mission.as_ref().unwrap().id.clone();
    place(&mut w, p, [-44.0, 0.02, 52.0]);
    for _ in 0..29 {
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(a.profile(Some(&cookie)).unwrap().credits.0, 0);
    place(&mut w, p, [-44.0, 0.02, 60.0]);
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions[&p].mission.as_ref().unwrap().dwell, 0);
    place(&mut w, p, [-44.0, 0.02, 52.0]);
    for _ in 0..30 {
        step(&mut w, &mut a, &mut t);
    }
    let profile = a.profile(Some(&cookie)).unwrap();
    assert_eq!((profile.credits.0, profile.completed_jobs), (150, 1));
    assert_eq!(
        w.sessions[&p].mission.as_ref().unwrap().phase,
        server::missions::Phase::Completed
    );
    let duplicate = a
        .apply_mission_reward(&profile.profile_id.0, &instance, 150)
        .unwrap();
    assert_eq!(duplicate.credits.0, 150);
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    for _ in 0..151 {
        step(&mut w, &mut a, &mut t);
    }
    assert!(w.sessions[&p].mission.is_none());
    assert_eq!(a.profile(Some(&cookie)).unwrap().completed_jobs, 1);
    w.disconnect(p, &mut a);
    let q = peer(2);
    join(&mut w, &mut a, &mut t, q, Some(&cookie));
    ready(&mut w, &mut a, &mut t, q);
    assert_eq!(w.sessions[&q].profile.as_ref().unwrap().credits.0, 150);
    assert!(w.sessions[&q].mission.is_none());
}
#[test]
fn courier_cancel_timeout_death_disconnect_and_fast_delivery_do_not_pay() {
    for cause in ["cancel", "timeout", "death", "disconnect", "fast"] {
        let (mut w, mut a, mut t) = setup();
        let p = peer(1);
        let cookie = join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
        place(&mut w, p, [-172.0, 0.02, 180.0]);
        courier_action(&w, &mut t, p, 1);
        step(&mut w, &mut a, &mut t);
        place(&mut w, p, [-44.0, 0.02, 52.0]);
        // Arrange success on the next tick so terminal failures must win.
        w.sessions
            .get_mut(&p)
            .unwrap()
            .mission
            .as_mut()
            .unwrap()
            .dwell = 29;
        match cause {
            "cancel" => vehicle_action(&w, &mut t, p, 2, 1, Action::CancelMission(Empty {})),
            "timeout" => {
                w.sessions
                    .get_mut(&p)
                    .unwrap()
                    .mission
                    .as_mut()
                    .unwrap()
                    .deadline = w.tick + 1
            }
            "death" => {
                let id = w.sessions[&p].player.unwrap().0;
                w.players.get_mut(&id).unwrap().state.health = CharacterHealth(0);
            }
            "disconnect" => {
                w.disconnect(p, &mut a);
            }
            "fast" => {
                batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
                let id = w.sessions[&p].player.unwrap().0;
                w.players.get_mut(&id).unwrap().state.motor.velocity =
                    Vec3([Scalar(0.0), Scalar(0.0), Scalar(-4.0)]);
            }
            _ => unreachable!(),
        }
        step(&mut w, &mut a, &mut t);
        assert_eq!(a.profile(Some(&cookie)).unwrap().credits.0, 0, "{cause}");
        if cause == "fast" {
            assert_eq!(w.sessions[&p].mission.as_ref().unwrap().dwell, 0);
        } else if cause != "disconnect" {
            assert_eq!(
                w.sessions[&p].mission.as_ref().unwrap().phase,
                server::missions::Phase::Failed,
                "{cause}"
            );
        }
    }
}
#[test]
fn memory_rewards_are_atomic_bounded_and_reject_event_conflicts() {
    let mut a = MemoryProfiles::default();
    let (p, _) = a.session(None).unwrap();
    let (q, _) = a.session(None).unwrap();
    assert!(
        a.apply_mission_reward(&p.profile_id.0, "too-large", 1_000_000_001)
            .is_err()
    );
    let v = a
        .apply_mission_reward(&p.profile_id.0, "first", 150)
        .unwrap();
    assert_eq!(v.credits.0, 150);
    assert!(
        a.apply_mission_reward(&q.profile_id.0, "first", 150)
            .is_err()
    );
    assert!(
        a.apply_mission_reward(&p.profile_id.0, "first", 151)
            .is_err()
    );
    assert_eq!(
        a.apply_mission_reward(&p.profile_id.0, "first", 150)
            .unwrap()
            .completed_jobs,
        1
    );
}

#[test]
fn courier_completes_from_authoritative_car_motion_and_does_not_release_the_driver() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    let cookie = join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    let car = *w.vehicles.keys().next().unwrap();
    at_door(&mut w, p, car);
    vehicle_action(
        &w,
        &mut t,
        p,
        2,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(car),
        }),
    );
    step(&mut w, &mut a, &mut t);
    w.vehicles.get_mut(&car).unwrap().state.motor =
        sim_core::spawn_vehicle([-44.0, 0.0, 52.0], 0.0).unwrap();
    for _ in 0..29 {
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(a.profile(Some(&cookie)).unwrap().credits.0, 0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(a.profile(Some(&cookie)).unwrap().credits.0, 150);
    assert_eq!(w.vehicles[&car].state.driver_id, w.sessions[&p].player);
    assert_eq!(w.sessions[&p].epoch, 2);
    assert!(t.sent.iter().any(|(target,_,m)| *target==p && matches!(&m.message,ServerMessage::PlayerStatus(v) if v.credits.0==150 && v.completed_jobs==1 && v.mission.as_ref().is_some_and(|m|m.phase==MissionPhase::Completed))));
}

#[test]
fn full_city_installs_three_public_classes_without_spawning_recovery_reservations() {
    let content = WorldContent::load_index(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
        "city.json",
    )
    .unwrap();
    let mut w = World::new(content).unwrap();
    let mut a = MemoryProfiles::default();
    let mut t = FakeTransport::default();
    assert_eq!(w.vehicles.len(), 4);
    assert!(w.vehicles.values().any(|v| v.template.id == "van"));
    assert!(w.vehicles.values().any(|v| v.template.id == "sport"));
    for id in w.vehicles.keys().copied().collect::<Vec<_>>() {
        let p = peer(id);
        join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
        at_door(&mut w, p, id);
        step(&mut w, &mut a, &mut t);
        vehicle_action(
            &w,
            &mut t,
            p,
            1,
            1,
            Action::EnterVehicle(EnterVehicle {
                vehicle_id: EntityId(id),
            }),
        );
        step(&mut w, &mut a, &mut t);
        assert_eq!(w.vehicles[&id].state.driver_id, w.sessions[&p].player);
    }
}

#[test]
fn full_city_population_uses_real_replication_and_ai_seats_cannot_be_taken() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content");
    let mut w = World::new(WorldContent::load_index(&root, "city.json").unwrap()).unwrap();
    let mut a = MemoryProfiles::default();
    let mut t = FakeTransport::default();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    for _ in 0..120 {
        step(&mut w, &mut a, &mut t);
    }
    assert!(!w.population.traffic.is_empty());
    assert!(!w.population.pedestrians.is_empty());
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::EntityShow(s) if s.descriptor.entity_kind==EntityKind::Pedestrian && matches!(s.state,EntityState::Character(_)))));
    let pedestrian_ids: BTreeSet<_> = w.population.pedestrians.keys().copied().collect();
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::Snapshot(s) if s.records.iter().any(|r|pedestrian_ids.contains(&r.entity_id.0)))));
    let car_id = *w.population.traffic.keys().next().unwrap();
    w.vehicles.get_mut(&car_id).unwrap().stop();
    let player = w.sessions[&p].player.unwrap().0;
    let entry = w.vehicles[&car_id].entry();
    w.players.get_mut(&player).unwrap().state.motor =
        sim_core::spawn_character(entry, 0.0).unwrap();
    step(&mut w, &mut a, &mut t);
    vehicle_action(
        &w,
        &mut t,
        p,
        100,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(car_id),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::ActionResult(r) if r.request_id.0==100 && r.code==ActionCode::Occupied)));
    assert_eq!(w.vehicles[&car_id].state.driver_id, None);
    // The parking state machine's Available transition enables ordinary possession.
    w.population.traffic.get_mut(&car_id).unwrap().phase =
        server::population::TrafficPhase::Available;
    w.vehicles.get_mut(&car_id).unwrap().stop();
    at_door(&mut w, p, car_id);
    vehicle_action(
        &w,
        &mut t,
        p,
        101,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(car_id),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.vehicles[&car_id].state.driver_id, Some(EntityId(player)));
    assert_eq!(
        w.population.pools[&car_id],
        server::population::VehiclePool::Traffic
    );
    let before = w.vehicles[&car_id].state.motor.position.clone();
    for _ in 0..6 {
        batch(&w, &mut t, p, w.tick + 1, 0, 32767, 2);
        step(&mut w, &mut a, &mut t);
    }
    assert_ne!(w.vehicles[&car_id].state.motor.position, before);
    w.disconnect(p, &mut a);
    assert_eq!(w.vehicles[&car_id].state.driver_id, None);
    assert_eq!(
        w.population.pools[&car_id],
        server::population::VehiclePool::Traffic
    );
    assert!(w.population.traffic.len() <= 24 && w.population.pedestrians.len() <= 32);
}

#[test]
fn populated_snapshots_preserve_budget_for_a_complete_resync_baseline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content");
    let mut w = World::new(WorldContent::load_index(&root, "city.json").unwrap()).unwrap();
    let mut a = MemoryProfiles::default();
    let mut t = FakeTransport::default();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    for _ in 0..360 {
        if w.tick.is_multiple_of(60) {
            batch(&w, &mut t, p, w.tick + 1, 0, 0, 1);
        }
        step(&mut w, &mut a, &mut t);
        t.sent.clear();
    }
    assert!(
        w.metrics.budget_drops > 0,
        "exercise saturated snapshot output"
    );
    send(
        &w,
        &mut t,
        p,
        ClientMessage::ResyncRequest(ResyncRequest {
            reason: ResyncReason::Stale,
        }),
    );
    step(&mut w, &mut a, &mut t);
    let mut shown = BTreeSet::new();
    let mut expected = None;
    for (_, _, envelope) in &t.sent {
        match &envelope.message {
            ServerMessage::EntityShow(s) if s.baseline_id == Some(BaselineId(2)) => {
                shown.insert(s.descriptor.id.0);
            }
            ServerMessage::BaselineEnd(end) if end.baseline_id == BaselineId(2) => {
                expected = Some(usize::from(end.entity_count.0))
            }
            _ => {}
        }
    }
    assert!(
        w.sessions.contains_key(&p),
        "resync must not exhaust reliable byte budget"
    );
    assert_eq!(Some(shown.len()), expected);
    assert!(shown.len() > 32);
    ready(&mut w, &mut a, &mut t, p);
    assert_eq!(w.sessions[&p].phase, SessionPhase::Playing);
    assert_eq!(w.sessions[&p].epoch, 2);
}
