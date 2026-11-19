use crate as server;
use protocol::*;
use server::component::PedestrianSnapshot;
use server::component::PlayerSnapshot;
use server::component::VehicleSnapshot;
use server::systems::police::Unit as PoliceUnitComponent;
use server::systems::population::Traffic as TrafficComponent;
use server::{
    admission::Profiles,
    content_loader::WorldContent,
    sessions::{SessionPhase, Sessions},
    systems::Systems,
    tick,
    transport::*,
    world::World as CoreWorld,
};
use std::result::Result;
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
};
/// Owns scenario world, session, system, and transport state between steps.
struct Fixture {
    /// Owned world state used by this scenario fixture.
    core: CoreWorld,
    /// Live session state used by this system.
    sessions: Sessions,
    /// Gameplay system resources retained between scenario steps.
    systems: Systems,
    /// Live network transport.
    network: Network,
}
impl std::ops::Deref for Fixture {
    type Target = CoreWorld;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}
impl std::ops::DerefMut for Fixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}
/// Owns world state for scenario fixtures between scheduled ticks.
struct World {
    /// Owned scenario state temporarily moved into a tick driver during each step.
    fixture: Option<Fixture>,
    /// Ordered ECS schedule used to advance one tick.
    schedule: Option<bevy_ecs::prelude::Schedule>,
}
impl std::ops::Deref for World {
    type Target = Fixture;
    fn deref(&self) -> &Fixture {
        self.fixture.as_ref().unwrap()
    }
}
impl std::ops::DerefMut for World {
    fn deref_mut(&mut self) -> &mut Fixture {
        self.fixture.as_mut().unwrap()
    }
}
impl World {
    /// Creates an owned scenario world from validated content.
    fn new(content: WorldContent) -> Result<Self, String> {
        let mut core = CoreWorld::new(content)?;
        let mut sessions = Sessions::default();
        let systems = Systems::new(&mut core, &mut sessions)?;
        Ok(Self {
            fixture: Some(Fixture {
                core,
                sessions,
                systems,
                network: Network::default(),
            }),
            schedule: None,
        })
    }
    /// Advances one complete tick and returns borrowed profile storage to its caller.
    fn step(
        &mut self,
        network: &mut Network,
        profiles: &mut Profiles,
        now: u64,
        allow_joins: bool,
    ) -> Result<(), String> {
        let Fixture {
            core,
            sessions,
            systems,
            network: mut idle,
        } = self.fixture.take().unwrap();
        std::mem::swap(&mut idle, network);
        let mut driver = tick::TickDriver::from_parts(core, sessions, systems, idle);
        if let Some(schedule) = self.schedule.take() {
            driver.use_schedule(schedule);
        }
        let result = driver.step(profiles, now, allow_joins);
        let (core, sessions, systems, mut active, schedule) = driver.into_parts();
        std::mem::swap(&mut active, network);
        self.fixture = Some(Fixture {
            core,
            sessions,
            systems,
            network: active,
        });
        self.schedule = Some(schedule);
        result
    }
    /// Requests peer cleanup and records its reason.
    fn disconnect(&mut self, peer: PeerKey, profiles: &mut Profiles) {
        let Fixture {
            core,
            sessions,
            systems,
            ..
        } = self.fixture.as_mut().unwrap();
        sessions.disconnect(peer, profiles, core, systems);
    }
    /// Stops active work and drains pending persistence operations within the shutdown limit.
    fn shutdown(&mut self, profiles: &mut Profiles) -> bool {
        let Fixture {
            core,
            sessions,
            systems,
            network,
        } = self.fixture.take().unwrap();
        let mut driver = tick::TickDriver::from_parts(core, sessions, systems, network);
        if let Some(schedule) = self.schedule.take() {
            driver.use_schedule(schedule);
        }
        let drained = driver.shutdown(profiles);
        let (core, sessions, systems, network, schedule) = driver.into_parts();
        self.fixture = Some(Fixture {
            core,
            sessions,
            systems,
            network,
        });
        self.schedule = Some(schedule);
        drained
    }
}
/// Collects test output and simulates reliable delivery failures.
#[derive(Default)]
struct FakeTransport {
    /// Live network transport.
    network: Network,
    /// Transport events staged for the next test tick.
    events: VecDeque<TransportEvent>,
    /// Number of messages successfully submitted during this drain.
    sent: Vec<(PeerKey, Delivery, ServerEnvelope)>,
    /// Connection incarnations closed during this admission batch.
    closed: BTreeSet<PeerKey>,
    /// Peers whose reliable output is forced to fail in this test.
    full: BTreeSet<PeerKey>,
}
impl FakeTransport {
    /// Advances one complete tick and returns borrowed profile storage to its caller.
    fn step(&mut self, w: &mut World, a: &mut Profiles, now: u64) {
        for event in self.events.drain(..) {
            self.network.test_event(event);
        }
        for peer in std::mem::take(&mut self.full) {
            self.network.test_fill_reliable(peer);
        }
        w.step(&mut self.network, a, now, true).unwrap();
        let (sent, closed) = self.network.test_flush();
        self.sent.extend(sent);
        self.closed.extend(closed);
    }
}
/// Loads the published world content used by this test.
fn content() -> WorldContent {
    WorldContent::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
    )
    .unwrap()
}
/// Creates the world and profile state used by scenario tests.
fn setup() -> (World, Profiles, FakeTransport) {
    (
        World::new(content()).unwrap(),
        Profiles::default(),
        FakeTransport::default(),
    )
}
/// Looks up or creates the connection state used by this operation.
fn peer(id: u32) -> PeerKey {
    PeerKey {
        id,
        generation: u64::from(id) + 1,
    }
}
/// Advances one complete tick and returns borrowed profile storage to its caller.
fn step(w: &mut World, a: &mut Profiles, t: &mut FakeTransport) {
    t.step(w, a, u64::from(w.tick) * 34);
}
/// Queues or submits a message using the required delivery channel.
fn send(w: &World, t: &mut FakeTransport, p: PeerKey, message: ClientMessage) {
    let sid = w
        .sessions
        .peers
        .get(&p)
        .and_then(|s| s.id.as_ref())
        .map(|s| OpaqueId(s.clone()));
    t.events.push_back(TransportEvent::Message {
        peer: p,
        message: ClientEnvelope {
            v: ProtocolVersion(4),
            world_id: Some(OpaqueId(w.id.clone())),
            session_id: sid,
            message,
        },
    });
}
/// Admits a test guest and completes its initial readiness handshake.
fn join(
    w: &mut World,
    a: &mut Profiles,
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
            tshirt_colour: Default::default(),
            protocol_version: ProtocolVersion(4),
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
/// Builds the player readiness checkpoint.
fn ready(w: &mut World, a: &mut Profiles, t: &mut FakeTransport, p: PeerKey) {
    let baseline_id = BaselineId(w.sessions.peers[&p].baseline);
    send(
        w,
        t,
        p,
        ClientMessage::ClientReady(ClientReady { baseline_id }),
    );
    step(w, a, t);
}
/// Encodes the requested input frames into a client input batch.
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
/// Verifies that loading is inert and ready starts once with ordered baseline and status.
#[test]
fn loading_is_inert_and_ready_starts_once_with_ordered_baseline_and_status() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    let id = w.sessions.peers[&p].player.unwrap().0;
    let initial = w.entities.view::<PlayerSnapshot>()[&id].state.clone();
    for _ in 0..5 {
        batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(w.entities.view::<PlayerSnapshot>()[&id].state, initial);
    assert_eq!(w.sessions.peers[&p].phase, SessionPhase::Loading);
    send(
        &w,
        &mut t,
        p,
        ClientMessage::ClientReady(ClientReady {
            baseline_id: BaselineId(99),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions.peers[&p].phase, SessionPhase::Loading);
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
    assert_eq!(w.sessions.peers[&p].phase, SessionPhase::Playing);
}
/// Verifies that duplicates conflicts stale future and epoch inputs cannot accelerate the actor.
#[test]
fn duplicates_conflicts_stale_future_and_epoch_inputs_cannot_accelerate_the_actor() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let id = w.sessions.peers[&p].player.unwrap().0;
    let before = w.entities.view::<PlayerSnapshot>()[&id].state.motor.clone();
    let tick = w.tick + 1;
    for _ in 0..10 {
        batch(&w, &mut t, p, tick, 0, 32767, 1);
    }
    batch(&w, &mut t, p, tick, 32767, 0, 1);
    batch(&w, &mut t, p, w.tick, 0, 32767, 1);
    batch(&w, &mut t, p, tick + 9, 0, 32767, 1);
    batch(&w, &mut t, p, tick, 0, 32767, 2);
    step(&mut w, &mut a, &mut t);
    let after = &w.entities.view::<PlayerSnapshot>()[&id].state.motor;
    assert!((after.position.0[0].0 - before.position.0[0].0).abs() < 0.001);
    assert!(before.position.0[2].0 - after.position.0[2].0 > 0.0);
    assert!(before.position.0[2].0 - after.position.0[2].0 < 0.05);
    assert_eq!(w.metrics.conflicting_inputs, 1);
    assert_eq!(w.metrics.late_inputs, 1);
    assert_eq!(w.metrics.future_inputs, 1);
    assert_eq!(w.metrics.stale_epochs, 1);
    assert!(w.sessions.peers[&p].inputs.is_empty());
    for _ in 0..10 {
        step(&mut w, &mut a, &mut t);
    }
    assert!(
        w.entities.view::<PlayerSnapshot>()[&id]
            .state
            .motor
            .velocity
            .0
            .iter()
            .all(|v| v.0.abs() < 0.001)
    );
}
/// Verifies that future ring is nine ticks and one frame is consumed per step.
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
    assert_eq!(w.sessions.peers[&p].inputs.len(), 8);
    for expected in (0..8).rev() {
        step(&mut w, &mut a, &mut t);
        assert_eq!(w.sessions.peers[&p].inputs.len(), expected);
    }
}
/// Verifies that resync increments epoch and visibility without moving until new readiness.
#[test]
fn resync_increments_epoch_and_visibility_without_moving_until_new_readiness() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let id = w.sessions.peers[&p].player.unwrap().0;
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
    assert_eq!(w.sessions.peers[&p].epoch, 2);
    assert_eq!(w.sessions.peers[&p].baseline, 2);
    assert!(w.sessions.peers[&p].inputs.is_empty());
    let position = w.entities.view::<PlayerSnapshot>()[&id]
        .state
        .motor
        .position
        .clone();
    batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id]
            .state
            .motor
            .position,
        position
    );
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
/// Verifies that guest replacement disconnect cleanup and generation reuse are idempotent.
#[test]
fn guest_replacement_disconnect_cleanup_and_generation_reuse_are_idempotent() {
    let (mut w, mut a, mut t) = setup();
    let old = peer(0);
    let cookie = join(&mut w, &mut a, &mut t, old, None);
    ready(&mut w, &mut a, &mut t, old);
    let first = w.sessions.peers[&old].player.unwrap();
    let profile = w.sessions.peers[&old]
        .profile
        .as_ref()
        .unwrap()
        .profile_id
        .clone();
    let sid = w.sessions.peers[&old].id.clone();
    let new = peer(1);
    join(&mut w, &mut a, &mut t, new, Some(&cookie));
    assert!(!w.sessions.peers.contains_key(&old));
    assert!(t.closed.contains(&old));
    assert!(!w.entities.view::<PlayerSnapshot>().contains_key(&first.0));
    assert_eq!(w.entities.view::<PlayerSnapshot>().len(), 1);
    assert_eq!(
        w.sessions.peers[&new].profile.as_ref().unwrap().profile_id,
        profile
    );
    assert_ne!(w.sessions.peers[&new].id, sid);
    assert!(t.sent.iter().any(|(peer, _, m)| *peer == old
        && m.session_id.as_ref().map(|id| &id.0) == sid.as_ref()
        && matches!(m.message, ServerMessage::Disconnect(_))));
    assert!(w.sessions.peers[&new].player.unwrap().0 > first.0);
    t.events.push_back(TransportEvent::Closed {
        peer: old,
        reason: "late close",
    });
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.entities.view::<PlayerSnapshot>().len(), 1);
    send(&w, &mut t, new, ClientMessage::Leave(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert!(w.entities.view::<PlayerSnapshot>().is_empty());
    assert!(w.sessions.peers.is_empty());
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
    assert!(w.sessions.peers.contains_key(&reused));
}
/// Verifies that other observers receive show then hide and reliable full removes ownership.
#[test]
fn other_observers_receive_show_then_hide_and_reliable_full_removes_ownership() {
    let (mut w, mut a, mut t) = setup();
    let first = peer(0);
    let second = peer(1);
    join(&mut w, &mut a, &mut t, first, None);
    ready(&mut w, &mut a, &mut t, first);
    join(&mut w, &mut a, &mut t, second, None);
    let id = w.sessions.peers[&second].player.unwrap();
    assert!(t.sent.iter().any(|(p,_,m)|*p==first && matches!(&m.message,ServerMessage::EntityShow(s) if s.descriptor.id==id && s.baseline_id.is_none())));
    t.full.insert(second);
    ready(&mut w, &mut a, &mut t, second);
    assert!(!w.sessions.peers.contains_key(&second));
    assert!(!w.entities.view::<PlayerSnapshot>().contains_key(&id.0));
    assert!(t.sent.iter().any(|(p, _, m)| *p == first
        && matches!(&m.message,ServerMessage::EntityHide(s) if s.entity_id==id)));
}
/// Verifies that loading hello and playing timeouts remove all state.
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
        t.step(&mut w, &mut a, 40_000);
        assert!(w.sessions.peers.is_empty());
        assert!(w.entities.view::<PlayerSnapshot>().is_empty());
        assert!(t.closed.contains(&p));
    }
}
/// Verifies that unknown peers wrong scope and invalid tickets cannot allocate actors.
#[test]
fn unknown_peers_wrong_scope_and_invalid_tickets_cannot_allocate_actors() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    send(&w, &mut t, p, ClientMessage::Leave(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert!(w.entities.view::<PlayerSnapshot>().is_empty());
    t.events.push_back(TransportEvent::Opened(p));
    step(&mut w, &mut a, &mut t);
    let build = w.build();
    send(
        &w,
        &mut t,
        p,
        ClientMessage::ClientHello(ClientHello {
            tshirt_colour: Default::default(),
            protocol_version: ProtocolVersion(4),
            build_id: build.build_id,
            content_hash: build.content_hash,
            sim_config_hash: build.sim_config_hash,
            join_ticket: OpaqueId("bad-ticket".into()),
            display_name: DisplayName("Test Name".into()),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert!(w.entities.view::<PlayerSnapshot>().is_empty());
    assert!(w.sessions.peers.is_empty());
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::Reject(r) if r.code==RejectCode::InvalidTicket)));
    join(&mut w, &mut a, &mut t, peer(1), None);
    let p = peer(1);
    t.events.push_back(TransportEvent::Message {
        peer: p,
        message: ClientEnvelope {
            v: ProtocolVersion(4),
            world_id: Some(OpaqueId("wrong-boot".into())),
            session_id: w.sessions.peers[&p].id.clone().map(OpaqueId),
            message: ClientMessage::Leave(Empty {}),
        },
    });
    step(&mut w, &mut a, &mut t);
    assert!(w.entities.view::<PlayerSnapshot>().is_empty());
}
/// Verifies that full magazine reload is rejected and duplicate requests return the cached result.
#[test]
fn full_magazine_reload_is_rejected_and_duplicate_requests_return_the_cached_result() {
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
    assert_eq!(results[0].code, ActionCode::Cooldown);
}

/// Verifies that loading sessions reserve capacity and unknown peers cannot allocate a seventeenth player.
#[test]
fn loading_sessions_reserve_capacity_and_unknown_peers_cannot_allocate_a_seventeenth_player() {
    let (mut w, mut a, mut t) = setup();
    for id in 0..16 {
        join(&mut w, &mut a, &mut t, peer(id), None);
    }
    assert_eq!(w.entities.view::<PlayerSnapshot>().len(), 16);
    assert_eq!(w.sessions.peers.len(), 16);
    assert!(
        w.sessions
            .peers
            .values()
            .all(|s| s.phase == SessionPhase::Loading)
    );
    t.events.push_back(TransportEvent::Opened(peer(16)));
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.entities.view::<PlayerSnapshot>().len(), 16);
    assert!(t.closed.contains(&peer(16)));
    let positions: Vec<_> = w
        .entities
        .view::<PlayerSnapshot>()
        .values()
        .map(|p| p.state.motor.position.clone())
        .collect();
    for (i, p) in positions.iter().enumerate() {
        assert!(positions.iter().skip(i + 1).all(|other| other != p));
    }
}

