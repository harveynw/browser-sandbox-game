use crate::{
    admission::{MemoryProfiles, random_id},
    content_loader::WorldContent,
    replication::{Budget, InputRate, SpatialIndex, packets},
    transport::*,
};
use protocol::*;
use sim_core::{CollisionQueries, CollisionWorld, QueryFilter, SimConfig, StepContext};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::result::Result;
pub const BUILD_ID: &str = "t10-local";
pub const MAX_PLAYERS: usize = 16;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPhase {
    AwaitHello,
    Loading,
    Playing,
}
pub struct Player {
    pub id: EntityId,
    pub state: CharacterState,
    pub display_name: DisplayName,
    slot: usize,
}
pub struct Session {
    pub id: Option<String>,
    pub profile: Option<ProfileView>,
    pub player: Option<EntityId>,
    pub phase: SessionPhase,
    pub epoch: u32,
    pub baseline: u32,
    visibility_counter: u32,
    visible: BTreeMap<u32, u32>,
    pub inputs: BTreeMap<u32, InputFrame>,
    actions: BTreeMap<u32, ActionResult>,
    last_request: u32,
    last_heard: u64,
    deadline: u64,
    last_resync: Option<u64>,
    ingress_budget: Budget,
    egress_budget: Budget,
    input_rate: InputRate,
    action_rate: Budget,
    last_replicated: BTreeMap<u32, u32>,
}
#[derive(Default, Debug)]
pub struct Metrics {
    pub late_inputs: u64,
    pub future_inputs: u64,
    pub conflicting_inputs: u64,
    pub stale_epochs: u64,
    pub ingress_bytes: u64,
    pub egress_bytes: u64,
    pub budget_drops: u64,
    pub snapshot_packets: u64,
    pub max_snapshot_bytes: usize,
    pub max_refresh_age_ticks: u32,
}
pub struct World {
    pub tick: u32,
    pub id: String,
    pub sessions: BTreeMap<PeerKey, Session>,
    pub players: BTreeMap<u32, Player>,
    pub metrics: Metrics,
    pub vehicles: BTreeMap<u32, crate::vehicles::Vehicle>,
    content: WorldContent,
    config: SimConfig,
    collision: CollisionWorld,
    next_entity: u32,
    output: VecDeque<(PeerKey, Delivery, ServerEnvelope)>,
    closing: BTreeMap<PeerKey, &'static str>,
}
impl World {
    pub fn new(content: WorldContent) -> Result<Self, String> {
        let config = SimConfig::from_content(&content.config)?;
        let mut collision = CollisionWorld::from_content(&content.collision, &config)?;
        // Reserve and verify all local player positions at boot, before readiness.
        for slot in 0..MAX_PLAYERS {
            let spawn = spawn_position(&content, slot)?;
            if collision.overlaps_character(spawn, QueryFilter::MOVEMENT)? {
                return Err("player spawn obstructed".into());
            }
        }
        let mut vehicles = BTreeMap::new();
        for parking in &content.manifest.parking {
            let template = content
                .manifest
                .vehicle_templates
                .iter()
                .find(|t| t.id == parking.template_id)
                .ok_or("parking template missing")?
                .clone();
            sim_core::vehicle::validate_template(&template)?;
            let id = EntityId(vehicles.len() as u32 + 1);
            let motor =
                sim_core::spawn_vehicle(parking.position.map(|v| v as f32), parking.yaw as f32)?;
            if collision.overlaps_vehicle(
                motor.position.0.map(|v| v.0),
                motor.yaw.0,
                template.dimensions.map(|v| v as f32),
            )? {
                return Err("car spawn obstructed".into());
            }
            vehicles.insert(
                id.0,
                crate::vehicles::Vehicle {
                    id,
                    state: VehicleState {
                        motor,
                        health: VehicleHealth(template.health),
                        driver_id: None,
                        disabled: false,
                    },
                    template,
                    idle_ticks: 30,
                },
            );
            collision
                .set_vehicle_proxies(&vehicles.values().map(|v| v.proxy()).collect::<Vec<_>>())?;
        }
        let next_entity = vehicles.len() as u32 + 1;
        Ok(Self {
            tick: 0,
            id: random_id("world-", 16)?,
            sessions: BTreeMap::new(),
            players: BTreeMap::new(),
            metrics: Metrics::default(),
            content,
            config,
            collision,
            next_entity,
            vehicles,
            output: VecDeque::new(),
            closing: BTreeMap::new(),
        })
    }
    pub fn build(&self) -> ServerHello {
        ServerHello {
            protocol_version: ProtocolVersion(1),
            build_id: BuildId(BUILD_ID.into()),
            content_hash: ContentHash(self.content.manifest.content_hash.clone()),
            sim_config_hash: ContentHash(self.content.manifest.sim_config_hash.clone()),
            tick_rate: TickRate(30),
            snapshot_rate: SnapshotRate(10),
        }
    }
    fn send(
        &mut self,
        peer: PeerKey,
        sid: Option<&str>,
        delivery: Delivery,
        message: ServerMessage,
    ) {
        self.output.push_back((
            peer,
            delivery,
            ServerEnvelope {
                v: ProtocolVersion(1),
                world_id: Some(OpaqueId(self.id.clone())),
                session_id: sid.map(|s| OpaqueId(s.into())),
                message,
            },
        ));
    }
    fn reliable(&mut self, peer: PeerKey, sid: Option<&str>, message: ServerMessage) {
        self.send(peer, sid, Delivery::Reliable, message);
    }
    fn cleanup(&mut self, session: &Session, profiles: &mut MemoryProfiles) {
        if let Some(id) = session.player {
            for car in self
                .vehicles
                .values_mut()
                .filter(|car| car.state.driver_id == Some(id))
            {
                car.state.driver_id = None;
                car.stop();
            }
            self.players.remove(&id.0);
        }
        if let (Some(profile), Some(id)) = (&session.profile, &session.id) {
            profiles.deactivate(&profile.profile_id.0, id);
        }
    }
    pub fn disconnect(&mut self, peer: PeerKey, profiles: &mut MemoryProfiles) {
        if let Some(session) = self.sessions.remove(&peer) {
            self.cleanup(&session, profiles);
        }
    }
    fn owner(&self, session: &Session) -> Result<OwnerState, String> {
        let id = session.player.ok_or("missing owner")?;
        let player = self.players.get(&id.0).ok_or("missing player")?;
        Ok(OwnerState {
            server_tick: self.tick,
            consumed_through_tick: self.tick,
            control_epoch: ControlEpoch(session.epoch),
            player_id: id,
            controlled_entity_id: player.state.vehicle_id.unwrap_or(id),
            state: player
                .state
                .vehicle_id
                .map(|car| EntityState::Vehicle(self.vehicles[&car.0].state.clone()))
                .unwrap_or_else(|| EntityState::Character(player.state.clone())),
            player_state: player.state.clone(),
        })
    }
    fn entities(&self) -> BTreeMap<u32, (EntityDescriptor, EntityState)> {
        self.players
            .iter()
            .map(|(&id, p)| {
                (
                    id,
                    (
                        EntityDescriptor {
                            id: p.id,
                            entity_kind: EntityKind::Player,
                            template_id: TemplateId("player_greybox".into()),
                            display_name: Some(p.display_name.clone()),
                        },
                        EntityState::Character(p.state.clone()),
                    ),
                )
            })
            .chain(self.vehicles.iter().map(|(&id, v)| {
                (
                    id,
                    (
                        EntityDescriptor {
                            id: v.id,
                            entity_kind: EntityKind::Vehicle,
                            template_id: TemplateId(v.template.id.clone()),
                            display_name: None,
                        },
                        EntityState::Vehicle(v.state.clone()),
                    ),
                )
            }))
            .collect()
    }
    fn show(&self, id: u32, visibility: u32, baseline: Option<u32>) -> EntityShow {
        let (descriptor, state) = self.entities().remove(&id).expect("visible entity exists");
        EntityShow {
            baseline_id: baseline.map(BaselineId),
            visibility_id: VisibilityId(visibility),
            descriptor,
            state,
            state_tick: self.tick,
        }
    }
    fn controlled(&self, s: &Session) -> u32 {
        let player = s.player.expect("authenticated player");
        self.players[&player.0].state.vehicle_id.unwrap_or(player).0
    }
    fn transition(&mut self, peer: PeerKey, s: &mut Session, ids: &[u32]) -> Result<(), String> {
        s.epoch = s.epoch.checked_add(1).ok_or("epoch exhausted")?;
        s.inputs.clear();
        // Refresh both replicas reliably with a fresh visibility generation. Older RTC
        // packets cannot restore a previous occupancy mode, even at the same tick.
        let entities = self.entities();
        let mut messages = Vec::new();
        for (target, session) in
            std::iter::once((peer, s)).chain(self.sessions.iter_mut().map(|(p, s)| (*p, s)))
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
                        state_tick: self.tick,
                    }),
                ));
            }
        }
        for (target, sid, message) in messages {
            self.reliable(target, sid.as_deref(), message);
        }
        Ok(())
    }
    fn action(&mut self, peer: PeerKey, s: &mut Session, action: &Action) -> ActionCode {
        let Some(id) = s.player else {
            return ActionCode::NotReady;
        };
        if s.epoch == u32::MAX
            || s.visibility_counter > u32::MAX - 2
            || self
                .sessions
                .values()
                .any(|other| other.visibility_counter > u32::MAX - 2)
        {
            return ActionCode::Unavailable;
        }
        let player = &self.players[&id.0];
        if player.state.health.0 == 0 {
            return ActionCode::Dead;
        }
        let vehicle_id;
        match action {
            Action::EnterVehicle(request) => {
                if player.state.mode != CharacterMode::OnFoot {
                    return ActionCode::Unavailable;
                }
                vehicle_id = request.vehicle_id.0;
                let Some(car) = self.vehicles.get(&vehicle_id) else {
                    return ActionCode::InvalidTarget;
                };
                if !s.visible.contains_key(&vehicle_id) {
                    return ActionCode::OutOfRange;
                }
                if car.state.driver_id.is_some() {
                    return ActionCode::Occupied;
                }
                if car.state.disabled || car.state.health.0 == 0 {
                    return ActionCode::Unavailable;
                }
                if car.state.motor.speed.0.abs() >= 0.5 {
                    return ActionCode::TooFast;
                }
                let feet = player.state.motor.position.0.map(|v| v.0);
                let entry = car.entry();
                if feet
                    .iter()
                    .zip(entry)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f32>()
                    .sqrt()
                    > 2.0
                {
                    return ActionCode::OutOfRange;
                }
                self.collision.exclude_vehicle(Some(vehicle_id));
                let clear = crate::vehicles::clear_segment(
                    &self.collision,
                    [feet[0], feet[1] + 1.45, feet[2]],
                    [entry[0], entry[1] + 0.9, entry[2]],
                );
                self.collision.exclude_vehicle(None);
                if !clear {
                    return ActionCode::OutOfRange;
                }
                let car = self
                    .vehicles
                    .get_mut(&vehicle_id)
                    .expect("validated vehicle");
                car.state.driver_id = Some(id);
                car.idle_ticks = 0;
                let player = self.players.get_mut(&id.0).expect("validated player");
                player.state.mode = CharacterMode::Driving;
                player.state.vehicle_id = Some(EntityId(vehicle_id));
                player.state.motor = sim_core::spawn_character(car.seat(), car.state.motor.yaw.0)
                    .expect("validated seat");
            }
            Action::ExitVehicle(_) => {
                let Some(car_id) = player.state.vehicle_id else {
                    return ActionCode::InvalidTarget;
                };
                vehicle_id = car_id.0;
                let car = &self.vehicles[&vehicle_id];
                if car.state.driver_id != Some(id) {
                    return ActionCode::InvalidTarget;
                }
                if car.state.motor.speed.0.abs() >= 2.0 {
                    return ActionCode::TooFast;
                }
                self.collision.exclude_vehicle(Some(vehicle_id));
                let exit = crate::vehicles::safe_exit(car, &self.collision);
                self.collision.exclude_vehicle(None);
                let Some(exit) = exit else {
                    return ActionCode::ExitBlocked;
                };
                let yaw = car.state.motor.yaw.0;
                let car = self
                    .vehicles
                    .get_mut(&vehicle_id)
                    .expect("validated vehicle");
                car.state.driver_id = None;
                car.stop();
                let player = self.players.get_mut(&id.0).expect("validated player");
                player.state.mode = CharacterMode::OnFoot;
                player.state.vehicle_id = None;
                player.state.motor = sim_core::spawn_character(exit, yaw).expect("validated exit");
            }
            _ => return ActionCode::Unavailable,
        }
        if self.transition(peer, s, &[id.0, vehicle_id]).is_err() {
            return ActionCode::Unavailable;
        }
        if let Ok(owner) = self.owner(s) {
            self.reliable(peer, s.id.as_deref(), ServerMessage::ControlChanged(owner));
        }
        ActionCode::Ok
    }
    fn baseline(&mut self, peer: PeerKey, s: &mut Session, now: u64) -> Result<(), String> {
        s.baseline = s.baseline.checked_add(1).ok_or("baseline exhausted")?;
        s.phase = SessionPhase::Loading;
        s.inputs.clear();
        s.visible.clear();
        s.deadline = now.saturating_add(30_000);
        self.reliable(
            peer,
            s.id.as_deref(),
            ServerMessage::BaselineBegin(BaselineBegin {
                baseline_id: BaselineId(s.baseline),
                server_tick: self.tick,
            }),
        );
        s.last_replicated.clear();
        let entities = self.entities();
        let spatial = SpatialIndex::new(entities.iter().map(|(&id, (_, state))| {
            let p = entity_position(state);
            (id, [p[0], p[2]])
        }));
        let owned = self.controlled(s);
        let origin = entity_position(&entities[&owned].1);
        let mut visible = spatial.relevant([origin[0], origin[2]], owned, &s.visible);
        visible.insert(s.player.ok_or("missing player")?.0);
        for id in visible {
            s.visibility_counter = s
                .visibility_counter
                .checked_add(1)
                .ok_or("visibility exhausted")?;
            s.visible.insert(id, s.visibility_counter);
            self.reliable(
                peer,
                s.id.as_deref(),
                ServerMessage::EntityShow(self.show(id, s.visibility_counter, Some(s.baseline))),
            );
        }
        self.reliable(
            peer,
            s.id.as_deref(),
            ServerMessage::BaselineEnd(BaselineEnd {
                baseline_id: BaselineId(s.baseline),
                entity_count: EntityCount(s.visible.len() as u16),
            }),
        );
        Ok(())
    }
    fn message(
        &mut self,
        peer: PeerKey,
        s: &mut Session,
        message: ClientEnvelope,
        profiles: &mut MemoryProfiles,
        now: u64,
        allow_joins: bool,
    ) -> Result<(), &'static str> {
        if protocol::validate::validate_client(&message).is_err()
            || message.world_id.as_ref().map(|v| v.0.as_str()) != Some(self.id.as_str())
            || message.session_id.as_ref().map(|v| v.0.as_str()) != s.id.as_deref()
        {
            return Err("invalid session scope");
        }
        let bytes = encode_client(&message)
            .map_err(|_| "invalid input encoding")?
            .len();
        if !s.ingress_budget.allow(bytes, now, 0) {
            return Err("ingress byte budget exceeded");
        }
        self.metrics.ingress_bytes += bytes as u64;
        s.last_heard = now;
        match message.message {
            ClientMessage::ClientHello(hello) if s.phase == SessionPhase::AwaitHello => {
                let build = self.build();
                let reject = if hello.build_id != build.build_id {
                    Some(RejectCode::VersionMismatch)
                } else if hello.content_hash != build.content_hash
                    || hello.sim_config_hash != build.sim_config_hash
                {
                    Some(RejectCode::ContentMismatch)
                } else if !allow_joins {
                    Some(RejectCode::Unavailable)
                } else {
                    None
                };
                if let Some(code) = reject {
                    self.reliable(
                        peer,
                        None,
                        ServerMessage::Reject(Reject {
                            code,
                            detail: DetailText("local build unavailable or mismatched".into()),
                        }),
                    );
                    return Err("join rejected");
                }
                let Some(profile) = profiles.consume(&hello.join_ticket.0, now, hello.display_name)
                else {
                    self.reliable(
                        peer,
                        None,
                        ServerMessage::Reject(Reject {
                            code: RejectCode::InvalidTicket,
                            detail: DetailText("ticket invalid, expired or consumed".into()),
                        }),
                    );
                    return Err("invalid ticket");
                };
                // Replace a previous session only after the new one proved its one-use ticket.
                let old = self
                    .sessions
                    .iter()
                    .find(|(_, old)| {
                        old.profile
                            .as_ref()
                            .is_some_and(|p| p.profile_id == profile.profile_id)
                    })
                    .map(|(peer, _)| *peer);
                if let Some(old) = old {
                    let old_session_id = self.sessions.get(&old).and_then(|s| s.id.clone());
                    self.disconnect(old, profiles);
                    self.output.push_back((
                        old,
                        Delivery::Reliable,
                        ServerEnvelope {
                            v: ProtocolVersion(1),
                            world_id: Some(OpaqueId(self.id.clone())),
                            session_id: old_session_id.map(OpaqueId),
                            message: ServerMessage::Disconnect(Disconnect {
                                code: TemplateId("replaced".into()),
                                detail: DetailText("guest joined again".into()),
                            }),
                        },
                    ));
                }
                if self.players.len() >= MAX_PLAYERS {
                    self.reliable(
                        peer,
                        None,
                        ServerMessage::Reject(Reject {
                            code: RejectCode::ServerFull,
                            detail: DetailText("shard full".into()),
                        }),
                    );
                    return Err("shard full");
                }
                let slot = (0..MAX_PLAYERS)
                    .find(|slot| self.players.values().all(|p| p.slot != *slot))
                    .ok_or("spawn slots full")?;
                let id = EntityId(self.next_entity);
                self.next_entity = self
                    .next_entity
                    .checked_add(1)
                    .ok_or("entity IDs exhausted")?;
                let position =
                    spawn_position(&self.content, slot).map_err(|_| "spawn unavailable")?;
                let motor =
                    sim_core::spawn_character(position, self.content.manifest.spawns[0].yaw as f32)
                        .map_err(|_| "spawn unavailable")?;
                self.players.insert(
                    id.0,
                    Player {
                        id,
                        display_name: profile.display_name.clone(),
                        slot,
                        state: CharacterState {
                            motor,
                            health: CharacterHealth(100),
                            mode: CharacterMode::OnFoot,
                            vehicle_id: None,
                        },
                    },
                );
                s.player = Some(id);
                s.profile = Some(profile.clone());
                s.id = Some(random_id("session-", 16).map_err(|_| "randomness failed")?);
                self.reliable(
                    peer,
                    s.id.as_deref(),
                    ServerMessage::Welcome(Welcome {
                        player_id: id,
                        server_tick: self.tick,
                        input_lead_ticks: InputLead(3),
                        profile,
                    }),
                );
                self.baseline(peer, s, now)
                    .map_err(|_| "baseline unavailable")?;
            }
            ClientMessage::ClientReady(ready) if s.phase == SessionPhase::Loading => {
                if ready.baseline_id.0 != s.baseline {
                    return Ok(());
                }
                s.phase = SessionPhase::Playing;
                profiles.activate(
                    &s.profile.as_ref().ok_or("profile missing")?.profile_id.0,
                    s.id.as_deref().ok_or("session missing")?,
                );
                self.reliable(
                    peer,
                    s.id.as_deref(),
                    ServerMessage::StartPlay(StartPlay {
                        baseline_id: ready.baseline_id,
                        owner: self.owner(s).map_err(|_| "owner missing")?,
                    }),
                );
                let p = s.profile.as_ref().ok_or("profile missing")?;
                self.reliable(
                    peer,
                    s.id.as_deref(),
                    ServerMessage::PlayerStatus(PlayerStatus {
                        revision: Revision(1),
                        server_tick: self.tick,
                        credits: p.credits,
                        completed_jobs: p.completed_jobs,
                        heat: Heat(0),
                        pursuit: Pursuit::Clear,
                        magazine: Magazine(12),
                        reload_end_tick: None,
                        mission: None,
                        respawn_tick: None,
                        protected_until_tick: None,
                    }),
                );
            }
            ClientMessage::InputBatch(batch) => {
                if !s.input_rate.allow(now) {
                    return Err("input rate exceeded");
                }
                if s.phase != SessionPhase::Playing {
                    return Ok(());
                }
                if batch.control_epoch.0 != s.epoch {
                    self.metrics.stale_epochs += 1;
                    return Ok(());
                }
                let next = self.tick + 1;
                for frame in batch.frames {
                    if frame.tick < next {
                        self.metrics.late_inputs += 1;
                        continue;
                    }
                    if frame.tick > next.saturating_add(8) || frame.tick == u32::MAX {
                        self.metrics.future_inputs += 1;
                        continue;
                    }
                    if let Some(previous) = s.inputs.get(&frame.tick) {
                        if *previous != frame {
                            self.metrics.conflicting_inputs += 1;
                        }
                        continue;
                    }
                    s.inputs.insert(frame.tick, frame);
                }
            }
            ClientMessage::Ping(ping) if s.phase != SessionPhase::AwaitHello => {
                // The crate lacks ingress delivery metadata; this existing nonce convention
                // selects only the requested response channel, never asserts ingress type.
                let channel = if ping.nonce & 0x8000_0000 != 0 {
                    Delivery::Unreliable
                } else {
                    Delivery::Reliable
                };
                self.send(
                    peer,
                    s.id.as_deref(),
                    channel,
                    ServerMessage::Pong(Pong {
                        nonce: ping.nonce,
                        client_time_ms: ping.client_time_ms,
                        server_tick: self.tick,
                        server_time_ms: Milliseconds(now as f64),
                    }),
                );
            }
            ClientMessage::ResyncRequest(_) if s.phase == SessionPhase::Playing => {
                if s.last_resync
                    .is_some_and(|last| now.saturating_sub(last) < 5000)
                {
                    return Ok(());
                }
                s.last_resync = Some(now);
                s.epoch = s.epoch.checked_add(1).ok_or("epoch exhausted")?;
                if let Some(p) = s.player.and_then(|id| self.players.get_mut(&id.0)) {
                    p.state.motor.velocity = Vec3([Scalar(0.0); 3]);
                    if let Some(car) = p.state.vehicle_id {
                        self.vehicles.get_mut(&car.0).ok_or("missing car")?.stop();
                    }
                }
                self.baseline(peer, s, now)
                    .map_err(|_| "baseline unavailable")?;
            }
            ClientMessage::Action(request) => {
                if !s.action_rate.allow(1, now, 0) {
                    return Err("action rate exceeded");
                }
                let result = if let Some(cached) = s.actions.get(&request.request_id.0) {
                    cached.clone()
                } else {
                    let code = if s.phase != SessionPhase::Playing {
                        ActionCode::NotReady
                    } else if request.control_epoch.0 != s.epoch {
                        ActionCode::StaleEpoch
                    } else if request.request_id.0 <= s.last_request {
                        ActionCode::StaleRequest
                    } else {
                        self.action(peer, s, &request.action)
                    };
                    let result = ActionResult {
                        request_id: request.request_id,
                        applied_tick: self.tick,
                        ok: code == ActionCode::Ok,
                        code,
                        affected_entity_id: if code == ActionCode::Ok {
                            Some(EntityId(self.controlled(s)))
                        } else {
                            None
                        },
                    };
                    if request.request_id.0 > s.last_request {
                        s.last_request = request.request_id.0;
                        s.actions.insert(request.request_id.0, result.clone());
                        while s.actions.len() > 32 {
                            s.actions.pop_first();
                        }
                    }
                    result
                };
                self.reliable(peer, s.id.as_deref(), ServerMessage::ActionResult(result));
            }
            ClientMessage::Leave(_) => return Err("left"),
            ClientMessage::ClientReady(_) => {} // A delayed duplicate readiness cannot start twice.
            _ => return Err("invalid phase"),
        }
        Ok(())
    }
    fn sync_visible(&mut self) -> Result<(), String> {
        let entities = self.entities();
        let spatial = SpatialIndex::new(entities.iter().map(|(&id, (_, state))| {
            let p = entity_position(state);
            (id, [p[0], p[2]])
        }));
        let mut messages = Vec::new();
        for (&peer, s) in &mut self.sessions {
            if s.phase == SessionPhase::AwaitHello {
                continue;
            }
            let player = s.player.ok_or("missing observer")?.0;
            let owner = self.players[&player]
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
                            server_tick: self.tick,
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
                            state_tick: self.tick,
                        }),
                    ));
                }
            }
        }
        for (peer, sid, message) in messages {
            self.reliable(peer, sid.as_deref(), message);
        }
        Ok(())
    }
    pub fn step<T: GameTransport>(
        &mut self,
        transport: &mut T,
        profiles: &mut MemoryProfiles,
        now: u64,
        allow_joins: bool,
    ) -> Result<(), String> {
        if self.tick >= u32::MAX - 1 {
            return Err("world tick exhausted; restart required".into());
        }
        let events = transport.drain(128);
        let closed: BTreeSet<_> = events
            .iter()
            .filter_map(|e| {
                if let TransportEvent::Closed { peer, .. } = e {
                    Some(*peer)
                } else {
                    None
                }
            })
            .collect();
        for peer in &closed {
            self.disconnect(*peer, profiles);
        }
        let expired: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| {
                if s.phase == SessionPhase::Playing {
                    now.saturating_sub(s.last_heard) >= 10_000
                } else {
                    now >= s.deadline
                }
            })
            .map(|(p, _)| *p)
            .collect();
        for peer in expired {
            self.disconnect(peer, profiles);
            self.closing.insert(peer, "session timeout");
        }
        let mut messages = Vec::new();
        for event in events {
            match event {
                TransportEvent::Opened(peer)
                    if !closed.contains(&peer) && !self.sessions.contains_key(&peer) =>
                {
                    if self.sessions.len() >= MAX_PLAYERS {
                        self.closing.insert(peer, "connection capacity");
                        continue;
                    }
                    self.sessions.insert(
                        peer,
                        Session {
                            id: None,
                            profile: None,
                            player: None,
                            phase: SessionPhase::AwaitHello,
                            epoch: 1,
                            baseline: 0,
                            visibility_counter: 0,
                            visible: BTreeMap::new(),
                            inputs: BTreeMap::new(),
                            actions: BTreeMap::new(),
                            last_request: 0,
                            last_heard: now,
                            deadline: now.saturating_add(5000),
                            last_resync: None,
                            ingress_budget: Budget::new(30 * 1024, 60 * 1024),
                            egress_budget: Budget::new(60 * 1024, 120 * 1024),
                            input_rate: InputRate::default(),
                            action_rate: Budget::new(10, 20),
                            last_replicated: BTreeMap::new(),
                        },
                    );
                    self.reliable(peer, None, ServerMessage::ServerHello(self.build()));
                }
                TransportEvent::Message { peer, message } if !closed.contains(&peer) => {
                    messages.push((peer, message))
                }
                _ => {}
            }
        }
        // Stable ordering within peers preserves FIFO, peer keys decide admission order.
        messages.sort_by_key(|(peer, _)| *peer);
        for (peer, message) in messages {
            let Some(mut s) = self.sessions.remove(&peer) else {
                continue;
            };
            match self.message(peer, &mut s, message, profiles, now, allow_joins) {
                Ok(()) => {
                    self.sessions.insert(peer, s);
                }
                Err(reason) => {
                    self.cleanup(&s, profiles);
                    self.closing.insert(peer, reason);
                }
            }
        }
        // Replaced guests have already been removed. Kill the old transport as well.
        for (peer, _, message) in &self.output {
            if matches!(message.message, ServerMessage::Disconnect(_)) {
                self.closing.insert(*peer, "guest replaced");
            }
        }
        let next = self.tick + 1;
        let mut failed = Vec::new();
        // Entity order, not network arrival order, determines simulation iteration.
        let owners: BTreeMap<_, _> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.phase == SessionPhase::Playing)
            .filter_map(|(peer, s)| s.player.map(|id| (id.0, *peer)))
            .collect();
        let mut actor_inputs = BTreeMap::new();
        for (&id, &peer) in &owners {
            let s = self.sessions.get_mut(&peer).ok_or("missing session")?;
            let player = &self.players[&id];
            let input = s
                .inputs
                .remove(&next)
                .unwrap_or_else(|| sim_core::neutral_input(next, player.state.motor.yaw));
            s.inputs.retain(|tick, _| *tick > next);
            actor_inputs.insert(player.state.vehicle_id.map_or(id, |v| v.0), input);
        }
        self.collision.set_vehicle_proxies(
            &self
                .vehicles
                .values()
                .map(|v| v.proxy())
                .collect::<Vec<_>>(),
        )?;
        for car_id in self.vehicles.keys().copied().collect::<Vec<_>>() {
            let car = self.vehicles.get_mut(&car_id).ok_or("missing vehicle")?;
            self.collision.exclude_vehicle(Some(car.id.0));
            let input = actor_inputs
                .get(&car.id.0)
                .cloned()
                .unwrap_or_else(|| sim_core::neutral_input(next, car.state.motor.yaw));
            match sim_core::step_vehicle(
                &car.state.motor,
                &input,
                &car.template,
                &StepContext {
                    tick: next,
                    collision: &self.collision,
                    config: &self.config,
                },
            ) {
                Ok(result) => {
                    car.state.motor = result.motor;
                    car.idle_ticks = if car.state.motor.speed.0.abs() < 0.1 {
                        car.idle_ticks.saturating_add(1)
                    } else {
                        0
                    };
                }
                Err(_) => {
                    car.stop();
                    if let Some(driver) = car.state.driver_id
                        && let Some(peer) = owners.get(&driver.0)
                    {
                        failed.push(*peer);
                    }
                }
            }
            // Later entity IDs observe earlier cars' committed positions in this tick.
            self.collision.set_vehicle_proxies(
                &self
                    .vehicles
                    .values()
                    .map(|v| v.proxy())
                    .collect::<Vec<_>>(),
            )?;
        }
        self.collision.exclude_vehicle(None);
        for (id, peer) in owners {
            let player = self.players.get_mut(&id).ok_or("player missing")?;
            if let Some(car_id) = player.state.vehicle_id {
                let car = &self.vehicles[&car_id.0];
                player.state.motor = sim_core::presentation_motor(&car.state.motor);
                player.state.motor.position = Vec3(car.seat().map(Scalar));
            } else if let Some(input) = actor_inputs.get(&id) {
                match sim_core::step_character(
                    &player.state.motor,
                    input,
                    &StepContext {
                        tick: next,
                        collision: &self.collision,
                        config: &self.config,
                    },
                ) {
                    Ok(result) => player.state.motor = result.motor,
                    Err(_) => failed.push(peer),
                }
            }
        }
        self.tick = next;
        for peer in failed {
            self.disconnect(peer, profiles);
            self.closing.insert(peer, "unsafe motor input/state");
        }
        self.sync_visible()?;
        if self.tick.is_multiple_of(3) {
            let updates: Result<Vec<_>, String> = self
                .sessions
                .iter()
                .filter(|(_, s)| s.phase == SessionPhase::Playing)
                .map(|(peer, s)| Ok((*peer, s.id.clone(), self.owner(s)?)))
                .collect();
            for (peer, sid, owner) in updates? {
                self.send(
                    peer,
                    sid.as_deref(),
                    Delivery::Unreliable,
                    ServerMessage::OwnerState(owner),
                );
            }
        }
        if self.tick.is_multiple_of(3) {
            let mut batches = Vec::new();
            let entities = self.entities();
            for (&peer, session) in &mut self.sessions {
                if session.phase != SessionPhase::Playing {
                    continue;
                }
                let player = session.player.ok_or("missing owner")?.0;
                let owned = self.players[&player]
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
                        self.vehicles.get(id).is_none_or(|v| {
                            v.idle_ticks < 30
                                || v.state.driver_id.is_some()
                                || self.tick.is_multiple_of(15)
                        })
                    })
                    .collect();
                ids.sort_by(|a, b| {
                    let age_a = self
                        .tick
                        .saturating_sub(*session.last_replicated.get(a).unwrap_or(&self.tick));
                    let age_b = self
                        .tick
                        .saturating_sub(*session.last_replicated.get(b).unwrap_or(&self.tick));
                    let dist = |id: &u32| {
                        let p = entity_position(&entities[id].1);
                        (p[0] - position[0]).hypot(p[2] - position[2])
                    };
                    (age_b >= 9)
                        .cmp(&(age_a >= 9))
                        .then(dist(a).total_cmp(&dist(b)))
                        .then(a.cmp(b))
                });
                let records = ids.into_iter().map(|id| StateRecord {
                    entity_id: EntityId(id),
                    visibility_id: VisibilityId(session.visible[&id]),
                    state: entities[&id].1.clone(),
                });
                for envelope in packets(
                    &self.id,
                    session.id.as_deref().ok_or("missing session")?,
                    self.tick,
                    records,
                )? {
                    batches.push((peer, Delivery::Unreliable, envelope));
                }
                if self.tick.is_multiple_of(30) {
                    let p = session.profile.as_ref().ok_or("missing profile")?;
                    batches.push((
                        peer,
                        Delivery::Reliable,
                        ServerEnvelope {
                            v: ProtocolVersion(1),
                            world_id: Some(OpaqueId(self.id.clone())),
                            session_id: session.id.clone().map(OpaqueId),
                            message: ServerMessage::PlayerStatus(PlayerStatus {
                                revision: Revision(1),
                                server_tick: self.tick,
                                credits: p.credits,
                                completed_jobs: p.completed_jobs,
                                heat: Heat(0),
                                pursuit: Pursuit::Clear,
                                magazine: Magazine(12),
                                reload_end_tick: None,
                                mission: None,
                                respawn_tick: None,
                                protected_until_tick: None,
                            }),
                        },
                    ));
                }
            }
            self.output.extend(batches);
        }
        let mut flushed = 0;
        while let Some((peer, delivery, message)) = self.output.pop_front() {
            flushed += 1;
            if flushed > 4096 {
                return Err("tick output budget exhausted".into());
            }
            let bytes = encode_server(&message, delivery)
                .map_err(|e| e.to_string())?
                .len();
            let is_snapshot = matches!(message.message, ServerMessage::Snapshot(_));
            if let Some(session) = self.sessions.get_mut(&peer) {
                if !session
                    .egress_budget
                    .allow(bytes, now, if is_snapshot { 1100 } else { 0 })
                {
                    self.metrics.budget_drops += 1;
                    if delivery == Delivery::Reliable {
                        self.disconnect(peer, profiles);
                        self.closing.insert(peer, "reliable byte budget exhausted");
                    }
                    continue;
                }
                if let ServerMessage::Snapshot(data) = &message.message {
                    self.metrics.snapshot_packets += 1;
                    self.metrics.max_snapshot_bytes = self.metrics.max_snapshot_bytes.max(bytes);
                    for record in &data.records {
                        let previous = session
                            .last_replicated
                            .insert(record.entity_id.0, self.tick)
                            .unwrap_or(self.tick);
                        self.metrics.max_refresh_age_ticks =
                            self.metrics.max_refresh_age_ticks.max(self.tick - previous);
                    }
                }
            }
            self.metrics.egress_bytes += bytes as u64;
            if matches!(
                transport.send(peer, delivery, message),
                SendOutcome::Full | SendOutcome::Closed
            ) {
                self.disconnect(peer, profiles);
                self.closing.insert(peer, "output unavailable");
                self.sync_visible()?;
            }
        }
        for (peer, reason) in std::mem::take(&mut self.closing) {
            transport.disconnect(peer, reason);
        }
        Ok(())
    }
}
fn spawn_position(content: &WorldContent, slot: usize) -> Result<[f32; 3], String> {
    let origin = content
        .manifest
        .spawns
        .first()
        .ok_or("missing spawn")?
        .position;
    Ok([
        origin[0] as f32 + ((slot % 4) as f32 - 1.5) * 2.0,
        origin[1] as f32,
        origin[2] as f32 + ((slot / 4) as f32 - 1.5) * 2.0,
    ])
}
#[cfg(test)]
pub(crate) fn test_envelope(
    world: &str,
    sid: Option<&str>,
    kind: &str,
    data: serde_json::Value,
) -> ServerEnvelope {
    serde_json::from_value(serde_json::json!({"v":1,"world_id":world,"session_id":sid,"message":{"kind":kind,"data":data}})).unwrap()
}

fn entity_position(state: &EntityState) -> [f32; 3] {
    match state {
        EntityState::Character(v) => v.motor.position.0.map(|v| v.0),
        EntityState::Vehicle(v) => v.motor.position.0.map(|v| v.0),
    }
}
