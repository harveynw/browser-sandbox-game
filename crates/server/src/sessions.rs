//! Connection lifecycle, admission and validation of client intent.
use crate::admission::{Profiles, random_id};
use crate::entities::player::Player;
use crate::replication::{Budget, InputRate};
use crate::transport::*;
use crate::world::{MAX_PLAYERS, spawn_position};
use crate::{replication::Replication, systems::Systems, world::World};
use protocol::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::result::Result;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPhase {
    AwaitHello,
    Loading,
    Playing,
}
#[derive(Default, serde::Serialize)]
pub struct TrafficCounters {
    pub up_bytes: u64,
    pub down_bytes: u64,
}
pub struct Session {
    pub traffic: TrafficCounters,
    pub id: Option<String>,
    pub profile: Option<ProfileView>,
    pub player: Option<EntityId>,
    pub(crate) pending_admission: Option<String>,
    pub phase: SessionPhase,
    pub epoch: u32,
    pub baseline: u32,
    pub(crate) visibility_counter: u32,
    pub(crate) visible: BTreeMap<u32, u32>,
    pub inputs: BTreeMap<u32, InputFrame>,
    pub(crate) actions: BTreeMap<u32, ActionResult>,
    pub(crate) last_request: u32,
    pub repair_view: Option<RepairView>,
    pub(crate) repair_clear: Option<u32>,
    pub(crate) status_revision: u32,
    pub(crate) status_dirty: bool,
    pub(crate) last_heard: u64,
    pub(crate) deadline: u64,
    pub(crate) last_resync: Option<u64>,
    pub(crate) ingress_budget: Budget,
    pub(crate) egress_budget: Budget,
    pub(crate) input_rate: InputRate,
    pub(crate) action_rate: Budget,
    pub(crate) last_replicated: BTreeMap<u32, u32>,
}

