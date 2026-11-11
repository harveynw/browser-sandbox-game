//! Native mission progression, recovery lifecycle and durable reward messages.
use super::*;
use crate::{
    components::{PlayerMission, RecoveryReservation},
    ecs::state::{Clock, Collision, Content},
    ecs::{EntityIndex, GameId, TickOutcome},
    gameplay_messages::ControlTransitionRequested,
};
use bevy_ecs::prelude::*;
use std::result::Result;
#[derive(Message, Clone)]
pub(crate) struct RewardRequested {
    pub peer: crate::transport::PeerKey,
    pub player: EntityId,
    pub entity: Entity,
    pub session: String,
    pub profile: String,
    pub mission: String,
    pub amount: u32,
    pub tick: u32,
}
#[derive(Message, Clone)]
pub(crate) struct RewardCommitted {
    pub request: RewardRequested,
    pub profile: ProfileView,
}
#[derive(Message, Clone, Copy)]
pub(crate) struct RecoveryRetired {
    pub car: EntityId,
    pub entity: Entity,
    pub tick: u32,
}
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct JobActors<'w, 's> {
    players: Query<'w, 's, (&'static GameId, &'static mut PlayerComponent)>,
    cars: Query<'w, 's, (&'static GameId, &'static mut VehicleComponent)>,
    missions: Query<'w, 's, &'static mut PlayerMission>,
    reservations: Query<'w, 's, &'static RecoveryReservation>,
    peds: Query<'w, 's, &'static PedestrianComponent>,
    index: Res<'w, EntityIndex>,
    collision: NonSendMut<'w, Collision>,
    controls: MessageWriter<'w, ControlTransitionRequested>,
    retired: MessageWriter<'w, RecoveryRetired>,
}
impl JobActors<'_, '_> {
    fn player(&self, id: u32) -> &PlayerComponent {
        self.players
            .get(self.index.0[&id])
            .expect("mission player")
            .1
    }
    fn car(&self, id: u32) -> &VehicleComponent {
        self.car_opt(id).expect("mission car")
    }
    fn car_opt(&self, id: u32) -> Option<&VehicleComponent> {
        self.index
            .0
            .get(&id)
            .and_then(|e| self.cars.get(*e).ok())
            .map(|(_, c)| c)
    }
    fn mission_mut(&mut self, id: u32) -> Mut<'_, PlayerMission> {
        self.missions
            .get_mut(self.index.0[&id])
            .expect("mission component")
    }
    fn reserved(&self, id: u32) -> bool {
        self.index
            .0
            .get(&id)
            .is_some_and(|e| self.reservations.get(*e).is_ok())
    }
    fn clear(&self, p: [f32; 3], owner: u32) -> bool {
        !self
            .collision
            .0
            .overlaps_character(p, sim_core::QueryFilter::MOVEMENT)
            .unwrap_or(true)
            && self.players.iter().all(|(id, a)| {
                id.0.0 == owner
                    || a.state.mode == CharacterMode::Downed
                    || distance(p, a.state.motor.position.0.map(|v| v.0)) > 1.0
            })
            && self.peds.iter().all(|a| {
                a.state.health.0 == 0 || distance(p, a.state.motor.position.0.map(|v| v.0)) > 1.0
            })
    }
    fn retire(
        &mut self,
        peer: crate::transport::PeerKey,
        s: &mut Session,
        car: u32,
        tick: u32,
    ) -> Result<bool, String> {
        if !self.reserved(car) {
            return Ok(true);
        }
        let id = s.player.ok_or("recovery owner missing")?;
        if self.player(id.0).state.vehicle_id == Some(EntityId(car)) {
            crate::systems::vehicle::stop(
                &mut self
                    .cars
                    .get_mut(self.index.0[&car])
                    .map_err(|_| "recovery car missing")?
                    .1,
            );
            self.collision.0.exclude_vehicle(Some(car));
            let exit =
                crate::systems::vehicle::safe_exit_with(self.car(car), &self.collision.0, |p| {
                    self.clear(p, id.0)
                });
            self.collision.0.exclude_vehicle(None);
            let Some(exit) = exit else {
                return Ok(false);
            };
            let yaw = self.car(car).state.motor.yaw.0;
            self.cars
                .get_mut(self.index.0[&car])
                .unwrap()
                .1
                .state
                .driver_id = None;
            let (_, mut p) = self.players.get_mut(self.index.0[&id.0]).unwrap();
            p.state.vehicle_id = None;
            p.state.mode = CharacterMode::OnFoot;
            p.state.motor = sim_core::spawn_character(exit, yaw)?;
            crate::systems::weapons::state::cancel(&mut p.weapon);
            s.status_dirty = true;
            self.controls.write(ControlTransitionRequested {
                peer,
                player: id,
                player_entity: self.index.0[&id.0],
                changed: vec![id.0, car],
                tick,
            });
        }
        self.retired.write(RecoveryRetired {
            car: EntityId(car),
            entity: self.index.0[&car],
            tick,
        });
        Ok(true)
    }
}
#[path = "progression.rs"]
mod progression;
pub(crate) use progression::progress;

