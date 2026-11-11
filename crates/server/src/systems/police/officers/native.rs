//! Native officer deployment and deterministic decision/motor systems.
use super::*;
use crate::{
    ecs::state::{Clock, Collision, EntityAllocator, SimulationConfig},
    ecs::{EntityIndex, GameId},
    systems::{
        planning::Pending,
        weapons::{OfficerShotRequested, OfficerWeaponTick},
    },
};
use bevy_ecs::prelude::*;
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct DeployActors<'w, 's> {
    units: Query<'w, 's, (&'static GameId, &'static Unit)>,
    cars: Query<'w, 's, (&'static GameId, &'static VehicleComponent)>,
    players: Query<'w, 's, (&'static GameId, &'static PlayerComponent, &'static Wanted)>,
    peds: Query<'w, 's, &'static PedestrianComponent>,
    officers: Query<'w, 's, (Entity, &'static GameId, &'static mut Officer)>,
    commands: Commands<'w, 's>,
}
pub(crate) fn deploy(
    mut actors: DeployActors,
    sessions: Res<Sessions>,
    clock: Res<Clock>,
    collision: NonSend<Collision>,
    mut next_id: ResMut<EntityAllocator>,
) {
    let units: BTreeMap<_, _> = actors.units.iter().map(|(id, u)| (id.0.0, u)).collect();
    let cars: BTreeMap<_, _> = actors.cars.iter().map(|(id, c)| (id.0.0, c)).collect();
    let players: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(id, p, _)| (id.0.0, p))
        .collect();
    let wanted: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(id, _, w)| (id.0.0, w))
        .collect();
    let peds: Vec<_> = actors.peds.iter().collect();
    let identities: BTreeMap<_, _> = actors
        .officers
        .iter()
        .map(|(e, id, _)| (id.0.0, e))
        .collect();
    let mut officers: BTreeMap<_, _> = actors
        .officers
        .iter_mut()
        .map(|(_, id, o)| (id.0.0, Pending::Live(o.into_inner())))
        .collect();
    let mut removed = Vec::new();
    let active_units: BTreeSet<_> = units.keys().copied().collect();
    officers.retain(|id, officer| {
        let keep = active_units.contains(&officer.unit);
        if !keep {
            removed.push(*id);
        }
        keep
    });
    let playing: BTreeSet<_> = sessions
        .peers
        .values()
        .filter(|s| s.phase == SessionPhase::Playing)
        .filter_map(|s| s.player.map(|id| id.0))
        .collect();
    let tick = clock.0;
    let deploying_units: Vec<_> = units
        .iter()
        .map(|(&id, u)| (id, u.target, u.role))
        .collect();
    for (id, unit_target, unit_role) in deploying_units {
        let car = cars[&id].clone();
        let Some(target) = unit_target.filter(|target| playing.contains(target)) else {
            continue;
        };
        let wanted = wanted[&target].clone();
        if car.state.health.0 == 0
            || car.state.disabled
            || car.state.motor.speed.0.abs() > 1.0
            || wanted.heat == 0
            || tick.saturating_sub(wanted.last_seen) > 30
        {
            continue;
        }
        let Some(player) = players.get(&target) else {
            continue;
        };
        if player.state.mode != CharacterMode::OnFoot || player.state.health.0 == 0 {
            continue;
        }
        let car_position = car.state.motor.position.0.map(|v| v.0);
        if !sight(&collision.0, car_position, wanted.last_known, 20.0) {
            continue;
        }
        let roster: Vec<_> = officers
            .iter()
            .filter(|(_, officer)| officer.unit == id)
            .map(|(&id, officer)| (id, officer.mounted && officer.state.health.0 > 0))
            .collect();
        let seats: usize = if wanted.heat == 1 { 1 } else { 2 };
        let exposed = roster
            .iter()
            .filter(|(id, _)| !officers[id].mounted && officers[id].state.health.0 > 0)
            .count();
        let missing = seats.saturating_sub(roster.len());
        let mut deployments: Vec<_> = roster
            .into_iter()
            .filter_map(|(id, mounted)| mounted.then_some(Some(id)))
            .collect();
        deployments.extend(std::iter::repeat_n(None, missing));
        for existing in deployments.into_iter().take(seats.saturating_sub(exposed)) {
            if existing.is_none() && officers.len() >= OFFICER_CAP {
                break;
            }
            let Some(position) = safe_exit_with(&car, &collision.0, |position| {
                !collision
                    .0
                    .overlaps_character(position, QueryFilter::MOVEMENT)
                    .unwrap_or(true)
                    && players.values().all(|p| {
                        p.state.mode == CharacterMode::Downed
                            || distance(position, p.state.motor.position.0.map(|v| v.0)) > 1.0
                    })
                    && peds.iter().all(|p| {
                        p.state.health.0 == 0
                            || distance(position, p.state.motor.position.0.map(|v| v.0)) > 1.0
                    })
                    && officers.values().all(|officer| {
                        officer.mounted
                            || distance(position, officer.state.motor.position.0.map(|v| v.0)) > 1.0
                    })
            }) else {
                break;
            };
            let Ok(motor) =
                sim_core::spawn_character(position, yaw_to(position, wanted.last_known))
            else {
                break;
            };
            if let Some(id) = existing {
                let officer = officers.get_mut(&id).expect("mounted roster");
                officer.mounted = false;
                officer.state.motor = motor;
                continue;
            }
            let id_new = next_id.0;
            let Some(next) = id_new.checked_add(1) else {
                break;
            };
            next_id.0 = next;
            let rifle = wanted.heat == 3
                && unit_role == Role::Lead
                && !officers.values().any(|officer| {
                    officer.unit == id && officer.weapon.equipped == WeaponId::Rifle
                });
            let mut weapon = Weapon {
                equipped: if rifle {
                    WeaponId::Rifle
                } else {
                    WeaponId::Pistol
                },
                ..Weapon::default()
            };
            weapon.reserve = u16::from(weapon.definition().capacity) * 2;
            officers.insert(
                id_new,
                Pending::Spawn(Officer {
                    unit: id,
                    mounted: false,
                    state: CharacterState {
                        motor,
                        equipment: None,
                        mode: CharacterMode::OnFoot,
                        vehicle_id: None,
                        health: CharacterHealth(100),
                    },
                    weapon,
                    assigned: None,
                    warned_at: None,
                    aiming_since: None,
                    route: VecDeque::new(),
                    route_goal: None,
                    next_plan: 0,
                }),
            );
        }
    }

    for id in removed {
        crate::systems::planning::despawn(&mut actors.commands, id, identities[&id]);
    }
    for (id, officer) in officers {
        if let Pending::Spawn(officer) = officer {
            crate::systems::planning::spawn(&mut actors.commands, id, officer);
        }
    }
}