/// Verifies that playing observer receives split movement snapshots and fresh visibility after aoi return.
#[test]
fn playing_observer_receives_split_movement_snapshots_and_fresh_visibility_after_aoi_return() {
    let (mut w, mut a, mut t) = setup();
    let observer = peer(0);
    join(&mut w, &mut a, &mut t, observer, None);
    ready(&mut w, &mut a, &mut t, observer);
    for id in 1..8 {
        join(&mut w, &mut a, &mut t, peer(id), None);
    }
    let mover = w.sessions.peers[&peer(1)].player.unwrap().0;
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
    let origin = w.entities.view::<PlayerSnapshot>()
        [&w.sessions.peers[&observer].player.unwrap().0]
        .state
        .motor
        .position
        .clone();
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&mover)
        .unwrap()
        .state
        .motor
        .position
        .0[0]
        .0 = origin.0[0].0 + 170.0;
    step(&mut w, &mut a, &mut t);
    assert!(!t.sent.iter().any(|(p, _, m)| *p == observer
        && matches!(&m.message, ServerMessage::EntityHide(h) if h.entity_id.0 == mover)));
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&mover)
        .unwrap()
        .state
        .motor
        .position
        .0[0]
        .0 = origin.0[0].0 + 200.0;
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(p, _, m)| *p == observer && matches!(&m.message, ServerMessage::EntityHide(h) if h.entity_id.0 == mover && h.reason == HideReason::OutOfRange)));
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&mover)
        .unwrap()
        .state
        .motor
        .position = origin;
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(p, _, m)| *p == observer && matches!(&m.message, ServerMessage::EntityShow(s) if s.descriptor.id.0 == mover && s.visibility_id.0 > visibility)));
    assert!(w.metrics.max_snapshot_bytes <= 1100);
    assert!(w.metrics.snapshot_packets > 1);
    assert!(w.metrics.max_refresh_age_ticks <= 3);
}