pub(crate) fn persist_rewards(
    mut requests: MessageReader<RewardRequested>,
    mut io: NonSendMut<crate::ecs::runtime::TickIo>,
    repairs: Res<RepairSystem>,
    mut committed: MessageWriter<RewardCommitted>,
    clock: Res<Clock>,
) {
    for request in requests.read() {
        if request.tick != clock.0 {
            continue;
        }
        if !repairs.pending.contains_key(&request.profile)
            && !io.profiles.economic_pending(&request.profile)
            && let Ok(profile) =
                io.profiles
                    .apply_mission_reward(&request.profile, &request.mission, request.amount)
        {
            committed.write(RewardCommitted {
                request: request.clone(),
                profile,
            });
        }
    }
}
pub(crate) fn apply_rewards(
    mut messages: MessageReader<RewardCommitted>,
    mut missions: Query<(&GameId, &mut PlayerMission)>,
    mut sessions: ResMut<Sessions>,
    clock: Res<Clock>,
) {
    for message in messages.read() {
        let r = &message.request;
        if r.tick != clock.0 {
            continue;
        }
        let Some(s) = sessions.peers.get_mut(&r.peer) else {
            continue;
        };
        if s.id.as_deref() != Some(&r.session) || s.player != Some(r.player) {
            continue;
        }
        let Ok((id, mut mission)) = missions.get_mut(r.entity) else {
            continue;
        };
        if id.0 != r.player {
            continue;
        }
        if let Some(m) = &mut mission.0
            && m.id == r.mission
            && m.phase == Phase::RewardPending
        {
            s.profile = Some(message.profile.clone());
            s.status_dirty = true;
            m.phase = Phase::Completed;
            m.clear_at = Some(clock.0.saturating_add(150));
        }
    }
}
pub(crate) fn remove_retired(
    mut messages: MessageReader<RecoveryRetired>,
    actors: Query<&GameId, With<RecoveryReservation>>,
    clock: Res<Clock>,
    mut commands: Commands,
) {
    for m in messages.read() {
        if m.tick == clock.0 && actors.get(m.entity).is_ok_and(|id| id.0 == m.car) {
            crate::systems::planning::despawn(&mut commands, m.car.0, m.entity);
        }
    }
}
pub(crate) fn refresh_proxies(
    cars: Query<(&GameId, &VehicleComponent)>,
    mut collision: NonSendMut<Collision>,
    mut outcome: ResMut<TickOutcome>,
) {
    let cars: BTreeMap<_, _> = cars.iter().map(|(id, c)| (id.0.0, c)).collect();
    if let Err(e) = collision.0.set_vehicle_proxies(
        &cars
            .values()
            .map(|c| crate::systems::vehicle::proxy(c))
            .collect::<Vec<_>>(),
    ) {
        outcome.fail(e);
    }
}