#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct DecisionActors<'w, 's> {
    units: Query<'w, 's, (&'static GameId, &'static Unit)>,
    cars: Query<'w, 's, (&'static GameId, &'static VehicleComponent)>,
    players: Query<'w, 's, (&'static GameId, &'static PlayerComponent)>,
    wanted: Query<'w, 's, (&'static GameId, &'static mut Wanted)>,
    officers: Query<'w, 's, (&'static GameId, &'static mut Officer)>,
}
pub(crate) fn decide(
    mut actors: DecisionActors,
    police: Res<PoliceSystem>,
    sessions: Res<Sessions>,
    clock: Res<Clock>,
    collision: NonSend<Collision>,
    config: Res<SimulationConfig>,
    index: Res<EntityIndex>,
    mut weapon_ticks_out: MessageWriter<OfficerWeaponTick>,
    mut shots_out: MessageWriter<OfficerShotRequested>,
) {
    let tick = clock.0;
    let playing: BTreeSet<_> = sessions
        .peers
        .values()
        .filter(|s| s.phase == SessionPhase::Playing)
        .filter_map(|s| s.player.map(|id| id.0))
        .collect();
    let units_view: BTreeMap<_, _> = actors.units.iter().map(|(id, u)| (id.0.0, u)).collect();
    let cars_view: BTreeMap<_, _> = actors.cars.iter().map(|(id, c)| (id.0.0, c)).collect();
    let players_view: BTreeMap<_, _> = actors.players.iter().map(|(id, p)| (id.0.0, p)).collect();
    let mut wanted_view: BTreeMap<_, _> = actors
        .wanted
        .iter_mut()
        .map(|(id, w)| (id.0.0, w.into_inner()))
        .collect();
    let mut officers: BTreeMap<_, _> = actors
        .officers
        .iter_mut()
        .map(|(id, o)| (id.0.0, o.into_inner()))
        .collect();
    let mut shots = Vec::new();
    let mut firing = BTreeSet::new();
    let mut route_budget = 2;
    let mut occupied_positions: BTreeMap<_, _> = officers
        .iter()
        .filter(|(_, officer)| !officer.mounted && officer.state.health.0 > 0)
        .map(|(&id, officer)| (id, officer.state.motor.position.0.map(|v| v.0)))
        .collect();
    // These actors are read-only throughout officer decisions. Snapshot their
    // observation state before borrowing officer components for mutation.
    struct TargetView {
        state: CharacterState,
        aiming: bool,
        protected_until: Option<u32>,
    }
    let players: BTreeMap<_, _> = players_view
        .iter()
        .map(|(&id, p)| {
            (
                id,
                TargetView {
                    state: p.state.clone(),
                    aiming: p.weapon.aiming,
                    protected_until: p.lifecycle.protected_until,
                },
            )
        })
        .collect();
    let vehicles: BTreeMap<_, _> = cars_view
        .iter()
        .map(|(&id, v)| (id, (*v).clone()))
        .collect();
    let unit_targets: BTreeMap<_, _> = units_view.iter().map(|(&id, u)| (id, u.target)).collect();
    let wanted_observations: BTreeMap<_, _> = wanted_view
        .iter()
        .map(|(&id, w)| (id, (**w).clone()))
        .collect();
    // Engagement writes are staged while officer components are borrowed. The
    // local overlay makes them visible to subsequent officers in this same tick.
    let mut engagements: BTreeMap<u32, (bool, u32)> = BTreeMap::new();
    let mut weapon_ticks = Vec::new();
    for id in officers.keys().copied().collect::<Vec<_>>() {
        let officer = officers.get_mut(&id).expect("officer roster");

        if officer.mounted && vehicles[&officer.unit].state.health.0 == 0 {
            officer.state.health = CharacterHealth(0);
        }
        if officer.mounted || officer.state.health.0 == 0 {
            continue;
        }
        let car = &vehicles[&officer.unit];
        let p = officer.state.motor.position.0.map(|v| v.0);
        let target = unit_targets[&officer.unit]
            .filter(|id| playing.contains(id))
            .and_then(|id| {
                let player = players.get(&id)?;
                let wanted = wanted_observations.get(&id)?;
                (wanted.heat > 0
                    && !wanted.arrest_pending
                    && player.state.health.0 > 0
                    && matches!(
                        player.state.mode,
                        CharacterMode::OnFoot | CharacterMode::Driving
                    ))
                .then_some((id, player, wanted))
            });
        if officer.assigned != target.map(|(id, _, _)| id) {
            officer.assigned = target.map(|(id, _, _)| id);
            officer.warned_at = None;
            officer.aiming_since = None;
            officer.route.clear();
            officer.next_plan = 0;
        }
        let car_position = car.state.motor.position.0.map(|v| v.0);
        let returning = target.is_none_or(|(_, _, wanted)| {
            distance(wanted.last_known, car_position) > 35.0
                || tick.saturating_sub(wanted.last_seen) >= 150
        });
        let mut goal = car_position;
        let mut aim = None;
        if returning {
            if car.state.health.0 == 0 {
                continue;
            }
            if let Some(door) = safe_exit_with(car, &collision.0, |_| true) {
                goal = door;
                if distance(p, door) < 1.1 {
                    occupied_positions.remove(&id);
                    officer.mounted = true;
                    officer.state.equipment = None;
                    continue;
                }
            }
        } else if let Some((target_id, player, wanted)) = target {
            let actual = player
                .state
                .vehicle_id
                .and_then(|id| vehicles.get(&id.0))
                .map_or(player.state.motor.position.0, |car| {
                    car.state.motor.position.0
                })
                .map(|v| v.0);
            let visible = sight(&collision.0, p, actual, 38.0);
            goal = if visible { actual } else { wanted.last_known };
            if visible {
                let recent = |t: Option<u32>| t.is_some_and(|t| tick.saturating_sub(t) < 150);
                let authorized = engagements
                    .get(&target_id)
                    .map_or(wanted.fire_authorized, |e| e.0);
                let aiming_at_officer = player.aiming
                    && wrap(player.state.motor.yaw.0 - yaw_to(actual, p)).abs() <= 0.45;
                let resisting = aiming_at_officer || recent(wanted.last_gunfire);
                let protected = player.protected_until.is_some_and(|end| tick < end);
                if !protected
                    && !wanted.surrendering
                    && (authorized || (wanted.heat >= 2 && resisting))
                {
                    let warned = *officer.warned_at.get_or_insert(tick);
                    let height = if player.state.vehicle_id.is_some() {
                        0.8
                    } else {
                        1.25
                    };
                    let may_fire = authorized || tick.saturating_sub(warned) >= 30;
                    if may_fire && !authorized {
                        engagements
                            .entry(target_id)
                            .or_insert((wanted.fire_authorized, wanted.next_officer_shot))
                            .0 = true;
                    }
                    aim = Some((
                        target_id,
                        [actual[0], actual[1] + height, actual[2]],
                        may_fire,
                    ));
                } else {
                    officer.warned_at = None;
                }
            }
        }
        if aim.is_none() {
            officer.warned_at = None;
        }
        if let Some((_, threat, _)) = aim
            && officer.weapon.reload_end.is_some()
            && car.state.health.0 > 0
            && distance(p, car_position) < 10.0
        {
            let away = yaw_to(threat, car_position);
            let cover = [
                car_position[0] - away.sin() * 3.4,
                car_position[1],
                car_position[2] - away.cos() * 3.4,
            ];
            let delta = [
                cover[0] - threat[0],
                cover[1] + 0.9 - threat[1],
                cover[2] - threat[2],
            ];
            let length = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
            if sim_core::collision::ray_vehicle(
                threat,
                delta.map(|v| v / length.max(0.01)),
                car_position,
                car.state.motor.yaw.0,
                car.template.dimensions.map(|v| v as f32),
                length,
            )
            .is_some()
            {
                goal = cover;
                aim = None;
            }
        }
        if let Some((target_id, threat, _)) = aim {
            let occupied = players[&target_id].state.vehicle_id.map(|id| id.0);
            let clear_fire_lane = |position: [f32; 3]| {
                let origin = [position[0], position[1] + 1.45, position[2]];
                let delta = std::array::from_fn::<_, 3, _>(|i| threat[i] - origin[i]);
                let length = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
                let direction = delta.map(|v| v / length.max(0.01));
                occupied_positions.iter().all(|(&other, &point)| {
                    other == id
                        || sim_core::collision::ray_character(origin, direction, point, length)
                            .is_none()
                }) && vehicles.iter().all(|(&id, vehicle)| {
                    Some(id) == occupied
                        || sim_core::collision::ray_vehicle(
                            origin,
                            direction,
                            vehicle.state.motor.position.0.map(|v| v.0),
                            vehicle.state.motor.yaw.0,
                            vehicle.template.dimensions.map(|v| v as f32),
                            length,
                        )
                        .is_none()
                })
            };
            if !clear_fire_lane(p) {
                let facing = yaw_to(p, threat);
                let side = [facing.cos(), 0.0, -facing.sin()];
                let cast_options = sim_core::collision::CharacterCastOptions {
                    was_grounded: officer.state.motor.grounded,
                    allow_snap: false,
                    filter: QueryFilter::MOVEMENT,
                };
                goal = [2.5, -2.5, 4.0, -4.0]
                    .into_iter()
                    .find_map(|width| {
                        let delta = side.map(|v| v * width);
                        let candidate = std::array::from_fn(|i| p[i] + delta[i]);
                        (clear_fire_lane(candidate)
                            && collision
                                .0
                                .cast_character(p, delta, cast_options)
                                .is_ok_and(|cast| {
                                    (cast.applied[0] - delta[0]).abs() < 0.1
                                        && (cast.applied[2] - delta[2]).abs() < 0.1
                                }))
                        .then_some(candidate)
                    })
                    .unwrap_or(p);
                aim = None;
            }
        }
        let mut direction_goal = goal;
        let mut walk = aim.is_none() && distance(p, goal) > if returning { 0.8 } else { 2.1 };
        if walk && !sight(&collision.0, p, goal, 35.0) {
            let end = police.sidewalk.nearest(goal);
            if tick >= officer.next_plan
                && route_budget > 0
                && (officer.route.is_empty() || officer.route_goal != end)
            {
                route_budget -= 1;
                officer.next_plan = tick.saturating_add(30);
                officer.route_goal = end;
                officer.route = police
                    .sidewalk
                    .nearest(p)
                    .zip(end)
                    .map_or_else(VecDeque::new, |(start, end)| {
                        police.sidewalk.path(start, end)
                    });
            }
            while officer
                .route
                .front()
                .is_some_and(|node| distance(p, police.sidewalk.points[node]) < 1.1)
            {
                officer.route.pop_front();
            }
            if let Some(node) = officer.route.front() {
                direction_goal = police.sidewalk.points[node];
            } else {
                walk = false;
            }
        }
        let look = aim.map_or(direction_goal, |(_, position, _)| position);
        let yaw = yaw_to(p, look);
        let mut input = sim_core::neutral_input(tick, Yaw(yaw));
        input.buttons = Buttons(8);
        if walk {
            input.move_z = InputAxis(24000);
        }
        if let Ok(result) = sim_core::step_character(
            &officer.state.motor,
            &input,
            &sim_core::StepContext {
                tick,
                collision: &collision.0,
                config: &config.0,
            },
        ) {
            let next = result.motor.position.0.map(|v| v.0);
            if occupied_positions.iter().all(|(&other, &point)| {
                other == id
                    || distance(next, point) >= 0.9
                    || distance(next, point) >= distance(p, point)
            }) {
                officer.state.motor = result.motor;
                occupied_positions.insert(id, next);
            } else {
                officer.state.motor.velocity = Vec3([Scalar(0.0); 3]);
            }
        }
        // Pure readiness preview lets AI reserve its per-target firing slot.
        // The native weapon consumer commits timers, ammunition and presentation.
        let mut ready_weapon = officer.weapon.clone();
        ready_weapon.advance(tick);
        if ready_weapon.magazine == 0 {
            ready_weapon.reload(tick);
        }
        let origin = [p[0], p[1] + 1.45, p[2]];
        let pitch = (look[1] - origin[1])
            .atan2(distance(origin, look))
            .clamp(-1.2, 1.2);
        weapon_ticks.push((id, aim.is_some(), Pitch(pitch)));
        let Some((target, _, may_fire)) = aim else {
            officer.aiming_since = None;
            continue;
        };
        let since = *officer.aiming_since.get_or_insert(tick);
        if !may_fire
            || tick.saturating_sub(since) < 12
            || ready_weapon.reload_end.is_some()
            || ready_weapon.magazine == 0
            || tick < ready_weapon.next_fire
            || tick
                < engagements
                    .get(&target)
                    .map_or(wanted_observations[&target].next_officer_shot, |e| e.1)
            || !firing.insert(target)
        {
            continue;
        }
        let wanted = &wanted_observations[&target];
        engagements
            .entry(target)
            .or_insert((wanted.fire_authorized, wanted.next_officer_shot))
            .1 = tick + 12;
        let definition = officer.weapon.definition();
        let seed = id.wrapping_mul(0x9e3779b9) ^ tick.wrapping_mul(0x85ebca6b);
        let jitter = |shift| ((seed.rotate_left(shift) & 255) as f32 / 255.0 - 0.5) * 0.045;
        let (sin, cos) = (yaw + jitter(0)).sin_cos();
        let (sp, cp) = (pitch + jitter(13)).sin_cos();
        shots.push(OfficerShot {
            shooter: id,
            target,
            origin,
            direction: [-sin * cp, sp, -cos * cp],
            range: (definition.range as f32).min(40.0),
            damage: definition.damage,
            weapon: officer.weapon.equipped,
            sequence: officer.weapon.shot_sequence.saturating_add(1),
        });
    }
    for (id, (authorized, next_shot)) in engagements {
        let wanted = wanted_view.get_mut(&id).expect("active pursuit");
        wanted.fire_authorized = authorized;
        wanted.next_officer_shot = next_shot;
    }
    for (id, aiming, pitch) in weapon_ticks {
        if let Some(entity) = index.0.get(&id).copied() {
            weapon_ticks_out.write(OfficerWeaponTick {
                officer: EntityId(id),
                entity,
                tick,
                aiming,
                pitch,
            });
        }
    }

    for shot in shots {
        if let (Some(&shooter), Some(&target)) =
            (index.0.get(&shot.shooter), index.0.get(&shot.target))
        {
            shots_out.write(OfficerShotRequested {
                shot,
                shooter,
                target,
                tick,
            });
        }
    }
}
