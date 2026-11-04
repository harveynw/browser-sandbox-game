use crate::admission::Profiles;
use crate::transport::*;
use crate::{
    sessions::{Session, SessionPhase, Sessions},
    systems::{
        Systems, missions::MissionSystem, police::PoliceSystem, population::PopulationSystem,
    },
    world::World,
};

use crate::world::entity_position;
use protocol::*;
use std::collections::{BTreeMap, BTreeSet};

pub struct Replication;
impl Replication {
    pub(crate) fn send(
        peer: PeerKey,
        sid: Option<&str>,
        delivery: Delivery,
        message: ServerMessage,
        world: &mut World,
        sessions: &mut Sessions,
    ) {
        sessions.output.push_back((
            peer,
            delivery,
            ServerEnvelope {
                v: ProtocolVersion(4),
                world_id: Some(OpaqueId(world.id.clone())),
                session_id: sid.map(|s| OpaqueId(s.into())),
                message,
            },
        ));
    }
    pub(crate) fn reliable(
        peer: PeerKey,
        sid: Option<&str>,
        message: ServerMessage,
        world: &mut World,
        sessions: &mut Sessions,
    ) {
        Replication::send(peer, sid, Delivery::Reliable, message, world, sessions);
    }
    pub(crate) fn owner(session: &Session, world: &World) -> Result<OwnerState, String> {
        let id = session.player.ok_or("missing owner")?;
        let player = world.players.get(&id.0).ok_or("missing player")?;
        Ok(OwnerState {
            server_tick: world.tick,
            consumed_through_tick: world.tick,
            control_epoch: ControlEpoch(session.epoch),
            player_id: id,
            controlled_entity_id: player.state.vehicle_id.unwrap_or(id),
            state: player
                .state
                .vehicle_id
                .map(|car| EntityState::Vehicle(world.vehicles[&car.0].state.clone()))
                .unwrap_or_else(|| EntityState::Character(player.public_state())),
            player_state: player.public_state(),
        })
    }
    pub(crate) fn entities(
        world: &World,
        population: &PopulationSystem,
        police: &PoliceSystem,
    ) -> BTreeMap<u32, (EntityDescriptor, EntityState)> {
        world
            .players
            .iter()
            .map(|(&id, p)| {
                (
                    id,
                    (
                        EntityDescriptor {
                            tshirt_colour: Some(p.tshirt_colour),
                            id: p.id,
                            entity_kind: EntityKind::Player,
                            template_id: TemplateId("player_greybox".into()),
                            display_name: Some(p.display_name.clone()),
                        },
                        EntityState::Character(p.public_state()),
                    ),
                )
            })
            .chain(world.vehicles.iter().map(|(&id, v)| {
                (
                    id,
                    (
                        EntityDescriptor {
                            tshirt_colour: None,
                            id: v.id,
                            entity_kind: if police.units.contains_key(&id) {
                                EntityKind::Police
                            } else {
                                EntityKind::Vehicle
                            },
                            template_id: TemplateId(v.template.id.clone()),
                            display_name: if police.secured.contains(&id) {
                                Some(DisplayName(format!("Secured {}", v.template.id)))
                            } else if police.units.contains_key(&id) {
                                Some(DisplayName("PoliceSystem patrol".into()))
                            } else {
                                None
                            },
                        },
                        EntityState::Vehicle(v.state.clone()),
                    ),
                )
            }))
            .chain(population.pedestrians.iter().map(|(&id, p)| {
                (
                    id,
                    (
                        EntityDescriptor {
                            tshirt_colour: None,
                            id: p.id,
                            entity_kind: EntityKind::Pedestrian,
                            template_id: TemplateId("pedestrian_greybox".into()),
                            display_name: None,
                        },
                        EntityState::Character(p.state.clone()),
                    ),
                )
            }))
            .collect()
    }
    pub(crate) fn show(
        id: u32,
        visibility: u32,
        baseline: Option<u32>,
        world: &World,
        population: &PopulationSystem,
        police: &PoliceSystem,
    ) -> EntityShow {
        let (descriptor, state) = Replication::entities(world, population, police)
            .remove(&id)
            .expect("visible entity exists");
        EntityShow {
            baseline_id: baseline.map(BaselineId),
            visibility_id: VisibilityId(visibility),
            descriptor,
            state,
            state_tick: world.tick,
        }
    }
    pub(crate) fn controlled(s: &Session, world: &World) -> u32 {
        let player = s.player.expect("authenticated player");
        world.players[&player.0]
            .state
            .vehicle_id
            .unwrap_or(player)
            .0
    }
    pub(crate) fn transition(
        peer: PeerKey,
        s: &mut Session,
        ids: &[u32],
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
    ) -> Result<(), String> {
        s.epoch = s.epoch.checked_add(1).ok_or("epoch exhausted")?;
        if let Some(player) = s.player.and_then(|id| world.players.get_mut(&id.0)) {
            player.weapon.trigger = None;
        }
        s.inputs.clear();
        // Refresh both replicas reliably with a fresh visibility generation. Older RTC
        // packets cannot restore a previous occupancy mode, even at the same tick.
        let entities = Replication::entities(world, population, police);
        let mut messages = Vec::new();
        for (target, session) in
            std::iter::once((peer, s)).chain(sessions.peers.iter_mut().map(|(p, s)| (*p, s)))
        {
            for id in ids {
                if !session.visible.contains_key(id) {
                    continue;
                }
                session.visibility_counter = session
                    .visibility_counter
                    .checked_add(1)
                    .ok_or("visibility exhausted")?;
                session.visible.insert(*id, session.visibility_counter);
                let (descriptor, state) = entities[id].clone();
                messages.push((
                    target,
                    session.id.clone(),
                    ServerMessage::EntityShow(EntityShow {
                        baseline_id: None,
                        visibility_id: VisibilityId(session.visibility_counter),
                        descriptor,
                        state,
                        state_tick: world.tick,
                    }),
                ));
            }
        }
        for (target, sid, message) in messages {
            Replication::reliable(target, sid.as_deref(), message, world, sessions);
        }
        Ok(())
    }
    /// Refresh descriptors reliably without changing control epochs or discarding player inputs.
    pub(crate) fn refresh_appearance(
        ids: &[u32],
        world: &mut World,
        sessions: &mut Sessions,
        systems: &Systems,
    ) -> Result<(), String> {
        let entities = Self::entities(world, &systems.population, &systems.police);
        let mut messages = Vec::new();
        for (&peer, session) in &mut sessions.peers {
            for id in ids {
                if !session.visible.contains_key(id) {
                    continue;
                }
                session.visibility_counter = session
                    .visibility_counter
                    .checked_add(1)
                    .ok_or("visibility exhausted")?;
                session.visible.insert(*id, session.visibility_counter);
                let (descriptor, state) = entities[id].clone();
                messages.push((
                    peer,
                    session.id.clone(),
                    ServerMessage::EntityShow(EntityShow {
                        baseline_id: None,
                        visibility_id: VisibilityId(session.visibility_counter),
                        descriptor,
                        state,
                        state_tick: world.tick,
                    }),
                ));
            }
        }
        for (peer, sid, message) in messages {
            Self::reliable(peer, sid.as_deref(), message, world, sessions);
        }
        Ok(())
    }
    pub(crate) fn status(
        tick: u32,
        s: &Session,
        world: &World,
        police: &PoliceSystem,
        missions: &MissionSystem,
    ) -> Result<PlayerStatus, String> {
        let p = s.profile.as_ref().ok_or("profile missing")?;
        Ok(PlayerStatus {
            equipped_weapon: world.players[&s.player_id()].weapon.equipped,
            inventory: world.players[&s.player_id()].weapon.views(),
            equip_end_tick: world.players[&s.player_id()].weapon.equip_end,
            revision: Revision(s.status_revision),
            server_tick: tick,
            credits: p.credits,
            repair: s.repair_view.clone(),
            completed_jobs: p.completed_jobs,
            heat: Heat(police.wanted(s.player_id()).heat),
            pursuit: police.wanted(s.player_id()).pursuit,
            magazine: Magazine(world.players[&s.player_id()].weapon.magazine),
            reload_end_tick: world.players[&s.player_id()].weapon.reload_end,
            mission: missions.mission(s.player_id()).as_ref().map(|m| m.view()),
            respawn_tick: world.players[&s.player_id()].lifecycle.respawn,
            protected_until_tick: world.players[&s.player_id()].lifecycle.protected_until,
        })
    }
    pub(crate) fn baseline(
        peer: PeerKey,
        s: &mut Session,
        now: u64,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
    ) -> Result<(), String> {
        s.baseline = s.baseline.checked_add(1).ok_or("baseline exhausted")?;
        s.phase = SessionPhase::Loading;
        s.inputs.clear();
        s.visible.clear();
        s.deadline = now.saturating_add(30_000);
        Replication::reliable(
            peer,
            s.id.as_deref(),
            ServerMessage::BaselineBegin(BaselineBegin {
                baseline_id: BaselineId(s.baseline),
                server_tick: world.tick,
            }),
            world,
            sessions,
        );
        s.last_replicated.clear();
        let entities = Replication::entities(world, population, police);
        let spatial = SpatialIndex::new(entities.iter().map(|(&id, (_, state))| {
            let p = entity_position(state);
            (id, [p[0], p[2]])
        }));
        let owned = Replication::controlled(s, world);
        let origin = entity_position(&entities[&owned].1);
        let mut visible = spatial.relevant([origin[0], origin[2]], owned, &s.visible);
        visible.insert(s.player.ok_or("missing player")?.0);
        for id in visible {
            s.visibility_counter = s
                .visibility_counter
                .checked_add(1)
                .ok_or("visibility exhausted")?;
            s.visible.insert(id, s.visibility_counter);
            Replication::reliable(
                peer,
                s.id.as_deref(),
                ServerMessage::EntityShow(Replication::show(
                    id,
                    s.visibility_counter,
                    Some(s.baseline),
                    world,
                    population,
                    police,
                )),
                world,
                sessions,
            );
        }
        Replication::reliable(
            peer,
            s.id.as_deref(),
            ServerMessage::BaselineEnd(BaselineEnd {
                baseline_id: BaselineId(s.baseline),
                entity_count: EntityCount(s.visible.len() as u16),
            }),
            world,
            sessions,
        );
        Ok(())
    }
    pub(crate) fn sync_visible(
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
    ) -> Result<(), String> {
        let entities = Replication::entities(world, population, police);
        let spatial = SpatialIndex::new(entities.iter().map(|(&id, (_, state))| {
            let p = entity_position(state);
            (id, [p[0], p[2]])
        }));
        let mut messages = Vec::new();
        for (&peer, s) in &mut sessions.peers {
            if s.phase == SessionPhase::AwaitHello {
                continue;
            }
            let player = s.player.ok_or("missing observer")?.0;
            let owner = world.players[&player]
                .state
                .vehicle_id
                .map_or(player, |v| v.0);
            let origin = entity_position(&entities[&owner].1);
            let mut present = spatial.relevant([origin[0], origin[2]], owner, &s.visible);
            present.insert(player);
            for id in s.visible.keys().copied().collect::<Vec<_>>() {
                if !present.contains(&id) {
                    let visibility = s.visible.remove(&id).ok_or("visibility missing")?;
                    s.last_replicated.remove(&id);
                    messages.push((
                        peer,
                        s.id.clone(),
                        ServerMessage::EntityHide(EntityHide {
                            entity_id: EntityId(id),
                            visibility_id: VisibilityId(visibility),
                            server_tick: world.tick,
                            reason: if entities.contains_key(&id) {
                                HideReason::OutOfRange
                            } else {
                                HideReason::Disconnected
                            },
                        }),
                    ));
                }
            }
            for id in &present {
                if !s.visible.contains_key(id) {
                    s.visibility_counter = s
                        .visibility_counter
                        .checked_add(1)
                        .ok_or("visibility exhausted")?;
                    s.visible.insert(*id, s.visibility_counter);
                    // Build after releasing the mutable session borrow.
                    messages.push((
                        peer,
                        s.id.clone(),
                        ServerMessage::EntityShow(EntityShow {
                            baseline_id: None,
                            visibility_id: VisibilityId(s.visibility_counter),
                            descriptor: entities[id].0.clone(),
                            state: entities[id].1.clone(),
                            state_tick: world.tick,
                        }),
                    ));
                }
            }
        }
        for (peer, sid, message) in messages {
            Replication::reliable(peer, sid.as_deref(), message, world, sessions);
        }
        Ok(())
    }
    pub(crate) fn publish(
        world: &mut World,
        sessions: &mut Sessions,
        systems: &mut Systems,
    ) -> Result<(), String> {
        if world.tick.is_multiple_of(30) {
            let players: Vec<_> = sessions
                .peers
                .values()
                .filter(|s| s.phase == SessionPhase::Playing)
                .filter_map(|s| s.player.and_then(|id| world.players.get(&id.0)))
                .map(|p| PlayerMapEntry {
                    player_id: p.id,
                    position: p
                        .state
                        .vehicle_id
                        .and_then(|id| world.vehicles.get(&id.0))
                        .map_or_else(
                            || p.state.motor.position.clone(),
                            |v| v.state.motor.position.clone(),
                        ),
                })
                .collect();
            let recipients: Vec<_> = sessions
                .peers
                .iter()
                .filter(|(_, s)| s.phase == SessionPhase::Playing)
                .map(|(&peer, s)| (peer, s.id.clone()))
                .collect();
            for (peer, sid) in recipients {
                Self::reliable(
                    peer,
                    sid.as_deref(),
                    ServerMessage::PlayerMap(players.clone()),
                    world,
                    sessions,
                );
            }
        }
        Replication::sync_visible(
            world,
            sessions,
            &mut systems.population,
            &mut systems.police,
        )?;
        if world.tick.is_multiple_of(3) {
            let updates: Result<Vec<_>, String> = sessions
                .peers
                .iter()
                .filter(|(_, s)| s.phase == SessionPhase::Playing)
                .map(|(peer, s)| Ok((*peer, s.id.clone(), Replication::owner(s, world)?)))
                .collect();
            for (peer, sid, owner) in updates? {
                Replication::send(
                    peer,
                    sid.as_deref(),
                    Delivery::Unreliable,
                    ServerMessage::OwnerState(owner),
                    world,
                    sessions,
                );
            }
        }
        if world.tick.is_multiple_of(3) {
            let mut batches = Vec::new();
            let entities =
                Replication::entities(world, &mut systems.population, &mut systems.police);
            for (&peer, session) in &mut sessions.peers {
                if session.phase != SessionPhase::Playing {
                    continue;
                }
                let player = session.player.ok_or("missing owner")?.0;
                let owned = world.players[&player]
                    .state
                    .vehicle_id
                    .map_or(player, |v| v.0);
                let position = entity_position(&entities[&owned].1);
                let mut ids: Vec<_> = session
                    .visible
                    .keys()
                    .copied()
                    .filter(|id| *id != owned)
                    .filter(|id| {
                        world.vehicles.get(id).is_none_or(|v| {
                            v.idle_ticks < 30
                                || v.state.driver_id.is_some()
                                || world.tick.is_multiple_of(15)
                        })
                    })
                    .collect();
                ids.sort_by(|a, b| {
                    let age_a = world
                        .tick
                        .saturating_sub(*session.last_replicated.get(a).unwrap_or(&world.tick));
                    let age_b = world
                        .tick
                        .saturating_sub(*session.last_replicated.get(b).unwrap_or(&world.tick));
                    let dist = |id: &u32| {
                        let p = entity_position(&entities[id].1);
                        (p[0] - position[0]).hypot(p[2] - position[2])
                    };
                    (age_b >= 9)
                        .cmp(&(age_a >= 9))
                        .then_with(|| {
                            if age_a >= 9 && age_b >= 9 {
                                age_b.cmp(&age_a)
                            } else {
                                std::cmp::Ordering::Equal
                            }
                        })
                        .then(dist(a).total_cmp(&dist(b)))
                        .then(a.cmp(b))
                });
                let records = ids.into_iter().map(|id| StateRecord {
                    entity_id: EntityId(id),
                    visibility_id: VisibilityId(session.visible[&id]),
                    state: entities[&id].1.clone(),
                });
                for envelope in packets(
                    &world.id,
                    session.id.as_deref().ok_or("missing session")?,
                    world.tick,
                    records,
                )? {
                    batches.push((peer, Delivery::Unreliable, envelope));
                }
                if world.tick.is_multiple_of(30) {
                    batches.push((
                        peer,
                        Delivery::Reliable,
                        ServerEnvelope {
                            v: ProtocolVersion(4),
                            world_id: Some(OpaqueId(world.id.clone())),
                            session_id: session.id.clone().map(OpaqueId),
                            message: ServerMessage::PlayerStatus(Replication::status(
                                world.tick,
                                session,
                                world,
                                &mut systems.police,
                                &mut systems.missions,
                            )?),
                        },
                    ));
                }
            }
            sessions.output.extend(batches);
        }
        Ok(())
    }
    pub(crate) fn flush(
        network: &mut Network,
        profiles: &mut Profiles,
        now: u64,
        previous_cars: &BTreeMap<u32, VehicleMotor>,
        world: &mut World,
        sessions: &mut Sessions,
        systems: &mut Systems,
    ) -> Result<(), String> {
        let mut flushed = 0;
        while let Some((peer, delivery, message)) = sessions.output.pop_front() {
            // A failed reliable send retires this session immediately. Never send the
            // tail of its baseline after dropping an earlier show record.
            if !sessions.peers.contains_key(&peer)
                && !matches!(
                    message.message,
                    ServerMessage::Reject(_) | ServerMessage::Disconnect(_)
                )
            {
                continue;
            }
            flushed += 1;
            if flushed > 4096 {
                return Err("tick output budget exhausted".into());
            }
            let bytes = encode_server(&message, delivery)
                .map_err(|e| e.to_string())?
                .len();
            let is_snapshot = matches!(message.message, ServerMessage::Snapshot(_));
            if let Some(session) = sessions.peers.get_mut(&peer) {
                if !session.egress_budget.allow(
                    bytes,
                    now,
                    if is_snapshot { LIFECYCLE_RESERVE } else { 0 },
                ) {
                    world.metrics.budget_drops += 1;
                    if delivery == Delivery::Reliable {
                        sessions.disconnect(peer, profiles, world, systems);
                        sessions
                            .closing
                            .insert(peer, "reliable byte budget exhausted");
                    }
                    continue;
                }
                session.traffic.down_bytes += bytes as u64;
                if let ServerMessage::Snapshot(data) = &message.message {
                    world.metrics.snapshot_packets += 1;
                    world.metrics.max_snapshot_bytes = world.metrics.max_snapshot_bytes.max(bytes);
                    for record in &data.records {
                        let previous = session
                            .last_replicated
                            .insert(record.entity_id.0, world.tick)
                            .unwrap_or(world.tick);
                        world.metrics.max_refresh_age_ticks = world
                            .metrics
                            .max_refresh_age_ticks
                            .max(world.tick - previous);
                    }
                }
            }
            world.metrics.egress_bytes += bytes as u64;
            if matches!(
                network.send(peer, delivery, message),
                SendOutcome::Full | SendOutcome::Closed
            ) {
                sessions.disconnect(peer, profiles, world, systems);
                sessions.closing.insert(peer, "output unavailable");
                systems
                    .repairs
                    .update(profiles, world, sessions, &mut systems.police)?;
                systems.missions.update(
                    profiles,
                    previous_cars,
                    world,
                    sessions,
                    &mut systems.population,
                    &mut systems.police,
                    &mut systems.combat,
                    &mut systems.repairs,
                )?;
                Replication::sync_visible(
                    world,
                    sessions,
                    &mut systems.population,
                    &mut systems.police,
                )?;
            }
        }
        for (peer, reason) in std::mem::take(&mut sessions.closing) {
            network.disconnect(peer, reason);
        }
        Ok(())
    }
}
// Full independent records, bounded bytes and spatial relevance; no delta dependencies.