#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct AcceptanceActors<'w, 's> {
    players: Query<'w, 's, (&'static GameId, &'static mut PlayerComponent)>,
    cars: Query<'w, 's, (&'static GameId, &'static VehicleComponent)>,
    peds: Query<'w, 's, (&'static GameId, &'static PedestrianComponent)>,
    missions: Query<'w, 's, &'static mut PlayerMission>,
    reservations: Query<'w, 's, (&'static GameId, &'static RecoveryReservation)>,
    pools: Query<'w, 's, (&'static GameId, &'static crate::components::PoolMembership)>,
    commands: Commands<'w, 's>,
}
fn accept(
    s: &mut Session,
    template_id: &str,
    tick: u32,
    manifest: &content::WorldManifest,
    collision: &sim_core::CollisionWorld,
    next_id: &mut u32,
    index: &EntityIndex,
    repairs: &RepairSystem,
    actors: &mut AcceptanceActors,
) -> ActionCode {
    let players: BTreeMap<_, _> = actors.players.iter().map(|(id, p)| (id.0.0, p)).collect();
    let cars: BTreeMap<_, _> = actors.cars.iter().map(|(id, p)| (id.0.0, p)).collect();
    let peds: BTreeMap<_, _> = actors.peds.iter().map(|(id, p)| (id.0.0, p)).collect();
    let missions: BTreeMap<_, _> = index
        .0
        .iter()
        .filter_map(|(id, e)| actors.missions.get(*e).ok().map(|m| (*id, &m.0)))
        .collect();
    let reservations: BTreeMap<_, _> = actors
        .reservations
        .iter()
        .map(|(id, r)| (id.0.0, &r.0))
        .collect();
    let pools: BTreeMap<_, _> = actors.pools.iter().map(|(id, p)| (id.0.0, &p.0)).collect();

    if s.profile
        .as_ref()
        .is_some_and(|p| repairs.pending.contains_key(&p.profile_id.0))
    {
        return ActionCode::AlreadyActive;
    }
    if missions
        .get(&(s.player_id()))
        .expect("admitted mission state")
        .is_some()
    {
        return ActionCode::AlreadyActive;
    }
    let Some(template) = manifest
        .mission_templates
        .iter()
        .find(|t| t.id == template_id)
        .cloned()
    else {
        return ActionCode::InvalidTarget;
    };
    let id = s.player.expect("job player");
    let player = &players[&id.0];
    let position = player
        .state
        .vehicle_id
        .map(|id| cars[&id.0].state.motor.position.0.map(|v| v.0))
        .unwrap_or_else(|| player.state.motor.position.0.map(|v| v.0));
    let Some(terminal) = manifest
        .zones
        .iter()
        .filter(|z| {
            z.kind == content::ZoneKind::Terminal && z.allowed_templates.contains(&template.id)
        })
        .min_by(|a, b| {
            distance(position, a.center.map(|v| v as f32))
                .total_cmp(&distance(position, b.center.map(|v| v as f32)))
        })
    else {
        return ActionCode::Unavailable;
    };
    if distance(position, terminal.center.map(|v| v as f32)) > terminal.radius.min(3.0) as f32 {
        return ActionCode::OutOfRange;
    }
    let Ok(instance) = random_id("mission-", 16) else {
        return ActionCode::Unavailable;
    };
    let variant_index = selection::variant(&instance, template.variants.len());
    let variant = variant_index.map(|i| &template.variants[usize::from(i)]);
    let duration = variant.map_or(template.duration_ticks, |v| v.duration_ticks);
    let reward = variant.map_or(template.reward_credits, |v| v.reward_credits);
    let objective = variant
        .map_or(&template.objective, |v| &v.objective)
        .clone();
    let Some(deadline) = tick.checked_add(duration) else {
        return ActionCode::Unavailable;
    };
    let mut m = Mission {
        id: instance,
        template: template.id,
        variant_index,
        deadline,
        target: [0.0; 3],
        radius: 5.0,
        max_speed: 2.0,
        required_dwell: 30,
        reward,
        dwell: 0,
        phase: Phase::Active,
        clear_at: None,
        objective: Objective::Courier,
        checkpoint_index: 0,
        reserved_vehicle: None,
    };
    match objective {
        content::MissionObjective::Courier {
            destination_zone_id,
            dwell_ticks,
            max_speed,
        } => {
            let Some(zone) = manifest.zones.iter().find(|z| z.id == destination_zone_id) else {
                return ActionCode::Unavailable;
            };
            m.target = zone.center.map(|v| v as f32);
            m.radius = zone.radius.min(5.0) as f32;
            m.required_dwell = dwell_ticks;
            m.max_speed = max_speed as f32;
        }
        content::MissionObjective::TimeTrial {
            checkpoints,
            gate_radius,
        } => {
            if player.state.vehicle_id.is_none() {
                return ActionCode::InvalidTarget;
            }
            if checkpoints.len() < 2
                || distance(position, checkpoints[0].map(|v| v as f32)) > gate_radius as f32
            {
                return ActionCode::OutOfRange;
            }
            m.checkpoint_index = 1;
            m.target = checkpoints[1].map(|v| v as f32);
            m.objective = Objective::TimeTrial {
                checkpoints: checkpoints
                    .into_iter()
                    .map(|p| p.map(|v| v as f32))
                    .collect(),
                gate_radius: gate_radius as f32,
            };
        }
        content::MissionObjective::Recovery {
            parking_ids,
            garage_zone_id,
        } => {
            if reservations.len() >= crate::systems::population::RECOVERY_RESERVED
                || pools
                    .values()
                    .filter(|p| ***p == crate::systems::population::VehiclePool::Parked)
                    .count()
                    >= crate::systems::population::PARKED_CAP
            {
                return ActionCode::Unavailable;
            }
            let Some(garage) = manifest.zones.iter().find(|z| z.id == garage_zone_id) else {
                return ActionCode::Unavailable;
            };
            let garage = garage.center.map(|v| v as f32);
            let slot = manifest
                .parking
                .iter()
                .filter(|slot| {
                    if !slot.mission_reserved
                        || !parking_ids.contains(&slot.id)
                        || reservations.values().any(|r| r.slot == slot.id)
                    {
                        return false;
                    }
                    let p = slot.position.map(|v| v as f32);
                    let Some(t) = manifest
                        .vehicle_templates
                        .iter()
                        .find(|t| t.id == slot.template_id)
                    else {
                        return false;
                    };
                    !collision
                        .overlaps_vehicle(p, slot.yaw as f32, t.dimensions.map(|v| v as f32))
                        .unwrap_or(true)
                        && players
                            .values()
                            .all(|a| distance(p, a.state.motor.position.0.map(|v| v.0)) > 6.0)
                        && peds
                            .values()
                            .all(|a| distance(p, a.state.motor.position.0.map(|v| v.0)) > 6.0)
                })
                .min_by_key(|slot| selection::rank(&m.id, &slot.id))
                .cloned();
            let Some(slot) = slot else {
                return ActionCode::Unavailable;
            };
            let template = manifest
                .vehicle_templates
                .iter()
                .find(|t| t.id == slot.template_id)
                .expect("validated recovery template")
                .clone();
            let car = *next_id;
            let Some(next) = car.checked_add(1) else {
                return ActionCode::Unavailable;
            };
            let Ok(motor) =
                sim_core::spawn_vehicle(slot.position.map(|v| v as f32), slot.yaw as f32)
            else {
                return ActionCode::Unavailable;
            };
            *next_id = next;
            m.reserved_vehicle = Some(car);
            m.target = slot.position.map(|v| v as f32);
            m.objective = Objective::Recovery { garage };
            let vehicle = VehicleComponent {
                id: EntityId(car),
                state: VehicleState {
                    motor,
                    health: VehicleHealth(template.health),
                    driver_id: None,
                    disabled: false,
                },
                template: template.into(),
                idle_ticks: 30,
            };
            let bundle = (
                vehicle,
                crate::components::PoolMembership(crate::systems::population::VehiclePool::Parked),
                crate::components::RecoveryReservation(Reservation {
                    slot: slot.id,
                    owner: id.0,
                }),
            );
            if slot.secured {
                crate::systems::planning::spawn(
                    &mut actors.commands,
                    car,
                    (bundle, crate::components::SecuredVehicle),
                );
            } else {
                crate::systems::planning::spawn(&mut actors.commands, car, bundle);
            }
        }
    }
    actors
        .players
        .get_mut(index.0[&s.player_id()])
        .unwrap()
        .1
        .lifecycle
        .protected_until = None;
    actors.missions.get_mut(index.0[&s.player_id()]).unwrap().0 = Some(m);
    s.status_dirty = true;
    ActionCode::Ok
}

