use super::*;
use crate::entities::{vehicle::safe_exit_with, weapon::Weapon};

const OFFICER_CAP: usize = 16;

pub(crate) struct Officer {
    pub(crate) unit: u32,
    pub(crate) state: CharacterState,
    pub(crate) mounted: bool,
    weapon: Weapon,
    assigned: Option<u32>,
    warned_at: Option<u32>,
    aiming_since: Option<u32>,
    route: VecDeque<u32>,
    route_goal: Option<u32>,
    next_plan: u32,
}

pub(crate) struct OfficerShot {
    pub shooter: u32,
    pub target: u32,
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub range: f32,
    pub damage: u16,
    pub weapon: WeaponId,
    pub sequence: u32,
}

impl PoliceSystem {
    pub(crate) fn damage_officer(&mut self, id: u32, damage: u16) {
        let officer = self.officers.get_mut(&id).expect("traced officer");
        officer.state.health.0 = officer.state.health.0.saturating_sub(damage);
        if officer.state.health.0 == 0 {
            officer.state.mode = CharacterMode::Downed;
            officer.state.motor.velocity = Vec3([Scalar(0.0); 3]);
            officer.state.equipment = None;
        }
    }

    pub(crate) fn step_officers(
        &mut self,
        world: &mut World,
        sessions: &Sessions,
        population: &PopulationSystem,
    ) -> Vec<OfficerShot> {
        self.officers
            .retain(|_, officer| self.units.contains_key(&officer.unit));
        let playing: BTreeSet<_> = sessions
            .peers
            .values()
            .filter(|s| s.phase == SessionPhase::Playing)
            .filter_map(|s| s.player.map(|id| id.0))
            .collect();
        let tick = world.tick;
        for (&id, unit) in &self.units {
            let car = &world.vehicles[&id];
            let Some(target) = unit.target.filter(|target| playing.contains(target)) else {
                continue;
            };
            let wanted = &self.wanted[&target];
            if car.state.health.0 == 0
                || car.state.disabled
                || car.state.motor.speed.0.abs() > 1.0
                || wanted.heat == 0
                || tick.saturating_sub(wanted.last_seen) > 30
            {
                continue;
            }
            let Some(player) = world.players.get(&target) else {
                continue;
            };
            if player.state.mode != CharacterMode::OnFoot || player.state.health.0 == 0 {
                continue;
            }
            let car_position = car.state.motor.position.0.map(|v| v.0);
            if !sight(&world.collision, car_position, wanted.last_known, 20.0) {
                continue;
            }
            let roster: Vec<_> = self
                .officers
                .iter()
                .filter(|(_, officer)| officer.unit == id)
                .map(|(&id, officer)| (id, officer.mounted && officer.state.health.0 > 0))
                .collect();
            let seats: usize = if wanted.heat == 1 { 1 } else { 2 };
            let exposed = roster
                .iter()
                .filter(|(id, _)| {
                    !self.officers[id].mounted && self.officers[id].state.health.0 > 0
                })
                .count();
            let missing = seats.saturating_sub(roster.len());
            let mut deployments: Vec<_> = roster
                .into_iter()
                .filter_map(|(id, mounted)| mounted.then_some(Some(id)))
                .collect();
            deployments.extend(std::iter::repeat_n(None, missing));
            for existing in deployments.into_iter().take(seats.saturating_sub(exposed)) {
                if existing.is_none() && self.officers.len() >= OFFICER_CAP {
                    break;
                }
                let Some(position) = safe_exit_with(car, &world.collision, |position| {
                    CombatSystem::clear_spawn(position, None, world, population)
                        && self.officers.values().all(|officer| {
                            officer.mounted
                                || distance(position, officer.state.motor.position.0.map(|v| v.0))
                                    > 1.0
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
                    let officer = self.officers.get_mut(&id).expect("mounted roster");
                    officer.mounted = false;
                    officer.state.motor = motor;
                    continue;
                }
                let id_new = world.next_entity;
                let Some(next) = id_new.checked_add(1) else {
                    break;
                };
                world.next_entity = next;
                let rifle = wanted.heat == 3
                    && unit.role == Role::Lead
                    && !self.officers.values().any(|officer| {
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
                self.officers.insert(
                    id_new,
                    Officer {
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
                    },
                );
            }
        }

        let mut shots = Vec::new();
        let mut firing = BTreeSet::new();
        let mut route_budget = 2;
        let mut occupied_positions: BTreeMap<_, _> = self
            .officers
            .iter()
            .filter(|(_, officer)| !officer.mounted && officer.state.health.0 > 0)
            .map(|(&id, officer)| (id, officer.state.motor.position.0.map(|v| v.0)))
            .collect();
        for (&id, officer) in &mut self.officers {
            if officer.mounted && world.vehicles[&officer.unit].state.health.0 == 0 {
                officer.state.health = CharacterHealth(0);
            }
            if officer.mounted || officer.state.health.0 == 0 {
                continue;
            }
            let car = &world.vehicles[&officer.unit];
            let p = officer.state.motor.position.0.map(|v| v.0);
            let target = self.units[&officer.unit]
                .target
                .filter(|id| playing.contains(id))
                .and_then(|id| {
                    let player = world.players.get(&id)?;
                    let wanted = self.wanted.get(&id)?;
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
                if let Some(door) = safe_exit_with(car, &world.collision, |_| true) {
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
                    .and_then(|id| world.vehicles.get(&id.0))
                    .map_or(player.state.motor.position.0, |car| {
                        car.state.motor.position.0
                    })
                    .map(|v| v.0);
                let visible = sight(&world.collision, p, actual, 38.0);
                goal = if visible { actual } else { wanted.last_known };
                if visible {
                    let recent = |t: Option<u32>| t.is_some_and(|t| tick.saturating_sub(t) < 150);
                    let authorized = wanted.fire_authorized;
                    let aiming_at_officer = player.weapon.aiming
                        && wrap(player.state.motor.yaw.0 - yaw_to(actual, p)).abs() <= 0.45;
                    let resisting = aiming_at_officer || recent(wanted.last_gunfire);
                    let protected = player
                        .lifecycle
                        .protected_until
                        .is_some_and(|end| tick < end);
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
                            self.wanted
                                .get_mut(&target_id)
                                .expect("active pursuit")
                                .fire_authorized = true;
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
                let occupied = world.players[&target_id].state.vehicle_id.map(|id| id.0);
                let clear_fire_lane = |position: [f32; 3]| {
                    let origin = [position[0], position[1] + 1.45, position[2]];
                    let delta = std::array::from_fn::<_, 3, _>(|i| threat[i] - origin[i]);
                    let length = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
                    let direction = delta.map(|v| v / length.max(0.01));
                    occupied_positions.iter().all(|(&other, &point)| {
                        other == id
                            || sim_core::collision::ray_character(origin, direction, point, length)
                                .is_none()
                    }) && world.vehicles.iter().all(|(&id, vehicle)| {
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
                                && world
                                    .collision
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
            if walk && !sight(&world.collision, p, goal, 35.0) {
                let end = self.sidewalk.nearest(goal);
                if tick >= officer.next_plan
                    && route_budget > 0
                    && (officer.route.is_empty() || officer.route_goal != end)
                {
                    route_budget -= 1;
                    officer.next_plan = tick.saturating_add(30);
                    officer.route_goal = end;
                    officer.route = self
                        .sidewalk
                        .nearest(p)
                        .zip(end)
                        .map_or_else(VecDeque::new, |(start, end)| self.sidewalk.path(start, end));
                }
                while officer
                    .route
                    .front()
                    .is_some_and(|node| distance(p, self.sidewalk.points[node]) < 1.1)
                {
                    officer.route.pop_front();
                }
                if let Some(node) = officer.route.front() {
                    direction_goal = self.sidewalk.points[node];
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
                    collision: &world.collision,
                    config: &world.config,
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
            officer.weapon.advance(tick);
            if officer.weapon.magazine == 0 {
                officer.weapon.reload(tick);
            }
            let origin = [p[0], p[1] + 1.45, p[2]];
            let pitch = (look[1] - origin[1])
                .atan2(distance(origin, look))
                .clamp(-1.2, 1.2);
            officer.state.equipment = Some(EquipmentView {
                weapon_id: officer.weapon.equipped,
                aiming: aim.is_some(),
                aim_pitch: Pitch(pitch),
                reload_end_tick: officer.weapon.reload_end,
                equip_end_tick: None,
            });
            let Some((target, _, may_fire)) = aim else {
                officer.aiming_since = None;
                continue;
            };
            let since = *officer.aiming_since.get_or_insert(tick);
            if !may_fire
                || tick.saturating_sub(since) < 12
                || officer.weapon.reload_end.is_some()
                || officer.weapon.magazine == 0
                || tick < officer.weapon.next_fire
                || tick < self.wanted[&target].next_officer_shot
                || !firing.insert(target)
            {
                continue;
            }
            self.wanted
                .get_mut(&target)
                .expect("active pursuit")
                .next_officer_shot = tick + 12;
            let definition = officer.weapon.definition();
            officer.weapon.shot_sequence += 1;
            officer.weapon.magazine -= 1;
            officer.weapon.next_fire = tick.saturating_add(
                definition.fire_ticks.max(12)
                    + if officer.weapon.shot_sequence.is_multiple_of(3) {
                        24
                    } else {
                        0
                    },
            );
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
                sequence: officer.weapon.shot_sequence,
            });
        }
        shots
    }
}