/// Verifies that sustained input byte flood cannot expand the ring or keep an actor.
#[test]
fn sustained_input_byte_flood_cannot_expand_the_ring_or_keep_an_actor() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    // A one-off delayed burst is tolerated; sustained raw ingress still exhausts
    // the unchanged byte budget and retires the actor. Never grow the input ring.
    for _ in 0..4 {
        for _ in 0..100 {
            batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
        }
        step(&mut w, &mut a, &mut t);
        if let Some(session) = w.sessions.peers.get(&p) {
            assert!(session.inputs.len() <= 8);
        }
    }
    assert!(!w.sessions.peers.contains_key(&p));
    assert!(w.entities.view::<PlayerSnapshot>().is_empty());
    assert!(t.closed.contains(&p));
}

/// Builds a scoped vehicle action request for the scenario player.
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
/// Places the scenario player at a valid vehicle entry position.
fn at_door(w: &mut World, p: PeerKey, car: u32) {
    let position = crate::systems::vehicle::entry(&w.entities.view::<VehicleSnapshot>()[&car]);
    let id = w.sessions.peers[&p].player.unwrap().0;
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&id)
        .unwrap()
        .state
        .motor = sim_core::spawn_character(position, 0.0).unwrap();
}
/// Verifies that simultaneous entry is atomic epoch scoped idempotent and disconnect releases car.
#[test]
fn simultaneous_entry_is_atomic_epoch_scoped_idempotent_and_disconnect_releases_car() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    let q = peer(2);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    join(&mut w, &mut a, &mut t, q, None);
    ready(&mut w, &mut a, &mut t, q);
    let car = *w.entities.view::<VehicleSnapshot>().keys().next().unwrap();
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
    let driver = w.sessions.peers[&p].player.unwrap();
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car].state.driver_id,
        Some(driver)
    );
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&driver.0].state.mode,
        CharacterMode::Driving
    );
    assert_eq!(w.sessions.peers[&p].epoch, 2);
    assert_eq!(w.sessions.peers[&q].epoch, 1);
    assert!(w.sessions.peers[&p].inputs.is_empty());
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
    assert_eq!(w.sessions.peers[&p].epoch, 2);
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car]
            .state
            .motor
            .speed
            .0,
        0.0
    );
    for _ in 0..15 {
        batch(&w, &mut t, p, w.tick + 1, 0, 32767, 2);
        step(&mut w, &mut a, &mut t);
    }
    assert!(
        w.entities.view::<VehicleSnapshot>()[&car]
            .state
            .motor
            .speed
            .0
            > 3.0
    );
    vehicle_action(&w, &mut t, p, 2, 2, Action::ExitVehicle(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions.peers[&p].epoch, 2);
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ActionResult(v) if v.request_id.0==2 && v.code==ActionCode::TooFast)));
    w.disconnect(p, &mut a);
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car].state.driver_id,
        None
    );
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car]
            .state
            .motor
            .speed
            .0,
        0.0
    );
}
/// Verifies that exit reclaims player with fresh epoch and old car inputs cannot move it.
#[test]
fn exit_reclaims_player_with_fresh_epoch_and_old_car_inputs_cannot_move_it() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let car = *w.entities.view::<VehicleSnapshot>().keys().next().unwrap();
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
    let id = w.sessions.peers[&p].player.unwrap();
    assert_eq!(w.sessions.peers[&p].epoch, 3);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id.0].state.mode,
        CharacterMode::OnFoot
    );
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car].state.driver_id,
        None
    );
    let position = w.entities.view::<PlayerSnapshot>()[&id.0]
        .state
        .motor
        .position
        .clone();
    batch(&w, &mut t, p, w.tick + 1, 32767, 32767, 2);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id.0]
            .state
            .motor
            .position
            .0[0],
        position.0[0]
    );
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id.0]
            .state
            .motor
            .position
            .0[2],
        position.0[2]
    );
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ControlChanged(v) if v.controlled_entity_id==id && v.control_epoch.0==3)));
}
/// Builds a scoped courier mission request for the scenario player.
fn courier_action(w: &World, t: &mut FakeTransport, p: PeerKey, request: u32) {
    vehicle_action(
        w,
        t,
        p,
        request,
        w.sessions.peers[&p].epoch,
        Action::AcceptMission(AcceptMission {
            template_id: TemplateId("courier_quay_market".into()),
        }),
    );
}
/// Moves the scenario actor to the requested position.
fn place(w: &mut World, p: PeerKey, position: [f32; 3]) {
    let id = w.sessions.peers[&p].player.unwrap().0;
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&id)
        .unwrap()
        .state
        .motor = sim_core::spawn_character(position, 0.0).unwrap();
}
/// Verifies that memory rewards are atomic bounded and reject event conflicts.
#[test]
fn memory_rewards_are_atomic_bounded_and_reject_event_conflicts() {
    let mut a = Profiles::default();
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

/// Verifies that courier completes from authoritative car motion and does not release the driver.
#[test]
fn courier_completes_from_authoritative_car_motion_and_does_not_release_the_driver() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    let cookie = join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    let car = *w.entities.view::<VehicleSnapshot>().keys().next().unwrap();
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
    w.entities
        .get_mut_by_id::<VehicleSnapshot>(&car)
        .unwrap()
        .state
        .motor = sim_core::spawn_vehicle([-44.0, 0.0, 52.0], 0.0).unwrap();
    for _ in 0..29 {
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(a.profile(Some(&cookie)).unwrap().credits.0, 0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(a.profile(Some(&cookie)).unwrap().credits.0, 150);
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car].state.driver_id,
        w.sessions.peers[&p].player
    );
    assert_eq!(w.sessions.peers[&p].epoch, 2);
    assert!(t.sent.iter().any(|(target,_,m)| *target==p && matches!(&m.message,ServerMessage::PlayerStatus(v) if v.credits.0==150 && v.completed_jobs==1 && v.mission.as_ref().is_some_and(|m|m.phase==MissionPhase::Completed))));
}

/// Verifies that full city installs three public classes without spawning recovery reservations.
#[test]
fn full_city_installs_three_public_classes_without_spawning_recovery_reservations() {
    let content = WorldContent::load_index(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
        "city.json",
    )
    .unwrap();
    let mut w = World::new(content).unwrap();
    let mut a = Profiles::default();
    let mut t = FakeTransport::default();
    assert_eq!(
        w.entities
            .values::<server::component::PoolMembership>()
            .len(),
        16
    );
    assert_eq!(w.entities.view::<PoliceUnitComponent>().len(), 4);
    assert!(
        w.entities
            .view::<VehicleSnapshot>()
            .values()
            .any(|v| v.template.id == "van")
    );
    assert!(
        w.entities
            .view::<VehicleSnapshot>()
            .values()
            .any(|v| v.template.id == "sport")
    );
    for id in w
        .entities
        .values::<server::component::PoolMembership>()
        .keys()
        .copied()
        .collect::<Vec<_>>()
    {
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
        assert_eq!(
            w.entities.view::<VehicleSnapshot>()[&id].state.driver_id,
            w.sessions.peers[&p].player
        );
    }
}