#[derive(Message, Clone)]
pub(crate) struct JobActionRequested {
    pub peer: crate::transport::PeerKey,
    pub request: ActionRequest,
    pub tick: u32,
    pub entity: Option<Entity>,
}
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct JobServices<'w> {
    sessions: ResMut<'w, Sessions>,
    repairs: ResMut<'w, RepairSystem>,
    clock: Res<'w, Clock>,
    content: Res<'w, Content>,
    collision: NonSend<'w, Collision>,
    next: ResMut<'w, crate::ecs::state::EntityAllocator>,
    index: Res<'w, EntityIndex>,
    identity: Res<'w, crate::ecs::state::WorldIdentity>,
}
pub(crate) fn actions(
    mut messages: MessageReader<JobActionRequested>,
    mut services: JobServices,
    mut actors: ParamSet<(
        AcceptanceActors,
        crate::systems::repairs::native::RepairActors,
    )>,
    wanted: Query<&crate::systems::police::Wanted>,
) {
    for message in messages.read() {
        if message.tick != services.clock.0 {
            continue;
        }
        let overflow = services
            .sessions
            .peers
            .values()
            .any(|s| s.visibility_counter > u32::MAX - 2);
        let Some(mut s) = services.sessions.peers.remove(&message.peer) else {
            continue;
        };
        let request = &message.request;
        let result = if let Some(cached) = s.actions.get(&request.request_id.0) {
            cached.clone()
        } else {
            let code = (|| {
                if s.phase != SessionPhase::Playing {
                    return ActionCode::NotReady;
                }
                if request.control_epoch.0 != s.epoch {
                    return ActionCode::StaleEpoch;
                }
                if request.request_id.0 <= s.last_request {
                    return ActionCode::StaleRequest;
                }
                if overflow || s.epoch == u32::MAX {
                    return ActionCode::Unavailable;
                }
                let (Some(id), Some(entity)) = (s.player, message.entity) else {
                    return ActionCode::NotReady;
                };
                if services.index.0.get(&id.0) != Some(&entity) {
                    return ActionCode::NotReady;
                }
                {
                    let q = actors.p0();
                    let Ok((actual, p)) = q.players.get(entity) else {
                        return ActionCode::NotReady;
                    };
                    if actual.0 != id {
                        return ActionCode::NotReady;
                    }
                    if p.state.health.0 == 0
                        || matches!(
                            p.state.mode,
                            CharacterMode::Downed | CharacterMode::Arrested
                        )
                        || wanted.get(entity).is_ok_and(|w| w.arrest_pending)
                    {
                        return ActionCode::Dead;
                    }
                }
                match &request.action {
                    Action::AcceptMission(r) => accept(
                        &mut s,
                        &r.template_id.0,
                        services.clock.0,
                        &services.content.0.manifest,
                        &services.collision.0,
                        &mut services.next.0,
                        &services.index,
                        &services.repairs,
                        &mut actors.p0(),
                    ),
                    Action::CancelMission(_) => {
                        let mut q = actors.p0();
                        let mut m = q.missions.get_mut(entity).unwrap();
                        if let Some(m) = &mut m.0
                            && m.phase == Phase::Active
                        {
                            m.fail(services.clock.0);
                            s.status_dirty = true;
                            ActionCode::Ok
                        } else {
                            ActionCode::NotActive
                        }
                    }
                    Action::Repair(r) => crate::systems::repairs::native::request(
                        &mut services.repairs,
                        &mut s,
                        r,
                        request.request_id.0,
                        services.clock.0,
                        &services.content.0.manifest,
                        &mut actors.p1(),
                    ),
                    _ => ActionCode::Unavailable,
                }
            })();
            let affected = if code == ActionCode::Ok {
                s.player
                    .and_then(|id| services.index.0.get(&id.0))
                    .and_then(|e| {
                        actors
                            .p0()
                            .players
                            .get(*e)
                            .ok()
                            .map(|(_, p)| p.state.vehicle_id.unwrap_or(p.id))
                    })
            } else {
                None
            };
            let result = ActionResult {
                request_id: request.request_id,
                applied_tick: services.clock.0,
                ok: code == ActionCode::Ok,
                code,
                affected_entity_id: affected,
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
        crate::replication::Replication::queue(
            message.peer,
            s.id.as_deref(),
            Delivery::Reliable,
            ServerMessage::ActionResult(result),
            &services.identity.0,
            &mut services.sessions,
        );
        services.sessions.peers.insert(message.peer, s);
    }
}
pub(crate) fn retire_actions(mut messages: ResMut<Messages<JobActionRequested>>) {
    messages.clear();
}

pub(crate) fn publish_status(
    players: Query<(
        &GameId,
        &PlayerComponent,
        &crate::systems::police::Wanted,
        &PlayerMission,
    )>,
    index: Res<EntityIndex>,
    clock: Res<Clock>,
    identity: Res<crate::ecs::state::WorldIdentity>,
    mut sessions: ResMut<Sessions>,
    mut outcome: ResMut<TickOutcome>,
) {
    let mut output = Vec::new();
    for (&peer, s) in &mut sessions.peers {
        let Some(id) = s.player.filter(|_| s.status_dirty) else {
            continue;
        };
        let Some(&entity) = index.0.get(&id.0) else {
            outcome.fail("status actor missing".into());
            return;
        };
        let Ok((actual, p, w, m)) = players.get(entity) else {
            outcome.fail("status components missing".into());
            return;
        };
        if actual.0 != id {
            outcome.fail("status identity mismatch".into());
            return;
        }
        let Some(next) = s.status_revision.checked_add(1) else {
            outcome.fail("status revision exhausted".into());
            return;
        };
        s.status_revision = next;
        s.status_dirty = false;
        if s.phase != SessionPhase::Playing {
            continue;
        }
        let Some(profile) = &s.profile else {
            outcome.fail("profile missing".into());
            return;
        };
        output.push((
            peer,
            s.id.clone(),
            PlayerStatus {
                equipped_weapon: p.weapon.equipped,
                inventory: crate::replication::projection::views(&p.weapon),
                equip_end_tick: p.weapon.equip_end,
                revision: Revision(next),
                server_tick: clock.0,
                credits: profile.credits,
                repair: s.repair_view.clone(),
                completed_jobs: profile.completed_jobs,
                heat: Heat(w.heat),
                pursuit: w.pursuit,
                magazine: Magazine(p.weapon.slots[p.weapon.equipped.index()].magazine),
                reload_end_tick: p.weapon.reload_end,
                mission: m.0.as_ref().map(|m| m.view()),
                respawn_tick: p.lifecycle.respawn,
                protected_until_tick: p.lifecycle.protected_until,
            },
        ));
    }
    for (peer, sid, status) in output {
        crate::replication::Replication::queue(
            peer,
            sid.as_deref(),
            Delivery::Reliable,
            ServerMessage::PlayerStatus(status),
            &identity.0,
            &mut sessions,
        );
    }
}