#[derive(Default)]
pub struct Sessions {
    pub peers: BTreeMap<PeerKey, Session>,
    pub(crate) output: VecDeque<(PeerKey, Delivery, ServerEnvelope)>,
    pub(crate) closing: BTreeMap<PeerKey, &'static str>,
}
impl Sessions {
    pub(crate) fn cleanup(
        session: &Session,
        profiles: &mut Profiles,
        world: &mut World,
        systems: &mut Systems,
    ) {
        if let Some(id) = session.player {
            systems.missions.release_disconnected_recoveries(
                id.0,
                world,
                &mut systems.population,
                &mut systems.police,
                &mut systems.combat,
            );
            for car in world
                .vehicles
                .values_mut()
                .filter(|car| car.state.driver_id == Some(id))
            {
                car.state.driver_id = None;
                car.stop();
            }
            systems.police.release(id.0);
            world.players.remove(&id.0);
            systems.police.wanted.remove(&id.0);
            systems.missions.active.remove(&id.0);
        }
        if let (Some(profile), Some(id)) = (&session.profile, &session.id) {
            profiles.deactivate(&profile.profile_id.0, id);
        }
    }
    pub(crate) fn disconnect(
        &mut self,
        peer: PeerKey,
        profiles: &mut Profiles,
        world: &mut World,
        systems: &mut Systems,
    ) {
        if let Some(session) = self.peers.remove(&peer) {
            Sessions::cleanup(&session, profiles, world, systems);
        }
    }
    pub(crate) fn action(
        &mut self,
        peer: PeerKey,
        s: &mut Session,
        action: &Action,
        request_id: u32,
        world: &mut World,
        systems: &mut Systems,
    ) -> ActionCode {
        let Some(id) = s.player else {
            return ActionCode::NotReady;
        };
        if s.epoch == u32::MAX
            || s.visibility_counter > u32::MAX - 2
            || self
                .peers
                .values()
                .any(|other| other.visibility_counter > u32::MAX - 2)
        {
            return ActionCode::Unavailable;
        }
        let player = &world.players[&id.0];
        if systems.police.wanted(s.player_id()).arrest_pending
            || player.state.health.0 == 0
            || matches!(
                player.state.mode,
                CharacterMode::Downed | CharacterMode::Arrested
            )
        {
            return ActionCode::Dead;
        }
        if matches!(action, Action::Fire(_) | Action::Reload(_)) {
            return systems.combat.weapon_action(
                peer,
                s,
                action,
                world,
                self,
                &mut systems.population,
                &mut systems.police,
                &mut systems.missions,
            );
        }
        if let Action::Repair(request) = action {
            return systems.repairs.request_repair(
                s,
                request,
                request_id,
                world,
                &mut systems.police,
                &mut systems.missions,
            );
        }
        match action {
            Action::AcceptMission(request) => {
                return systems.missions.accept_job(
                    s,
                    &request.template_id.0,
                    world,
                    &mut systems.population,
                    &mut systems.police,
                    &mut systems.repairs,
                );
            }
            Action::CancelMission(_) => {
                let Some(m) = systems.missions.mission_mut(s.player_id()) else {
                    return ActionCode::NotActive;
                };
                if m.phase != crate::systems::missions::Phase::Active {
                    return ActionCode::NotActive;
                }
                m.fail(world.tick);
                s.status_dirty = true;
                return ActionCode::Ok;
            }
            _ => {}
        }
        let vehicle_id;
        match action {
            Action::EnterVehicle(request) => {
                if player.state.mode != CharacterMode::OnFoot {
                    return ActionCode::Unavailable;
                }
                vehicle_id = request.vehicle_id.0;
                let Some(car) = world.vehicles.get(&vehicle_id) else {
                    return ActionCode::InvalidTarget;
                };
                if !s.visible.contains_key(&vehicle_id) {
                    return ActionCode::OutOfRange;
                }
                if systems
                    .missions
                    .recoveries
                    .get(&vehicle_id)
                    .is_some_and(|r| r.owner != id.0)
                {
                    return ActionCode::Occupied;
                }
                if car.state.driver_id.is_some()
                    || systems.police.units.contains_key(&vehicle_id)
                    || !systems.population.can_enter(vehicle_id)
                {
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
                world.collision.exclude_vehicle(Some(vehicle_id));
                let clear = crate::entities::vehicle::clear_segment(
                    &world.collision,
                    [feet[0], feet[1] + 1.45, feet[2]],
                    [entry[0], entry[1] + 0.9, entry[2]],
                );
                world.collision.exclude_vehicle(None);
                if !clear {
                    return ActionCode::OutOfRange;
                }
                if systems.police.secured.contains(&vehicle_id) {
                    systems.police.report_crime(
                        id.0,
                        crate::systems::police::CrimeId::Theft(world.tick, vehicle_id),
                        crate::systems::police::CrimeKind::Theft,
                        feet,
                        world,
                    );
                }
                let car = world
                    .vehicles
                    .get_mut(&vehicle_id)
                    .expect("validated vehicle");
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .reload_end = None;
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .protected_until = None;
                s.status_dirty = true;
                car.state.driver_id = Some(id);
                car.idle_ticks = 0;
                let player = world.players.get_mut(&id.0).expect("validated player");
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
                let car = &world.vehicles[&vehicle_id];
                if car.state.driver_id != Some(id) {
                    return ActionCode::InvalidTarget;
                }
                if car.state.motor.speed.0.abs() >= 2.0 {
                    return ActionCode::TooFast;
                }
                world.collision.exclude_vehicle(Some(vehicle_id));
                let exit = crate::entities::vehicle::safe_exit(car, &world.collision);
                world.collision.exclude_vehicle(None);
                let Some(exit) = exit else {
                    return ActionCode::ExitBlocked;
                };
                let yaw = car.state.motor.yaw.0;
                let car = world
                    .vehicles
                    .get_mut(&vehicle_id)
                    .expect("validated vehicle");
                car.state.driver_id = None;
                car.stop();
                let player = world.players.get_mut(&id.0).expect("validated player");
                player.state.mode = CharacterMode::OnFoot;
                player.state.vehicle_id = None;
                player.state.motor = sim_core::spawn_character(exit, yaw).expect("validated exit");
            }
            _ => return ActionCode::Unavailable,
        }
        if Replication::transition(
            peer,
            s,
            &[id.0, vehicle_id],
            world,
            self,
            &mut systems.population,
            &mut systems.police,
        )
        .is_err()
        {
            return ActionCode::Unavailable;
        }
        if let Ok(owner) = Replication::owner(s, world) {
            Replication::reliable(
                peer,
                s.id.as_deref(),
                ServerMessage::ControlChanged(owner),
                world,
                self,
            );
        }
        ActionCode::Ok
    }
    pub(crate) fn admit_profile(
        &mut self,
        peer: PeerKey,
        s: &mut Session,
        profile: ProfileView,
        profiles: &mut Profiles,
        now: u64,
        world: &mut World,
        systems: &mut Systems,
    ) -> Result<(), &'static str> {
        // Replace a previous session only after the new one proved its one-use ticket.
        let old = self
            .peers
            .iter()
            .find(|(_, old)| {
                old.profile
                    .as_ref()
                    .is_some_and(|p| p.profile_id == profile.profile_id)
            })
            .map(|(peer, _)| *peer);
        if let Some(old) = old {
            let old_session_id = self.peers.get(&old).and_then(|s| s.id.clone());
            self.disconnect(old, profiles, world, systems);
            self.output.push_back((
                old,
                Delivery::Reliable,
                ServerEnvelope {
                    v: ProtocolVersion(3),
                    world_id: Some(OpaqueId(world.id.clone())),
                    session_id: old_session_id.map(OpaqueId),
                    message: ServerMessage::Disconnect(Disconnect {
                        code: TemplateId("replaced".into()),
                        detail: DetailText("guest joined again".into()),
                    }),
                },
            ));
        }
        if world.players.len() >= MAX_PLAYERS {
            Replication::reliable(
                peer,
                None,
                ServerMessage::Reject(Reject {
                    code: RejectCode::ServerFull,
                    detail: DetailText("shard full".into()),
                }),
                world,
                self,
            );
            return Err("shard full");
        }
        let slot = (0..MAX_PLAYERS)
            .find(|slot| world.players.values().all(|p| p.slot != *slot))
            .ok_or("spawn slots full")?;
        let id = EntityId(world.next_entity);
        world.next_entity = world
            .next_entity
            .checked_add(1)
            .ok_or("entity IDs exhausted")?;
        let position = spawn_position(&world.content, slot).map_err(|_| "spawn unavailable")?;
        let motor =
            sim_core::spawn_character(position, world.content.manifest.spawns[0].yaw as f32)
                .map_err(|_| "spawn unavailable")?;
        world.players.insert(
            id.0,
            Player {
                tshirt_colour: profile.tshirt_colour,
                weapon: Default::default(),
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
        systems.police.wanted.insert(id.0, Default::default());
        systems.missions.active.insert(id.0, None);
        s.profile = Some(profile.clone());
        s.id = Some(random_id("session-", 16).map_err(|_| "randomness failed")?);
        Replication::reliable(
            peer,
            s.id.as_deref(),
            ServerMessage::Welcome(Welcome {
                player_id: id,
                server_tick: world.tick,
                input_lead_ticks: InputLead(3),
                profile,
            }),
            world,
            self,
        );
        Replication::baseline(
            peer,
            s,
            now,
            world,
            self,
            &mut systems.population,
            &mut systems.police,
        )
        .map_err(|_| "baseline unavailable")?;
        Ok(())
    }
    pub(crate) fn message(
        &mut self,
        peer: PeerKey,
        s: &mut Session,
        message: ClientEnvelope,
        profiles: &mut Profiles,
        now: u64,
        allow_joins: bool,
        world: &mut World,
        systems: &mut Systems,
    ) -> Result<(), &'static str> {
        if protocol::validate::validate_client(&message).is_err()
            || message.world_id.as_ref().map(|v| v.0.as_str()) != Some(world.id.as_str())
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
        world.metrics.ingress_bytes += bytes as u64;
        s.traffic.up_bytes += bytes as u64;
        s.last_heard = now;
        match message.message {
            ClientMessage::ClientHello(hello)
                if s.phase == SessionPhase::AwaitHello && s.pending_admission.is_none() =>
            {
                let build = world.build();
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
                    Replication::reliable(
                        peer,
                        None,
                        ServerMessage::Reject(Reject {
                            code,
                            detail: DetailText("local build unavailable or mismatched".into()),
                        }),
                        world,
                        self,
                    );
                    return Err("join rejected");
                }
                let admission = match profiles.begin_admission(
                    &hello.join_ticket.0,
                    now,
                    hello.display_name,
                    hello.tshirt_colour,
                ) {
                    Ok(admission) => admission,
                    Err(code) => {
                        Replication::reliable(
                            peer,
                            None,
                            ServerMessage::Reject(Reject {
                                code,
                                detail: DetailText(
                                    "ticket invalid or profile storage unavailable".into(),
                                ),
                            }),
                            world,
                            self,
                        );
                        return Err("ticket or profile unavailable");
                    }
                };
                match admission {
                    crate::admission::Admission::Ready(profile) => {
                        self.admit_profile(peer, s, profile, profiles, now, world, systems)?
                    }
                    crate::admission::Admission::Pending(id) => s.pending_admission = Some(id),
                }
            }
            ClientMessage::ClientReady(ready) if s.phase == SessionPhase::Loading => {
                if ready.baseline_id.0 != s.baseline {
                    return Ok(());
                }
                if s.baseline == 1 {
                    world
                        .players
                        .get_mut(&s.player_id())
                        .expect("player equipment")
                        .weapon
                        .protected_until = Some(world.tick.saturating_add(90));
                }
                s.phase = SessionPhase::Playing;
                profiles.activate(
                    &s.profile.as_ref().ok_or("profile missing")?.profile_id.0,
                    s.id.as_deref().ok_or("session missing")?,
                );
                Replication::reliable(
                    peer,
                    s.id.as_deref(),
                    ServerMessage::StartPlay(StartPlay {
                        baseline_id: ready.baseline_id,
                        owner: Replication::owner(s, world).map_err(|_| "owner missing")?,
                    }),
                    world,
                    self,
                );
                Replication::reliable(
                    peer,
                    s.id.as_deref(),
                    ServerMessage::PlayerStatus(
                        Replication::status(
                            world.tick,
                            s,
                            world,
                            &mut systems.police,
                            &mut systems.missions,
                        )
                        .map_err(|_| "status unavailable")?,
                    ),
                    world,
                    self,
                );
            }
            ClientMessage::InputBatch(batch) => {
                if !s.input_rate.allow(now) {
                    // RTC congestion can release more than a second of legitimate
                    // 30 Hz input at once. Keep the strict processing cap, discard
                    // excess stale work, and let fresh input recover the session.
                    world.metrics.rate_limited_inputs += 1;
                    return Ok(());
                }
                if s.phase != SessionPhase::Playing {
                    return Ok(());
                }
                if batch.control_epoch.0 != s.epoch {
                    world.metrics.stale_epochs += 1;
                    return Ok(());
                }
                let next = world.tick + 1;
                for frame in batch.frames {
                    if frame.tick < next {
                        world.metrics.late_inputs += 1;
                        continue;
                    }
                    if frame.tick > next.saturating_add(8) || frame.tick == u32::MAX {
                        world.metrics.future_inputs += 1;
                        continue;
                    }
                    if let Some(previous) = s.inputs.get(&frame.tick) {
                        if *previous != frame {
                            world.metrics.conflicting_inputs += 1;
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
                Replication::send(
                    peer,
                    s.id.as_deref(),
                    channel,
                    ServerMessage::Pong(Pong {
                        nonce: ping.nonce,
                        client_time_ms: ping.client_time_ms,
                        server_tick: world.tick,
                        server_time_ms: Milliseconds(now as f64),
                    }),
                    world,
                    self,
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
                if let Some(p) = s.player.and_then(|id| world.players.get_mut(&id.0)) {
                    p.state.motor.velocity = Vec3([Scalar(0.0); 3]);
                    if let Some(car) = p.state.vehicle_id {
                        world.vehicles.get_mut(&car.0).ok_or("missing car")?.stop();
                    }
                }
                Replication::baseline(
                    peer,
                    s,
                    now,
                    world,
                    self,
                    &mut systems.population,
                    &mut systems.police,
                )
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
                        if matches!(request.action, Action::AcceptMission(_) | Action::Repair(_))
                            && s.profile
                                .as_ref()
                                .is_some_and(|p| profiles.economic_pending(&p.profile_id.0))
                        {
                            ActionCode::Unavailable
                        } else {
                            self.action(
                                peer,
                                s,
                                &request.action,
                                request.request_id.0,
                                world,
                                systems,
                            )
                        }
                    };
                    let result = ActionResult {
                        request_id: request.request_id,
                        applied_tick: world.tick,
                        ok: code == ActionCode::Ok,
                        code,
                        affected_entity_id: if code == ActionCode::Ok {
                            Some(EntityId(Replication::controlled(s, world)))
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
                Replication::reliable(
                    peer,
                    s.id.as_deref(),
                    ServerMessage::ActionResult(result),
                    world,
                    self,
                );
            }
            ClientMessage::Leave(_) => return Err("left"),
            ClientMessage::ClientReady(_) => {} // A delayed duplicate readiness cannot start twice.
            _ => return Err("invalid phase"),
        }
        Ok(())
    }
    pub(crate) fn shutdown(
        &mut self,
        profiles: &mut Profiles,
        world: &mut World,
        systems: &mut Systems,
    ) -> bool {
        for peer in self.peers.keys().copied().collect::<Vec<_>>() {
            self.disconnect(peer, profiles, world, systems);
        }
        let until = std::time::Instant::now() + std::time::Duration::from_millis(3500);
        while !systems.repairs.pending.is_empty() && std::time::Instant::now() < until {
            world.tick = world.tick.saturating_add(1);
            let _ = systems
                .repairs
                .update(profiles, world, self, &mut systems.police);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        profiles.shutdown(std::time::Duration::from_secs(1)) && systems.repairs.pending.is_empty()
    }
    pub(crate) fn receive(
        &mut self,
        network: &mut Network,
        profiles: &mut Profiles,
        now: u64,
        allow_joins: bool,
        world: &mut World,
        systems: &mut Systems,
    ) -> Result<(), String> {
        profiles.poll_store(world.tick);
        let events = network.drain(128);
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
            self.disconnect(*peer, profiles, world, systems);
        }
        let expired: Vec<_> = self
            .peers
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
            self.disconnect(peer, profiles, world, systems);
            self.closing.insert(peer, "session timeout");
        }
        let mut messages = Vec::new();
        for event in events {
            match event {
                TransportEvent::Opened(peer)
                    if !closed.contains(&peer) && !self.peers.contains_key(&peer) =>
                {
                    if self.peers.len() >= MAX_PLAYERS {
                        self.closing.insert(peer, "connection capacity");
                        continue;
                    }
                    self.peers.insert(
                        peer,
                        Session {
                            traffic: Default::default(),
                            id: None,
                            profile: None,
                            player: None,
                            pending_admission: None,
                            phase: SessionPhase::AwaitHello,
                            epoch: 1,
                            baseline: 0,
                            visibility_counter: 0,
                            visible: BTreeMap::new(),
                            inputs: BTreeMap::new(),
                            actions: BTreeMap::new(),
                            last_request: 0,
                            repair_view: None,
                            repair_clear: None,
                            status_revision: 1,
                            status_dirty: false,
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
                    Replication::reliable(
                        peer,
                        None,
                        ServerMessage::ServerHello(world.build()),
                        world,
                        self,
                    );
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
            let Some(mut s) = self.peers.remove(&peer) else {
                continue;
            };
            match self.message(
                peer,
                &mut s,
                message,
                profiles,
                now,
                allow_joins,
                world,
                systems,
            ) {
                Ok(()) => {
                    self.peers.insert(peer, s);
                }
                Err(reason) => {
                    Sessions::cleanup(&s, profiles, world, systems);
                    self.closing.insert(peer, reason);
                }
            }
        }
        // Profile/name persistence completes before any player allocation or Welcome.
        for peer in self.peers.keys().copied().collect::<Vec<_>>() {
            let Some(mut s) = self.peers.remove(&peer) else {
                continue;
            };
            let result = s
                .pending_admission
                .as_deref()
                .and_then(|id| profiles.admission_result(id));
            if let Some(result) = result {
                s.pending_admission = None;
                if !allow_joins || result.as_ref().is_err() {
                    Replication::reliable(
                        peer,
                        None,
                        ServerMessage::Reject(Reject {
                            code: RejectCode::Unavailable,
                            detail: DetailText("profile storage unavailable".into()),
                        }),
                        world,
                        self,
                    );
                    self.closing.insert(peer, "profile storage unavailable");
                    continue;
                }
                if self
                    .admit_profile(
                        peer,
                        &mut s,
                        result.expect("successful admission"),
                        profiles,
                        now,
                        world,
                        systems,
                    )
                    .is_err()
                {
                    Sessions::cleanup(&s, profiles, world, systems);
                    self.closing.insert(peer, "profile admission failed");
                    continue;
                }
            }
            self.peers.insert(peer, s);
        }
        // Apply only committed profile colours, including changes made from a paused menu.
        let mut changed = Vec::new();
        for session in self.peers.values_mut() {
            let (Some(player_id), Some(profile)) = (session.player, session.profile.as_mut())
            else {
                continue;
            };
            let Some(committed) = profiles.committed(&profile.profile_id.0) else {
                continue;
            };
            let Some(player) = world.players.get_mut(&player_id.0) else {
                continue;
            };
            if player.tshirt_colour != committed.tshirt_colour {
                player.tshirt_colour = committed.tshirt_colour;
                changed.push(player_id.0);
            }
            profile.tshirt_colour = committed.tshirt_colour;
        }
        if !changed.is_empty() {
            Replication::refresh_appearance(&changed, world, self, systems)?;
        }
        // Replaced guests have already been removed. Kill the old transport as well.
        for (peer, _, message) in &self.output {
            if matches!(message.message, ServerMessage::Disconnect(_)) {
                self.closing.insert(*peer, "guest replaced");
            }
        }
        Ok(())
    }
}

impl Session {
    /// The entity allocated at admission; callers must have checked the session phase.
    pub(crate) fn player_id(&self) -> u32 {
        self.player.expect("admitted player").0
    }
}