/// Verifies that full city population uses real replication and ai seats cannot be taken.
#[test]
fn full_city_population_uses_real_replication_and_ai_seats_cannot_be_taken() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content");
    let mut w = World::new(WorldContent::load_index(&root, "city.json").unwrap()).unwrap();
    let mut a = Profiles::default();
    let mut t = FakeTransport::default();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    for _ in 0..120 {
        step(&mut w, &mut a, &mut t);
    }
    assert!(!w.entities.view::<TrafficComponent>().is_empty());
    assert!(!w.entities.view::<PedestrianSnapshot>().is_empty());
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::EntityShow(s) if s.descriptor.entity_kind==EntityKind::Pedestrian && matches!(s.state,EntityState::Character(_)))));
    let pedestrian_ids: BTreeSet<_> = w
        .entities
        .view::<PedestrianSnapshot>()
        .keys()
        .copied()
        .collect();
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::Snapshot(s) if s.records.iter().any(|r|pedestrian_ids.contains(&r.entity_id.0)))));
    let car_id = *w.entities.view::<TrafficComponent>().keys().next().unwrap();
    crate::systems::vehicle::stop(
        &mut w
            .entities
            .get_mut_by_id::<VehicleSnapshot>(&car_id)
            .unwrap(),
    );
    let player = w.sessions.peers[&p].player.unwrap().0;
    let entry = crate::systems::vehicle::entry(&w.entities.view::<VehicleSnapshot>()[&car_id]);
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&player)
        .unwrap()
        .state
        .motor = sim_core::spawn_character(entry, 0.0).unwrap();
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
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car_id]
            .state
            .driver_id,
        None
    );
    // The parking state machine's Available transition enables ordinary possession.
    w.entities
        .get_mut_by_id::<TrafficComponent>(&car_id)
        .unwrap()
        .phase = server::systems::population::TrafficPhase::Available;
    crate::systems::vehicle::stop(
        &mut w
            .entities
            .get_mut_by_id::<VehicleSnapshot>(&car_id)
            .unwrap(),
    );
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
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car_id]
            .state
            .driver_id,
        Some(EntityId(player))
    );
    assert_eq!(
        w.entities.values::<server::component::PoolMembership>()[&car_id],
        server::systems::population::VehiclePool::Traffic
    );
    let before = w.entities.view::<VehicleSnapshot>()[&car_id]
        .state
        .motor
        .position
        .clone();
    for _ in 0..6 {
        batch(&w, &mut t, p, w.tick + 1, 0, 32767, 2);
        step(&mut w, &mut a, &mut t);
    }
    assert_ne!(
        w.entities.view::<VehicleSnapshot>()[&car_id]
            .state
            .motor
            .position,
        before
    );
    w.disconnect(p, &mut a);
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car_id]
            .state
            .driver_id,
        None
    );
    assert_eq!(
        w.entities.values::<server::component::PoolMembership>()[&car_id],
        server::systems::population::VehiclePool::Traffic
    );
    assert!(
        w.entities.view::<TrafficComponent>().len() <= 24
            && w.entities.view::<PedestrianSnapshot>().len() <= 32
    );
}

/// Verifies that populated snapshots preserve budget for a complete resync baseline.
#[test]
fn populated_snapshots_preserve_budget_for_a_complete_resync_baseline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content");
    let mut w = World::new(WorldContent::load_index(&root, "city.json").unwrap()).unwrap();
    let mut a = Profiles::default();
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
        w.sessions.peers.contains_key(&p),
        "resync must not exhaust reliable byte budget"
    );
    assert_eq!(Some(shown.len()), expected);
    assert!(shown.len() > 32);
    ready(&mut w, &mut a, &mut t, p);
    assert_eq!(w.sessions.peers[&p].phase, SessionPhase::Playing);
    assert_eq!(w.sessions.peers[&p].epoch, 2);
}

/// Builds a scoped weapon fire request for the scenario player.
fn fire(w: &World, t: &mut FakeTransport, p: PeerKey, request: u32, yaw: f32) {
    if crate::systems::weapons::state::definition(
        &w.entities.view::<PlayerSnapshot>()[&w.sessions.peers[&p].player.unwrap().0].weapon,
    )
    .automatic
    {
        send(
            w,
            t,
            p,
            ClientMessage::Trigger(Trigger {
                control_epoch: ControlEpoch(w.sessions.peers[&p].epoch),
                sequence: request,
                held: true,
                pressed: true,
                weapon_id: WeaponId::MachineGun,
                aim_yaw: Yaw(yaw),
                aim_pitch: Pitch(-0.05),
            }),
        );
        return;
    }

    vehicle_action(
        w,
        t,
        p,
        request,
        w.sessions.peers[&p].epoch,
        Action::Fire(Fire {
            weapon_id: w.entities.view::<PlayerSnapshot>()[&w.sessions.peers[&p].player.unwrap().0]
                .weapon
                .equipped,
            aim_yaw: Yaw(yaw),
            aim_pitch: Pitch(-0.05),
        }),
    );
}
/// Verifies that shots hit players and do not damage another guests occupied car.
#[test]
fn shots_hit_players_and_do_not_damage_another_guests_occupied_car() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    let q = peer(1);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    join(&mut w, &mut a, &mut t, q, None);
    ready(&mut w, &mut a, &mut t, q);
    let car = 1;
    let pos = w.entities.view::<VehicleSnapshot>()[&car]
        .state
        .motor
        .position
        .0
        .map(|v| v.0);
    let shooter = w.sessions.peers[&p].player.unwrap().0;
    let other = w.sessions.peers[&q].player.unwrap().0;
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&other)
        .unwrap()
        .lifecycle
        .protected_until = None;
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&shooter)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([pos[0], 0.02, pos[2] + 10.0], 0.0).unwrap();
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&other)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([pos[0], 0.02, pos[2] + 5.0], 0.0).unwrap();
    fire(&w, &mut t, p, 1, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&other].state.health.0,
        75
    );
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car].state.health.0,
        100
    );
    at_door(&mut w, q, car);
    vehicle_action(
        &w,
        &mut t,
        q,
        1,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(car),
        }),
    );
    step(&mut w, &mut a, &mut t);
    for _ in 0..9 {
        step(&mut w, &mut a, &mut t);
    }
    fire(&w, &mut t, p, 2, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<VehicleSnapshot>()[&car].state.health.0,
        100
    );
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&other].state.health.0,
        75
    );
}

/// Verifies that walls block shots and pedestrians incapacitate then leave after ten seconds.
#[test]
fn walls_block_shots_and_pedestrians_incapacitate_then_leave_after_ten_seconds() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content");
    let mut w = World::new(WorldContent::load_index(&root, "city.json").unwrap()).unwrap();
    let mut a = Profiles::default();
    let mut t = FakeTransport::default();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    for _ in 0..35 {
        step(&mut w, &mut a, &mut t);
    }
    let ped = *w
        .entities
        .view::<PedestrianSnapshot>()
        .keys()
        .next()
        .unwrap();
    let player = w.sessions.peers[&p].player.unwrap().0;
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&player)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([-222.0, 0.02, 115.0], 0.0).unwrap();
    w.entities
        .get_mut_by_id::<PedestrianSnapshot>(&ped)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([-222.0, 0.02, 65.0], 0.0).unwrap();
    fire(&w, &mut t, p, 1, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PedestrianSnapshot>()[&ped].state.health.0,
        100
    );
    for request in 2..=5 {
        for _ in 0..9 {
            step(&mut w, &mut a, &mut t);
        }
        w.entities
            .get_mut_by_id::<PlayerSnapshot>(&player)
            .unwrap()
            .state
            .motor = sim_core::spawn_character([-128.0, 0.02, 120.0], 0.0).unwrap();
        w.entities
            .get_mut_by_id::<PedestrianSnapshot>(&ped)
            .unwrap()
            .state
            .motor = sim_core::spawn_character([-128.0, 0.02, 110.0], 0.0).unwrap();
        fire(&w, &mut t, p, request, 0.0);
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.entities.view::<PedestrianSnapshot>()[&ped].state.health.0,
        0
    );
    assert_eq!(
        w.entities.view::<PedestrianSnapshot>()[&ped].phase,
        server::systems::population::PedestrianPhase::Incapacitated
    );
    let downed = w.tick;
    for _ in 0..298 {
        if w.tick.is_multiple_of(60) {
            batch(&w, &mut t, p, w.tick + 1, 0, 0, 1);
        }
        step(&mut w, &mut a, &mut t);
    }
    assert!(w.entities.view::<PedestrianSnapshot>().contains_key(&ped));
    assert_eq!(w.tick, downed + 298);
    step(&mut w, &mut a, &mut t);
    assert!(!w.entities.view::<PedestrianSnapshot>().contains_key(&ped));
    assert!(w.entities.entity(EntityId(ped)).is_none());
    assert!(w.entities.view::<PedestrianSnapshot>().len() <= 32);
}