use std::result::Result;
/// Keep a bounded baseline/lifecycle burst available while snapshots saturate the link.
/// This reserves tokens inside the existing 60 KiB/s, 120 KiB burst budget.
pub const LIFECYCLE_RESERVE: usize = 64 * 1024;
#[derive(Debug)]
pub struct Budget {
    tokens: f64,
    at: u64,
    rate: f64,
    capacity: f64,
}
impl Default for Budget {
    fn default() -> Self {
        Self::new(60 * 1024, 120 * 1024)
    }
}
impl Budget {
    pub fn new(rate: u32, burst: u32) -> Self {
        Self {
            tokens: f64::from(burst),
            at: 0,
            rate: f64::from(rate),
            capacity: f64::from(burst),
        }
    }
    pub fn allow(&mut self, bytes: usize, now: u64, reserve: usize) -> bool {
        self.tokens = (self.tokens + now.saturating_sub(self.at) as f64 * self.rate / 1000.0)
            .min(self.capacity);
        self.at = now;
        if self.tokens < (bytes + reserve) as f64 {
            return false;
        }
        self.tokens -= bytes as f64;
        true
    }
}
/// Strict rolling one-second admission cap, independent of queue draining cadence.
#[derive(Default)]
pub struct InputRate {
    arrivals: std::collections::VecDeque<u64>,
}
impl InputRate {
    pub fn allow(&mut self, now: u64) -> bool {
        while self
            .arrivals
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= 1000)
        {
            self.arrivals.pop_front();
        }
        if self.arrivals.len() >= 60 {
            return false;
        }
        self.arrivals.push_back(now);
        true
    }
}
type CellContents = Vec<(u32, [f32; 2])>;
pub struct SpatialIndex {
    cells: BTreeMap<(i32, i32), CellContents>,
}
impl SpatialIndex {
    pub fn new(points: impl Iterator<Item = (u32, [f32; 2])>) -> Self {
        let mut cells: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for (id, point) in points {
            cells.entry(cell(point)).or_default().push((id, point));
        }
        Self { cells }
    }
    pub fn relevant(
        &self,
        center: [f32; 2],
        owner: u32,
        previous: &BTreeMap<u32, u32>,
    ) -> BTreeSet<u32> {
        let (cx, cz) = cell(center);
        let mut candidates = Vec::new();
        for x in cx - 3..=cx + 3 {
            for z in cz - 3..=cz + 3 {
                if let Some(points) = self.cells.get(&(x, z)) {
                    for &(id, p) in points {
                        let d = (p[0] - center[0]).hypot(p[1] - center[1]);
                        if id == owner
                            || d <= if previous.contains_key(&id) {
                                192.0
                            } else {
                                160.0
                            }
                        {
                            candidates.push((id, d));
                        }
                    }
                }
            }
        }
        candidates.sort_by(|a, b| {
            (a.0 != owner)
                .cmp(&(b.0 != owner))
                .then(a.1.total_cmp(&b.1))
                .then(a.0.cmp(&b.0))
        });
        candidates.into_iter().take(96).map(|(id, _)| id).collect()
    }
}
fn cell(p: [f32; 2]) -> (i32, i32) {
    ((p[0] / 64.0).floor() as i32, (p[1] / 64.0).floor() as i32)
}
pub fn snapshot(
    world: &str,
    session: &str,
    tick: u32,
    records: Vec<StateRecord>,
) -> ServerEnvelope {
    ServerEnvelope {
        v: ProtocolVersion(4),
        world_id: Some(OpaqueId(world.into())),
        session_id: Some(OpaqueId(session.into())),
        message: ServerMessage::Snapshot(Snapshot {
            server_tick: tick,
            records,
        }),
    }
}
/// Pack by the real MessagePack encoder, including scope/header overhead. Never split a record.
pub fn packets(
    world: &str,
    session: &str,
    tick: u32,
    records: impl IntoIterator<Item = StateRecord>,
) -> Result<Vec<ServerEnvelope>, String> {
    let mut result = Vec::new();
    let mut pending = Vec::new();
    for record in records {
        let mut candidate = pending.clone();
        candidate.push(record.clone());
        if encode_server(
            &snapshot(world, session, tick, candidate.clone()),
            Delivery::Unreliable,
        )
        .is_ok()
        {
            pending = candidate;
        } else {
            if pending.is_empty() {
                return Err("single snapshot record exceeds RTC byte budget".into());
            }
            result.push(snapshot(world, session, tick, std::mem::take(&mut pending)));
            encode_server(
                &snapshot(world, session, tick, vec![record.clone()]),
                Delivery::Unreliable,
            )
            .map_err(|e| e.to_string())?;
            pending.push(record);
        }
    }
    if !pending.is_empty() {
        result.push(snapshot(world, session, tick, pending));
    }
    Ok(result)
}
/// Coalesce per entity/visibility, preserving same-tick packet splits and unrelated older records.
#[derive(Default)]
pub struct SnapshotBuffer {
    scope: Option<(String, String)>,
    records: BTreeMap<u32, (u32, StateRecord)>,
}
impl SnapshotBuffer {
    pub fn pending_records(&self) -> usize {
        self.records.len()
    }
    pub fn clear(&mut self) {
        self.scope = None;
        self.records.clear();
    }
    pub fn hide(&mut self, entity: u32, visibility: u32) {
        if self
            .records
            .get(&entity)
            .is_some_and(|(_, r)| r.visibility_id.0 <= visibility)
        {
            self.records.remove(&entity);
        }
    }
    pub fn push(&mut self, envelope: &ServerEnvelope) -> Result<bool, String> {
        let ServerMessage::Snapshot(data) = &envelope.message else {
            return Err("expected snapshot".into());
        };
        let scope = (
            envelope.world_id.as_ref().ok_or("missing world")?.0.clone(),
            envelope
                .session_id
                .as_ref()
                .ok_or("missing session")?
                .0
                .clone(),
        );
        if self.scope.as_ref().is_some_and(|s| *s != scope) {
            self.clear();
        }
        self.scope = Some(scope);
        let mut replaced = false;
        for record in &data.records {
            if let Some((tick, old)) = self.records.get(&record.entity_id.0) {
                if record.visibility_id.0 < old.visibility_id.0
                    || (record.visibility_id == old.visibility_id && data.server_tick <= *tick)
                {
                    continue;
                }
                replaced = true;
            } else if self.records.len() >= 96 {
                return Err("snapshot queue entity cap".into());
            }
            self.records
                .insert(record.entity_id.0, (data.server_tick, record.clone()));
        }
        Ok(replaced)
    }
    pub fn drain(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let Some((world, session)) = self.scope.take() else {
            return Ok(Vec::new());
        };
        let mut groups: BTreeMap<u32, Vec<StateRecord>> = BTreeMap::new();
        for (_, (tick, record)) in std::mem::take(&mut self.records) {
            groups.entry(tick).or_default().push(record);
        }
        let mut bytes = Vec::new();
        for (tick, records) in groups {
            for envelope in packets(&world, &session, tick, records)? {
                bytes.push(
                    encode_server(&envelope, Delivery::Unreliable).map_err(|e| e.to_string())?,
                );
            }
        }
        Ok(bytes)
    }
}