/// Verifies that static impacts apply once per counterpart per fifteen ticks.
#[test]
fn static_impacts_apply_once_per_counterpart_per_fifteen_ticks() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let crash = |w: &mut World| {
        let car = w.entities.get_mut_by_id::<VehicleSnapshot>(&1).unwrap();
        car.state.motor = sim_core::spawn_vehicle([-222.0, 0.0, 102.1], 0.0).unwrap();
        car.state.motor.speed = Scalar(22.0);
        car.state.motor.velocity = Vec3([Scalar(0.0), Scalar(0.0), Scalar(-22.0)]);
    };
    crash(&mut w);
    step(&mut w, &mut a, &mut t);
    let first = w.entities.view::<VehicleSnapshot>()[&1].state.health.0;
    assert!((50..100).contains(&first));
    for _ in 0..14 {
        crash(&mut w);
        step(&mut w, &mut a, &mut t);
        assert_eq!(
            w.entities.view::<VehicleSnapshot>()[&1].state.health.0,
            first
        );
    }
    crash(&mut w);
    step(&mut w, &mut a, &mut t);
    assert!(w.entities.view::<VehicleSnapshot>()[&1].state.health.0 < first);
}
/// Verifies that car contacts damage both cars but never another players occupied car.
#[test]
fn car_contacts_damage_both_cars_but_never_another_players_occupied_car() {
    for occupied in [false, true] {
        let (mut w, mut a, mut t) = setup();
        let p = peer(0);
        let q = peer(1);
        join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
        join(&mut w, &mut a, &mut t, q, None);
        ready(&mut w, &mut a, &mut t, q);
        if occupied {
            for (peer, id) in [(p, 1), (q, 2)] {
                at_door(&mut w, peer, id);
                vehicle_action(
                    &w,
                    &mut t,
                    peer,
                    1,
                    1,
                    Action::EnterVehicle(EnterVehicle {
                        vehicle_id: EntityId(id),
                    }),
                );
                step(&mut w, &mut a, &mut t);
            }
        }
        w.entities
            .get_mut_by_id::<VehicleSnapshot>(&1)
            .unwrap()
            .state
            .motor = sim_core::spawn_vehicle([-128.0, 0.0, 130.4], 0.0).unwrap();
        w.entities
            .get_mut_by_id::<VehicleSnapshot>(&2)
            .unwrap()
            .state
            .motor = sim_core::spawn_vehicle([-128.0, 0.0, 126.0], 0.0).unwrap();
        w.entities
            .get_mut_by_id::<VehicleSnapshot>(&1)
            .unwrap()
            .state
            .motor
            .speed = Scalar(22.0);
        w.entities
            .get_mut_by_id::<VehicleSnapshot>(&1)
            .unwrap()
            .state
            .motor
            .velocity = Vec3([Scalar(0.0), Scalar(0.0), Scalar(-22.0)]);
        step(&mut w, &mut a, &mut t);
        if occupied {
            assert_eq!(w.entities.view::<VehicleSnapshot>()[&1].state.health.0, 100);
            assert_eq!(w.entities.view::<VehicleSnapshot>()[&2].state.health.0, 100);
            for p in w.entities.view::<PlayerSnapshot>().values() {
                assert_eq!(p.state.health.0, 100);
            }
        } else {
            assert!(w.entities.view::<VehicleSnapshot>()[&1].state.health.0 < 100);
            assert!(w.entities.view::<VehicleSnapshot>()[&2].state.health.0 < 100);
        }
    }
}

/// Verifies that moving vehicle damages pedestrians through the world swept query.
#[test]
fn moving_vehicle_damages_pedestrians_through_the_world_swept_query() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content");
    let mut w = World::new(WorldContent::load_index(&root, "city.json").unwrap()).unwrap();
    let mut a = Profiles::default();
    let mut t = FakeTransport::default();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    for _ in 0..35 {
        step(&mut w, &mut a, &mut t);
    }
    let ped = *w
        .entities
        .view::<PedestrianSnapshot>()
        .keys()
        .next()
        .unwrap();
    w.entities
        .get_mut_by_id::<PedestrianSnapshot>(&ped)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([-128.0, 0.02, 124.0], 0.0).unwrap();
    let car = w.entities.get_mut_by_id::<VehicleSnapshot>(&1).unwrap();
    car.state.motor = sim_core::spawn_vehicle([-128.0, 0.0, 130.0], 0.0).unwrap();
    car.state.motor.speed = Scalar(22.0);
    car.state.motor.velocity = Vec3([Scalar(0.0), Scalar(0.0), Scalar(-22.0)]);
    for _ in 0..12 {
        step(&mut w, &mut a, &mut t);
    }
    assert!(
        w.entities.view::<PedestrianSnapshot>()[&ped].state.health.0 < 100,
        "physical recovery must not erase the authoritative impact"
    );
}
/// Verifies that destroyed car waits for clear parking instead of spawning on a guest.
#[test]
fn destroyed_car_waits_for_clear_parking_instead_of_spawning_on_a_guest() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    let q = peer(1);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    join(&mut w, &mut a, &mut t, q, None);
    ready(&mut w, &mut a, &mut t, q);
    let shooter = w.sessions.peers[&p].player.unwrap().0;
    let blocker = w.sessions.peers[&q].player.unwrap().0;
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&shooter)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([-152.0, 0.02, 186.0], 0.0).unwrap();
    for request in 1..=4 {
        fire(&w, &mut t, p, request, 0.0);
        step(&mut w, &mut a, &mut t);
        for _ in 0..9 {
            step(&mut w, &mut a, &mut t);
        }
    }
    assert!(w.entities.view::<VehicleSnapshot>()[&1].state.disabled);
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&blocker)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([-148.5, 0.02, 176.0], 0.0).unwrap();
    for _ in 0..910 {
        if w.tick.is_multiple_of(60) {
            batch(&w, &mut t, p, w.tick + 1, 0, 0, 1);
            batch(&w, &mut t, q, w.tick + 1, 0, 0, 1);
        }
        step(&mut w, &mut a, &mut t);
        t.sent.clear();
    }
    assert!(
        w.entities.view::<VehicleSnapshot>()[&1].state.disabled,
        "occupied parking must defer respawn"
    );
    w.entities
        .get_mut_by_id::<PlayerSnapshot>(&blocker)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([-180.0, 0.02, 180.0], 0.0).unwrap();
    for _ in 0..31 {
        step(&mut w, &mut a, &mut t);
    }
    assert!(!w.entities.view::<VehicleSnapshot>()[&1].state.disabled);
    assert_eq!(w.entities.view::<VehicleSnapshot>()[&1].state.health.0, 100);
}

/// Creates a populated city scenario with active police units.
fn police_setup() -> (World, Profiles, FakeTransport) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content");
    (
        World::new(WorldContent::load_index(&root, "city.json").unwrap()).unwrap(),
        Profiles::default(),
        FakeTransport::default(),
    )
}
/// Refreshes the session while the scenario advances through multiple ticks.
fn keep_alive(w: &World, t: &mut FakeTransport, p: PeerKey) {
    batch(w, t, p, w.tick + 1, 0, 0, w.sessions.peers[&p].epoch);
}
#[path = "../scenarios/jobs_and_repairs.rs"]
mod jobs_and_repairs;

#[path = "../scenarios/durable_profiles.rs"]
mod durable_profiles;

#[path = "../scenarios/session_lifecycle.rs"]
mod session_lifecycle;

/// Verifies that delayed input burst is capped without retiring a healthy session.
#[test]
fn delayed_input_burst_is_capped_without_retiring_a_healthy_session() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let player = w.sessions.peers[&p].player.unwrap().0;
    // A delayed 30 Hz stream can be delivered as a burst. Only the first 60
    // batches enter simulation; the ten excess future inputs must not be stored.
    for _ in 0..60 {
        batch(&w, &mut t, p, w.tick + 1, 0, 0, 1);
    }
    for _ in 0..10 {
        batch(&w, &mut t, p, w.tick + 8, 32767, 0, 1);
    }
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.metrics.rate_limited_inputs, 10);
    assert_eq!(w.sessions.peers[&p].phase, SessionPhase::Playing);
    assert!(w.sessions.peers[&p].inputs.is_empty());
    assert!(!t.closed.contains(&p));
    for _ in 0..31 {
        step(&mut w, &mut a, &mut t);
    }
    let before = w.entities.view::<PlayerSnapshot>()[&player]
        .state
        .motor
        .position
        .0[0]
        .0;
    batch(&w, &mut t, p, w.tick + 1, 32767, 0, 1);
    step(&mut w, &mut a, &mut t);
    assert!(
        w.entities.view::<PlayerSnapshot>()[&player]
            .state
            .motor
            .position
            .0[0]
            .0
            > before
    );
    assert_eq!(w.metrics.rate_limited_inputs, 10);
}

mod restored_gameplay;

mod shirt_colour;

mod chat;

/// Verifies that public admission requires configured origin and issues secure same origin WebSocket.
#[test]
fn public_admission_requires_configured_origin_and_issues_secure_same_origin_ws() {
    let (w, _, _) = setup();
    let mut control = server::control::Control::new(w.core.build());
    control.public_origin = Some("https://game.example.test".into());
    assert_eq!(
        control
            .route(
                "POST",
                "/api/session",
                Some("http://localhost:4173"),
                None,
                b"{}",
                0
            )
            .code,
        403
    );
    assert_eq!(
        control
            .route(
                "POST",
                "/api/session",
                Some("https://evil.example.test"),
                None,
                b"{}",
                0
            )
            .code,
        403
    );
    let reply = control.route(
        "POST",
        "/api/session",
        Some("https://game.example.test"),
        None,
        b"{}",
        0,
    );
    assert_eq!(reply.code, 200);
    let cookie = reply.cookie.unwrap();
    assert!(cookie.contains("; Secure"));
    assert!(cookie.contains("HttpOnly; SameSite=Strict"));
    let secret = cookie.split(';').next().unwrap().split_once('=').unwrap().1;
    let ticket = control.route(
        "POST",
        "/api/join-ticket",
        Some("https://game.example.test"),
        Some(secret),
        b"{}",
        0,
    );
    assert_eq!(ticket.code, 200);
    assert_eq!(ticket.body["ws_url"], "wss://game.example.test/ws");
}

/// Verifies that all weapons damage players and preserve occupied vehicle protection.
#[test]
fn all_weapons_damage_players_and_preserve_occupied_vehicle_protection() {
    for definition in weapon_catalogue() {
        let (mut w, mut a, mut t) = setup();
        let p = peer(0);
        let q = peer(1);
        join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
        join(&mut w, &mut a, &mut t, q, None);
        ready(&mut w, &mut a, &mut t, q);
        let car = 1;
        let pos = w.entities.view::<VehicleSnapshot>()[&car]
            .state
            .motor
            .position
            .0
            .map(|v| v.0);
        let shooter = w.sessions.peers[&p].player.unwrap().0;
        let other = w.sessions.peers[&q].player.unwrap().0;
        w.entities
            .get_mut_by_id::<PlayerSnapshot>(&other)
            .unwrap()
            .lifecycle
            .protected_until = None;
        {
            let weapon = &mut w
                .entities
                .get_mut_by_id::<PlayerSnapshot>(&shooter)
                .unwrap()
                .weapon;
            weapon.slots[definition.id.index()].owned = true;
            weapon.slots[definition.id.index()].magazine = definition.capacity;
            weapon.equipped = definition.id;
        }
        w.entities
            .get_mut_by_id::<PlayerSnapshot>(&shooter)
            .unwrap()
            .state
            .motor = sim_core::spawn_character([pos[0], 0.02, pos[2] + 10.0], 0.0).unwrap();
        w.entities
            .get_mut_by_id::<PlayerSnapshot>(&other)
            .unwrap()
            .state
            .motor = sim_core::spawn_character([pos[0], 0.02, pos[2] + 5.0], 0.0).unwrap();
        fire(&w, &mut t, p, 1, 0.0);
        step(&mut w, &mut a, &mut t);
        assert_eq!(
            w.entities.view::<PlayerSnapshot>()[&other].state.health.0,
            100 - definition.damage
        );
        assert_eq!(
            w.entities.view::<VehicleSnapshot>()[&car].state.health.0,
            100
        );
        assert_eq!(
            w.entities.view::<PlayerSnapshot>()[&shooter].state.health.0,
            100
        );
        assert!(t.sent.iter().any(|(_, _, envelope)| matches!(
            &envelope.message,
            ServerMessage::WorldEvent(WorldEvent::Shot(shot))
                if shot.shooter_id.0 == shooter && shot.hit_entity_id == Some(EntityId(other))
        )));
        w.entities
            .get_mut_by_id::<PlayerSnapshot>(&shooter)
            .unwrap()
            .weapon
            .trigger = None;
        at_door(&mut w, q, car);
        vehicle_action(
            &w,
            &mut t,
            q,
            1,
            1,
            Action::EnterVehicle(EnterVehicle {
                vehicle_id: EntityId(car),
            }),
        );
        step(&mut w, &mut a, &mut t);
        for _ in 0..9 {
            step(&mut w, &mut a, &mut t);
        }
        fire(&w, &mut t, p, 2, 0.0);
        step(&mut w, &mut a, &mut t);
        assert_eq!(
            w.entities.view::<VehicleSnapshot>()[&car].state.health.0,
            100
        );
        assert_eq!(
            w.entities.view::<PlayerSnapshot>()[&other].state.health.0,
            100 - definition.damage
        );
    }
}

/// Verifies that equipment actions deduplicate and reject stale weapon or epoch.
#[test]
fn equipment_actions_deduplicate_and_reject_stale_weapon_or_epoch() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let id = w.sessions.peers[&p].player.unwrap().0;
    {
        let slot = &mut w
            .entities
            .get_mut_by_id::<PlayerSnapshot>(&id)
            .unwrap()
            .weapon
            .slots[1];
        slot.owned = true;
        slot.magazine = 20;
        slot.reserve = 40;
    }
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        1,
        Action::EquipWeapon(EquipWeapon {
            weapon_id: WeaponId::Rifle,
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.equipped,
        WeaponId::Rifle
    );
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        1,
        Action::EquipWeapon(EquipWeapon {
            weapon_id: WeaponId::Pistol,
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.equipped,
        WeaponId::Rifle
    );
    fire(&w, &mut t, p, 2, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.slots[w.entities.view::<PlayerSnapshot>()
            [&id]
            .weapon
            .equipped
            .index()]
        .magazine,
        20
    );
    for _ in 0..15 {
        step(&mut w, &mut a, &mut t);
    }
    vehicle_action(
        &w,
        &mut t,
        p,
        3,
        1,
        Action::Fire(Fire {
            weapon_id: WeaponId::Pistol,
            aim_yaw: Yaw(0.0),
            aim_pitch: Pitch(0.0),
        }),
    );
    step(&mut w, &mut a, &mut t);
    vehicle_action(
        &w,
        &mut t,
        p,
        4,
        2,
        Action::Fire(Fire {
            weapon_id: WeaponId::Rifle,
            aim_yaw: Yaw(0.0),
            aim_pitch: Pitch(0.0),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.slots[w.entities.view::<PlayerSnapshot>()
            [&id]
            .weapon
            .equipped
            .index()]
        .magazine,
        20
    );
    fire(&w, &mut t, p, 5, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.slots[w.entities.view::<PlayerSnapshot>()
            [&id]
            .weapon
            .equipped
            .index()]
        .magazine,
        19
    );
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id]
            .weapon
            .shot_sequence,
        1
    );
    vehicle_action(
        &w,
        &mut t,
        p,
        5,
        1,
        Action::Fire(Fire {
            weapon_id: WeaponId::Rifle,
            aim_yaw: Yaw(0.0),
            aim_pitch: Pitch(0.0),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.slots[w.entities.view::<PlayerSnapshot>()
            [&id]
            .weapon
            .equipped
            .index()]
        .magazine,
        19
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
    ready(&mut w, &mut a, &mut t, p);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.slots[w.entities.view::<PlayerSnapshot>()
            [&id]
            .weapon
            .equipped
            .index()]
        .magazine,
        19
    );
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.slots[w.entities.view::<PlayerSnapshot>()
            [&id]
            .weapon
            .equipped
            .index()]
        .reserve,
        40
    );
    assert_eq!(
        crate::component::snapshots::public_state(&w.entities.view::<PlayerSnapshot>()[&id])
            .equipment
            .unwrap()
            .weapon_id,
        WeaponId::Rifle
    );
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message, ServerMessage::PlayerStatus(status) if status.equipped_weapon==WeaponId::Rifle && status.inventory[1].magazine.0==19 && status.inventory[1].reserve==40)));
}

/// Verifies that automatic fire obeys server cadence lease timeout and release.
#[test]
fn automatic_fire_obeys_server_cadence_lease_timeout_and_release() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let id = w.sessions.peers[&p].player.unwrap().0;
    {
        let weapon = &mut w
            .entities
            .get_mut_by_id::<PlayerSnapshot>(&id)
            .unwrap()
            .weapon;
        weapon.slots[2].owned = true;
        weapon.slots[2].magazine = 50;
        weapon.equipped = WeaponId::MachineGun;
    }
    fire(&w, &mut t, p, 1, 0.0);
    for _ in 0..15 {
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.slots[w.entities.view::<PlayerSnapshot>()
            [&id]
            .weapon
            .equipped
            .index()]
        .magazine,
        45
    );
    for _ in 0..15 {
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.slots[w.entities.view::<PlayerSnapshot>()
            [&id]
            .weapon
            .equipped
            .index()]
        .magazine,
        45
    );
    fire(&w, &mut t, p, 2, 0.0);
    step(&mut w, &mut a, &mut t);
    send(
        &w,
        &mut t,
        p,
        ClientMessage::Trigger(Trigger {
            control_epoch: ControlEpoch(1),
            sequence: 3,
            held: false,
            pressed: false,
            weapon_id: WeaponId::MachineGun,
            aim_yaw: Yaw(0.0),
            aim_pitch: Pitch(0.0),
        }),
    );
    step(&mut w, &mut a, &mut t);
    let ammo = w.entities.view::<PlayerSnapshot>()[&id].weapon.slots[w
        .entities
        .view::<PlayerSnapshot>()[&id]
        .weapon
        .equipped
        .index()]
    .magazine;
    for _ in 0..10 {
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&id].weapon.slots[w.entities.view::<PlayerSnapshot>()
            [&id]
            .weapon
            .equipped
            .index()]
        .magazine,
        ammo
    );
}

/// Verifies that weapon presentation load 16 players.
#[test]
#[ignore = "measured synthetic 16-player weapon load; run in release with --nocapture"]
fn weapon_presentation_load_16_players() {
    let trace_path = std::env::var("PM_ECS_TRACE").ok();
    let mut records = Vec::<serde_json::Value>::new();
    /// Removes nonessential actors so the scenario can isolate shot geometry.
    fn scrub(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(key.as_str(), "world_id" | "session_id") {
                        *value = serde_json::Value::Null;
                    } else {
                        scrub(value);
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    scrub(value);
                }
            }
            _ => {}
        }
    }
    let (mut w, mut a, mut t) = if std::env::var("PM_ECS_MAP").as_deref() == Ok("city") {
        let city = WorldContent::load_index(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
            "city.json",
        )
        .unwrap();
        (
            World::new(city).unwrap(),
            Profiles::default(),
            FakeTransport::default(),
        )
    } else {
        setup()
    };
    let peers: Vec<_> = (0..16).map(peer).collect();
    for &p in &peers {
        join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
        let id = w.sessions.peers[&p].player.unwrap().0;
        let player = w.entities.get_mut_by_id::<PlayerSnapshot>(&id).unwrap();
        player.state.motor.position = Vec3([
            Scalar(-184.0 + (p.id % 4) as f32 * 2.0),
            Scalar(0.0),
            Scalar(180.0 + (p.id / 4) as f32 * 2.0),
        ]);
        player.weapon.slots[2].owned = true;
        player.weapon.equipped = WeaponId::MachineGun;
        player.lifecycle.protected_until = None;
    }
    t.sent.clear();
    let mut times = Vec::new();
    let mut bytes = 0usize;
    let mut shots = BTreeSet::new();
    for frame in 0..90 {
        // Test-only replenishment sustains the advertised peak, excluding reload downtime.
        for player in w.entities.values_mut::<PlayerSnapshot>() {
            player.weapon.slots[2].magazine = 50;
        }
        if frame % 3 == 0 {
            for &p in &peers {
                send(
                    &w,
                    &mut t,
                    p,
                    ClientMessage::Trigger(Trigger {
                        control_epoch: ControlEpoch(1),
                        sequence: frame / 3 + 1,
                        held: true,
                        pressed: frame == 0,
                        weapon_id: WeaponId::MachineGun,
                        aim_yaw: Yaw(0.0),
                        aim_pitch: Pitch(0.3),
                    }),
                );
            }
        }
        let started = std::time::Instant::now();
        step(&mut w, &mut a, &mut t);
        times.push(started.elapsed().as_secs_f64() * 1000.0);
        if trace_path.is_some() {
            records.push(serde_json::json!({"tick":w.tick,"entities":server::replication::Replication::entities(&mut w.core)}));
        }
        for (peer, delivery, message) in t.sent.drain(..) {
            if trace_path.is_some() {
                let mut wire = serde_json::to_value(&message).unwrap();
                scrub(&mut wire);
                records.push(serde_json::json!({"peer":peer.id,"delivery":format!("{delivery:?}"),"message":wire}));
            }
            bytes += serde_json::to_vec(&message).unwrap().len();
            if let ServerMessage::WorldEvent(WorldEvent::Shot(shot)) = message.message {
                shots.insert((shot.shooter_id.0, shot.shot_sequence));
            }
        }
    }
    if let Some(path) = trace_path {
        std::fs::write(path, serde_json::to_vec(&records).unwrap()).unwrap();
    }
    times.sort_by(f64::total_cmp);
    assert!(t.closed.is_empty());
    assert_eq!(shots.len(), 480);
    println!(
        "WEAPON_LOAD players=16 ticks=90 shots={} shots_per_second={} tick_p50_ms={:.3} tick_p95_ms={:.3} tick_max_ms={:.3} aggregate_json_egress_bytes_per_second={:.0}",
        shots.len(),
        shots.len() / 3,
        times[45],
        times[85],
        times[89],
        bytes as f64 / 3.0
    );
}

/// Verifies that player shots respect vehicle cover and skip dead or downed targets.
#[test]
fn player_shots_respect_vehicle_cover_and_skip_dead_or_downed_targets() {
    for (health, mode, behind_car) in [
        (100, CharacterMode::OnFoot, true),
        (0, CharacterMode::OnFoot, false),
        (100, CharacterMode::Downed, false),
        (10, CharacterMode::OnFoot, false),
    ] {
        let (mut w, mut a, mut t) = setup();
        let p = peer(0);
        let q = peer(1);
        join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
        join(&mut w, &mut a, &mut t, q, None);
        ready(&mut w, &mut a, &mut t, q);
        let pos = w.entities.view::<VehicleSnapshot>()[&1]
            .state
            .motor
            .position
            .0
            .map(|v| v.0);
        let shooter = w.sessions.peers[&p].player_id();
        let other = w.sessions.peers[&q].player_id();
        place(&mut w, p, [pos[0], 0.02, pos[2] + 10.0]);
        place(
            &mut w,
            q,
            [pos[0], 0.02, pos[2] + if behind_car { -5.0 } else { 5.0 }],
        );
        let target = w.entities.get_mut_by_id::<PlayerSnapshot>(&other).unwrap();
        target.lifecycle.protected_until = None;
        target.state.health = CharacterHealth(health);
        target.state.mode = mode;
        fire(&w, &mut t, p, 1, 0.0);
        step(&mut w, &mut a, &mut t);
        let eligible = !behind_car && health > 0 && mode == CharacterMode::OnFoot;
        assert_eq!(
            w.entities.view::<PlayerSnapshot>()[&other].state.health.0,
            if eligible { 0 } else { health }
        );
        assert_eq!(
            w.entities.view::<VehicleSnapshot>()[&1].state.health.0,
            if eligible { 100 } else { 75 }
        );
        assert_eq!(
            w.entities.view::<PlayerSnapshot>()[&shooter].state.health.0,
            100
        );
    }
}

/// Verifies that walls block player shots.
#[test]
fn walls_block_player_shots() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content");
    let mut w = World::new(WorldContent::load_index(&root, "city.json").unwrap()).unwrap();
    let mut a = Profiles::default();
    let mut t = FakeTransport::default();
    let p = peer(0);
    let q = peer(1);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    join(&mut w, &mut a, &mut t, q, None);
    ready(&mut w, &mut a, &mut t, q);
    place(&mut w, p, [-222.0, 0.02, 115.0]);
    place(&mut w, q, [-222.0, 0.02, 65.0]);
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        w.sessions.peers[&p].epoch,
        Action::Fire(Fire {
            weapon_id: WeaponId::Pistol,
            aim_yaw: Yaw(0.0),
            aim_pitch: Pitch(0.0),
        }),
    );
    step(&mut w, &mut a, &mut t);
    let other = w.sessions.peers[&q].player_id();
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&other].state.health.0,
        100
    );
    assert!(
        t.sent
            .iter()
            .any(|(_, _, envelope)| matches!(&envelope.message,
        ServerMessage::WorldEvent(WorldEvent::Shot(shot))
        if shot.hit_entity_id.is_none() && shot.end.0[2].0 > 65.0))
    );
}

/// Verifies that pvp death cancels control and respawns with full loadout and protection.
#[test]
fn pvp_death_cancels_control_and_respawns_with_full_loadout_and_protection() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    let q = peer(1);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    join(&mut w, &mut a, &mut t, q, None);
    ready(&mut w, &mut a, &mut t, q);
    let other = w.sessions.peers[&q].player_id();
    let pos = w.entities.view::<VehicleSnapshot>()[&1]
        .state
        .motor
        .position
        .0
        .map(|v| v.0);
    place(&mut w, p, [pos[0], 0.02, pos[2] + 10.0]);
    place(&mut w, q, [pos[0], 0.02, pos[2] + 5.0]);
    let epoch = w.sessions.peers[&q].epoch;
    let target = w.entities.get_mut_by_id::<PlayerSnapshot>(&other).unwrap();
    target.lifecycle.protected_until = None;
    target.state.health = CharacterHealth(10);
    target.weapon.reload_end = Some(u32::MAX);
    target.weapon.equip_end = Some(u32::MAX);
    fire(&w, &mut t, p, 1, 0.0);
    step(&mut w, &mut a, &mut t);
    let target = &w.entities.view::<PlayerSnapshot>()[&other];
    assert_eq!(target.state.mode, CharacterMode::Downed);
    assert_eq!(target.state.health.0, 0);
    assert!(target.weapon.reload_end.is_none());
    assert!(target.weapon.equip_end.is_none());
    assert!(target.weapon.trigger.is_none());
    assert_eq!(w.sessions.peers[&q].epoch, epoch + 1);
    let until = target.lifecycle.respawn.unwrap();
    let position = target.state.motor.position.clone();
    for recipient in [p, q] {
        let notices: Vec<_> = t.sent.iter().filter(|(peer, delivery, message)|
            *peer == recipient && *delivery == Delivery::Reliable && matches!(
                &message.message, ServerMessage::Chat(line)
                if line.system == Some(true) && line.text.0 == "Test Guest eliminated Test Guest"
            )).collect();
        assert_eq!(notices.len(), 1);
    }

    while w.tick < until - 1 {
        batch(&w, &mut t, q, w.tick + 1, 32767, 32767, epoch + 1);
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&other]
            .state
            .motor
            .position,
        position
    );
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&other]
            .lifecycle
            .respawn,
        Some(until)
    );
    step(&mut w, &mut a, &mut t);
    let target = &w.entities.view::<PlayerSnapshot>()[&other];
    assert_eq!(target.state.mode, CharacterMode::OnFoot);
    assert_eq!(target.state.health.0, 100);
    assert_eq!(target.lifecycle.protected_until, Some(w.tick + 90));
    assert!(target.lifecycle.respawn.is_none());
    assert_eq!(target.weapon.equipped, WeaponId::Pistol);
    for definition in weapon_catalogue() {
        let slot = &target.weapon.slots[definition.id.index()];
        assert!(slot.owned);
        assert_eq!(slot.magazine, definition.capacity);
        assert_eq!(slot.reserve, definition.max_reserve);
    }
    assert_eq!(w.sessions.peers[&q].epoch, epoch + 2);
    assert_eq!(
        t.sent
            .iter()
            .filter(|(_, _, message)| matches!(&message.message,
        ServerMessage::Chat(line) if line.text.0.contains(" eliminated ")))
            .count(),
        2
    );

    // Stale pre-death input cannot move the newly respawned character.
    let position = w.entities.view::<PlayerSnapshot>()[&other]
        .state
        .motor
        .position
        .clone();
    batch(&w, &mut t, q, w.tick + 1, 32767, 32767, epoch);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&other]
            .state
            .motor
            .position
            .0[0],
        position.0[0]
    );
    assert_eq!(
        w.entities.view::<PlayerSnapshot>()[&other]
            .state
            .motor
            .position
            .0[2],
        position.0[2]
    );
}

/// Verifies that spawn protection blocks shots until expiry or firing.
#[test]
fn spawn_protection_blocks_shots_until_expiry_or_firing() {
    for ends_by_firing in [false, true] {
        let (mut w, mut a, mut t) = setup();
        let p = peer(0);
        let q = peer(1);
        join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
        join(&mut w, &mut a, &mut t, q, None);
        ready(&mut w, &mut a, &mut t, q);
        let other = w.sessions.peers[&q].player_id();
        let pos = w.entities.view::<VehicleSnapshot>()[&1]
            .state
            .motor
            .position
            .0
            .map(|v| v.0);
        place(&mut w, p, [pos[0], 0.02, pos[2] + 10.0]);
        place(&mut w, q, [pos[0], 0.02, pos[2] + 5.0]);
        let expiry = w.entities.view::<PlayerSnapshot>()[&other]
            .lifecycle
            .protected_until
            .unwrap();
        fire(&w, &mut t, p, 1, 0.0);
        step(&mut w, &mut a, &mut t);
        assert_eq!(
            w.entities.view::<PlayerSnapshot>()[&other].state.health.0,
            100
        );
        assert_eq!(w.entities.view::<VehicleSnapshot>()[&1].state.health.0, 100);
        if ends_by_firing {
            fire(&w, &mut t, q, 1, std::f32::consts::FRAC_PI_2);
            step(&mut w, &mut a, &mut t);
            assert!(w.tick < expiry);
        } else {
            while w.tick < expiry {
                step(&mut w, &mut a, &mut t);
            }
        }
        assert!(
            w.entities.view::<PlayerSnapshot>()[&other]
                .lifecycle
                .protected_until
                .is_none()
        );
        for _ in 0..9 {
            step(&mut w, &mut a, &mut t);
        }
        fire(&w, &mut t, p, 2, 0.0);
        step(&mut w, &mut a, &mut t);
        assert_eq!(
            w.entities.view::<PlayerSnapshot>()[&other].state.health.0,
            75
        );
        assert_eq!(w.entities.view::<VehicleSnapshot>()[&1].state.health.0, 100);
    }
}

/// Verifies that player map includes distant online players and removes disconnected players.
#[test]
fn player_map_includes_distant_online_players_and_removes_disconnected_players() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    let q = peer(1);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    join(&mut w, &mut a, &mut t, q, None);
    ready(&mut w, &mut a, &mut t, q);
    let other = w.sessions.peers[&q].player_id();
    place(&mut w, q, [300.0, 0.02, 300.0]);
    w.entities
        .get_mut::<PlayerSnapshot>(EntityId(other))
        .unwrap()
        .display_name = DisplayName("Distant player".into());
    send(
        &w,
        &mut t,
        q,
        ClientMessage::Ping(Ping {
            nonce: 80,
            client_time_ms: Milliseconds(0.0),
            reported_rtt_ms: Some(Milliseconds(72.4)),
        }),
    );
    for _ in 0..30 {
        step(&mut w, &mut a, &mut t);
    }
    let entries = t
        .sent
        .iter()
        .rev()
        .find_map(|(peer, _, message)| {
            if *peer != p {
                return None;
            }
            match &message.message {
                ServerMessage::PlayerMap(entries) => Some(entries),
                _ => None,
            }
        })
        .unwrap();
    assert_eq!(entries.len(), 2);
    let remote = entries
        .iter()
        .find(|entry| entry.player_id.0 == other)
        .unwrap();
    assert_eq!(remote.display_name.0, "Distant player");
    assert_eq!(remote.ping_ms, Some(72));
    assert!(
        entries
            .iter()
            .any(|entry| entry.player_id.0 == w.sessions.peers[&p].player_id())
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.player_id.0 == other && entry.position.0[0].0 > 299.0)
    );
    // Cosmetic reports are bounded, and old readings expire even with live keepalives.
    send(
        &w,
        &mut t,
        q,
        ClientMessage::Ping(Ping {
            nonce: 81,
            client_time_ms: Milliseconds(0.0),
            reported_rtt_ms: Some(Milliseconds(20_000.0)),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.sessions.peers[&q].reported_ping.map(|(ping, _)| ping),
        Some(72)
    );
    for i in 0..330 {
        if i % 30 == 0 {
            for peer in [p, q] {
                send(
                    &w,
                    &mut t,
                    peer,
                    ClientMessage::Ping(Ping {
                        nonce: 100 + i,
                        client_time_ms: Milliseconds(0.0),
                        reported_rtt_ms: None,
                    }),
                );
            }
        }
        step(&mut w, &mut a, &mut t);
    }
    let entries = t
        .sent
        .iter()
        .rev()
        .find_map(|(peer, _, message)| {
            if *peer != p {
                return None;
            }
            match &message.message {
                ServerMessage::PlayerMap(entries) => Some(entries),
                _ => None,
            }
        })
        .unwrap();
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.player_id.0 == other)
            .unwrap()
            .ping_ms,
        None
    );
    w.disconnect(q, &mut a);
    for _ in 0..30 {
        step(&mut w, &mut a, &mut t);
    }
    let entries = t
        .sent
        .iter()
        .rev()
        .find_map(|(peer, _, message)| {
            if *peer != p {
                return None;
            }
            match &message.message {
                ServerMessage::PlayerMap(entries) => Some(entries),
                _ => None,
            }
        })
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries.iter().all(|entry| entry.player_id.0 != other));
}

mod replication_publication;

mod player_lifecycle;
