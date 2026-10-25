//! Server observations, private wanted state and bounded road-following police.
use crate::{
    population::{Navigation, Observer, hidden},
    vehicles::Vehicle,
    world::{SessionPhase, World},
};
use protocol::*;
use sim_core::{CollisionQueries, CollisionWorld, QueryFilter};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::result::Result;

pub const POLICE_CAP: usize = 8;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CrimeId {
    Shot(u32),
    Impact(u32, u32),
    Theft(u32, u32),
}
#[derive(Clone, Copy)]
pub(crate) enum CrimeKind {
    Theft,
    Shot,
    Harm,
    PoliceCar(u32),
}
pub struct Wanted {
    pub heat: u8,
    pub pursuit: Pursuit,
    pub last_known: [f32; 3],
    pub last_seen: u32,
    search_since: Option<u32>,
    last_increase: Option<u32>,
    arrest_ticks: u32,
    pub(crate) arrest_pending: bool,
    seen: VecDeque<CrimeId>,
}
impl Default for Wanted {
    fn default() -> Self {
        Self {
            heat: 0,
            pursuit: Pursuit::Clear,
            last_known: [0.0; 3],
            last_seen: 0,
            search_since: None,
            last_increase: None,
            arrest_ticks: 0,
            arrest_pending: false,
            seen: VecDeque::new(),
        }
    }
}
impl Wanted {
    fn observe(&mut self, event: CrimeId, tick: u32, position: [f32; 3]) {
        if self.seen.contains(&event) {
            return;
        }
        self.seen.push_back(event);
        if self.seen.len() > 64 {
            self.seen.pop_front();
        }
        if self
            .last_increase
            .is_none_or(|t| tick.saturating_sub(t) >= 30)
        {
            self.heat = (self.heat + 1).min(3);
            self.last_increase = Some(tick);
        }
        self.last_seen = tick;
        self.last_known = position;
        self.search_since = None;
        self.pursuit = Pursuit::Pursuing;
    }
    fn advance(&mut self, tick: u32, sight: Option<[f32; 3]>, arrest: bool) -> bool {
        if self.arrest_pending {
            return true;
        }
        if self.heat == 0 {
            return false;
        }
        if let Some(position) = sight {
            self.last_seen = tick;
            self.last_known = position;
            self.search_since = None;
            self.pursuit = Pursuit::Pursuing;
        } else if tick.saturating_sub(self.last_seen) >= 150 && self.search_since.is_none() {
            self.search_since = Some(tick);
            self.pursuit = Pursuit::Searching;
        }
        if self
            .search_since
            .is_some_and(|t| tick.saturating_sub(t) >= 600)
        {
            self.heat -= 1;
            self.search_since = Some(tick);
            if self.heat == 0 {
                self.pursuit = Pursuit::Clear;
                self.search_since = None;
            }
        }
        if arrest && self.heat > 0 {
            self.arrest_ticks = self.arrest_ticks.saturating_add(1);
            self.pursuit = Pursuit::Arresting;
        } else {
            self.arrest_ticks = 0;
            // Arresting is transient, not a state that survives failed conditions.
            if self.pursuit == Pursuit::Arresting {
                self.pursuit = if self.search_since.is_some() {
                    Pursuit::Searching
                } else {
                    Pursuit::Pursuing
                };
            }
        }
        self.arrest_ticks >= 90
    }
}
pub struct Unit {
    pub target: Option<u32>,
    home: [f32; 3],
    route: VecDeque<u32>,
    input: InputFrame,
    target_speed: f32,
    next_plan: u32,
    progress_at: u32,
    progress_position: [f32; 3],
    wreck_until: Option<u32>,
    patrol_leg: usize,
}
pub struct Police {
    pub units: BTreeMap<u32, Unit>,
    pub(crate) secured: BTreeSet<u32>,
    road: Navigation,
    enabled: bool,
    initialized: bool,
    cursor: usize,
    crimes: BTreeMap<(u32, CrimeId), [f32; 3]>,
}
impl Police {
    pub fn new(manifest: &content::WorldManifest) -> Self {
        Self {
            units: BTreeMap::new(),
            secured: BTreeSet::new(),
            road: Navigation::new(&manifest.road_graph, false),
            // The small T01–T11 map remains a regression fixture without police.
            enabled: manifest.districts.len() == 6,
            initialized: false,
            cursor: 0,
            crimes: BTreeMap::new(),
        }
    }
    pub fn release(&mut self, actor: u32) {
        for unit in self.units.values_mut().filter(|u| u.target == Some(actor)) {
            unit.target = None;
            unit.route.clear();
            unit.next_plan = 0;
        }
        self.crimes.retain(|(id, _), _| *id != actor);
    }
    pub(crate) fn vehicle_input(&self, id: u32, tick: u32, speed: f32) -> Option<InputFrame> {
        self.units.get(&id).map(|u| {
            let mut input = u.input.clone();
            input.tick = tick;
            input.move_z = InputAxis(
                ((u.target_speed - speed) * 2.0)
                    .clamp(-1.0, 1.0)
                    .mul_add(32767.0, 0.0) as i16,
            );
            input
        })
    }
}
pub(crate) fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}
fn yaw_to(a: [f32; 3], b: [f32; 3]) -> f32 {
    (-(b[0] - a[0])).atan2(-(b[2] - a[2]))
}
fn wrap(yaw: f32) -> f32 {
    (yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}
fn sight(collision: &CollisionWorld, from: [f32; 3], to: [f32; 3], limit: f32) -> bool {
    if distance(from, to) > limit {
        return false;
    }
    let a = [from[0], from[1] + 1.1, from[2]];
    let b = [to[0], to[1] + 1.0, to[2]];
    let delta: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let length = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
    length < 0.01
        || collision
            .raycast(a, delta, length, QueryFilter::VISION)
            .is_ok_and(|hit| hit.is_none_or(|h| h.distance >= length - 0.1))
}
impl World {
    /// Called only by successful authoritative actions/damage. Multiple stimuli from
    /// one causal event share an ID and produce at most one wanted observation.
    pub(crate) fn report_crime(
        &mut self,
        actor: u32,
        cause: CrimeId,
        kind: CrimeKind,
        at: [f32; 3],
    ) {
        let witnessed = self.police.units.keys().any(|id| {
            let car = &self.vehicles[id];
            // A police car struck by a fatal hit still reports that causal hit.
            if matches!(kind, CrimeKind::PoliceCar(target) if target == *id) {
                return true;
            }
            if car.state.health.0 == 0 {
                return false;
            }
            let p = car.state.motor.position.0.map(|v| v.0);
            match kind {
                CrimeKind::Shot => distance(p, at) <= 80.0,
                CrimeKind::Theft => sight(&self.collision, p, at, 50.0),
                CrimeKind::Harm => sight(&self.collision, p, at, 60.0),
                CrimeKind::PoliceCar(_) => false,
            }
        });
        if witnessed
            && self.police.crimes.len() < 256
            && let Some(player) = self.players.get(&actor)
        {
            self.police
                .crimes
                .entry((actor, cause))
                .or_insert(player.state.motor.position.0.map(|v| v.0));
        }
    }
    pub(crate) fn direct_police(
        &mut self,
        tick: u32,
        observers: &[Observer],
    ) -> Result<(), String> {
        if !self.police.enabled {
            return Ok(());
        }
        let initializing = !self.police.initialized;
        self.police.initialized = true;
        let wanted: Vec<_> = self
            .sessions
            .values()
            .filter(|s| s.phase == SessionPhase::Playing)
            .filter_map(|s| s.player.map(|id| (id.0, s.wanted.heat)))
            .filter(|(id, heat)| {
                *heat > 0
                    && matches!(
                        self.players[id].state.mode,
                        CharacterMode::OnFoot | CharacterMode::Driving
                    )
            })
            .collect();
        // Rotate the first recipient every five seconds only when demand exceeds
        // supply. Allocate one unit per player before assigning second/third units.
        if tick.is_multiple_of(150) && !wanted.is_empty() {
            self.police.cursor = (self.police.cursor + 1) % wanted.len();
        }
        let mut assignments = Vec::new();
        for level in 0..3 {
            for offset in 0..wanted.len() {
                let (id, heat) = wanted[(offset + self.police.cursor) % wanted.len()];
                if heat > level && assignments.len() < POLICE_CAP {
                    assignments.push(id);
                }
            }
        }
        let desired = assignments.len().max(2);
        let mut remove = Vec::new();
        for (&id, u) in &mut self.police.units {
            let car = &self.vehicles[&id];
            let p = car.state.motor.position.0.map(|v| v.0);
            if distance(p, u.progress_position) >= 2.0 {
                u.progress_position = p;
                u.progress_at = tick;
            }
            if car.state.health.0 == 0 {
                let until = *u.wreck_until.get_or_insert(tick.saturating_add(90));
                if tick >= until && hidden(p, observers, &self.collision) {
                    remove.push(id);
                }
            } else if tick.saturating_sub(u.progress_at) >= 450
                && hidden(p, observers, &self.collision)
            {
                remove.push(id);
            }
        }
        for id in remove {
            self.police.units.remove(&id);
            self.vehicles.remove(&id);
            self.combat.disabled.remove(&id);
            self.combat.homes.remove(&id);
        }
        if initializing || tick.is_multiple_of(30) {
            let template = self
                .content
                .manifest
                .vehicle_templates
                .iter()
                .find(|t| t.id == "compact")
                .ok_or("police compact template missing")?
                .clone();
            while self.police.units.len() < desired {
                let index = self.police.units.len();
                // Replacing a retired Quay unit must not select Civic merely
                // because one Civic unit remains. Balance actual home assignments.
                let homes = [[-192.0, 0.0, 160.0], [0.0, 0.0, -160.0]];
                let home = *homes
                    .iter()
                    .min_by_key(|home| {
                        self.police
                            .units
                            .values()
                            .filter(|unit| unit.home == **home)
                            .count()
                    })
                    .expect("two patrol homes");
                let near = assignments
                    .get(index)
                    .and_then(|id| {
                        self.sessions
                            .values()
                            .find(|s| s.player == Some(EntityId(*id)))
                    })
                    .map_or(home, |s| s.wanted.last_known);
                let mut candidates = self.police.road.candidates.clone();
                candidates.sort_by(|a, b| distance(a.2, near).total_cmp(&distance(b.2, near)));
                let spot = candidates.into_iter().find(|(_, _, p, yaw)| {
                    (initializing
                        || (observers.iter().all(|o| distance(*p, o.position) >= 40.0)
                            && hidden(*p, observers, &self.collision)))
                        && distance(*p, near) < 180.0
                        && self
                            .vehicles
                            .values()
                            .all(|v| distance(*p, v.state.motor.position.0.map(|v| v.0)) > 12.0)
                        && !self
                            .collision
                            .overlaps_vehicle(*p, *yaw, template.dimensions.map(|v| v as f32))
                            .unwrap_or(true)
                });
                let Some((_, to, p, yaw)) = spot else {
                    break;
                };
                let id = self.next_entity;
                self.next_entity = id.checked_add(1).ok_or("police entity IDs exhausted")?;
                self.vehicles.insert(
                    id,
                    Vehicle {
                        id: EntityId(id),
                        template: template.clone(),
                        idle_ticks: 0,
                        state: VehicleState {
                            motor: sim_core::spawn_vehicle(p, yaw)?,
                            health: VehicleHealth(template.health),
                            driver_id: None,
                            disabled: false,
                        },
                    },
                );
                self.police.units.insert(
                    id,
                    Unit {
                        target: None,
                        home,
                        route: VecDeque::from([to]),
                        input: sim_core::neutral_input(tick, Yaw(yaw)),
                        target_speed: 0.0,
                        next_plan: tick,
                        progress_at: tick,
                        progress_position: p,
                        wreck_until: None,
                        patrol_leg: 0,
                    },
                );
            }
        }
        // Retire surplus units only outside every player's static clear view.
        let surplus: Vec<_> = self
            .police
            .units
            .keys()
            .copied()
            .skip(desired)
            .filter(|id| {
                hidden(
                    self.vehicles[id].state.motor.position.0.map(|v| v.0),
                    observers,
                    &self.collision,
                )
            })
            .collect();
        for id in surplus {
            self.police.units.remove(&id);
            self.vehicles.remove(&id);
            self.combat.disabled.remove(&id);
            self.combat.homes.remove(&id);
        }
        for (index, unit) in self.police.units.values_mut().enumerate() {
            let target = assignments.get(index).copied();
            if target != unit.target {
                unit.target = target;
                unit.route.clear();
                unit.next_plan = tick;
                unit.progress_at = tick;
            }
        }
        Ok(())
    }
    pub(crate) fn decide_police(
        &mut self,
        tick: u32,
        observers: &[Observer],
    ) -> Result<(), String> {
        let targets: BTreeMap<_, _> = self
            .sessions
            .values()
            .filter_map(|s| s.player.map(|id| (id.0, s.wanted.last_known)))
            .collect();
        for (&id, unit) in &mut self.police.units {
            let car = &self.vehicles[&id];
            let p = car.state.motor.position.0.map(|v| v.0);
            let planning = tick >= unit.next_plan;
            if car.state.health.0 == 0 || (!planning && !tick.is_multiple_of(6)) {
                continue;
            }
            while unit
                .route
                .front()
                .is_some_and(|n| distance(p, self.police.road.points[n]) < 5.0)
            {
                unit.route.pop_front();
            }
            let known = unit.target.and_then(|id| targets.get(&id)).copied();
            self.collision.exclude_vehicle(Some(id));
            let direct = known
                .filter(|target| distance(p, *target) <= 15.0)
                .filter(|target| {
                    let d = distance(p, *target);
                    let stop = car.template.dimensions[2] as f32 / 2.0 + 2.8;
                    let delta = [
                        (target[0] - p[0]) * (1.0 - stop / d.max(stop)),
                        0.0,
                        (target[2] - p[2]) * (1.0 - stop / d.max(stop)),
                    ];
                    self.collision
                        .cast_vehicle(
                            p,
                            yaw_to(p, *target),
                            car.template.dimensions.map(|v| v as f32),
                            delta,
                        )
                        .is_ok_and(|cast| {
                            cast.contacts.is_empty()
                                && cast
                                    .applied
                                    .iter()
                                    .zip(delta)
                                    .all(|(a, b)| (a - b).abs() < 0.05)
                        })
                });
            if planning && direct.is_none() {
                // Up to eight jobs per planning turn; each unit requests at most 2 Hz.
                let goal = known.or_else(|| {
                    let legs = [
                        [-48.0, 0.0, -48.0],
                        [48.0, 0.0, -48.0],
                        [48.0, 0.0, 48.0],
                        [-48.0, 0.0, 48.0],
                    ];
                    let delta = legs[unit.patrol_leg % legs.len()];
                    if unit.route.is_empty() {
                        unit.patrol_leg += 1;
                    }
                    Some([unit.home[0] + delta[0], 0.0, unit.home[2] + delta[2]])
                });
                let start = unit
                    .route
                    .front()
                    .copied()
                    .or_else(|| self.police.road.nearest(p));
                let end = goal.and_then(|g| self.police.road.nearest(g));
                unit.route = start
                    .zip(end)
                    .map_or_else(VecDeque::new, |(a, b)| self.police.road.path(a, b));
            }
            if planning {
                unit.next_plan = tick.saturating_add(15);
            }
            if !tick.is_multiple_of(6) {
                continue;
            }
            let target = direct.or_else(|| unit.route.front().map(|n| self.police.road.points[n]));
            let mut input = sim_core::neutral_input(tick, car.state.motor.yaw);
            let mut speed = 0.0;
            if let Some(target) = target {
                let error = wrap(yaw_to(p, target) - car.state.motor.yaw.0);
                input.move_x =
                    InputAxis((-error * 1.8).clamp(-1.0, 1.0).mul_add(32767.0, 0.0) as i16);
                speed = if error.abs() > 0.2 || distance(p, target) < 25.0 {
                    4.0
                } else {
                    12.0
                };
                if direct.is_some() {
                    speed = ((distance(p, target) - car.template.dimensions[2] as f32 / 2.0 - 2.8)
                        * 1.3)
                        .clamp(0.0, 8.0);
                }
                if error.abs() > 1.5 {
                    speed = speed.min(2.0);
                }
                let forward = [
                    -car.state.motor.yaw.0.sin(),
                    0.0,
                    -car.state.motor.yaw.0.cos(),
                ];
                let sensor = (car.state.motor.speed.0.powi(2) / (2.0 * car.template.brake as f32)
                    + 0.1)
                    .max(0.2);
                let cast = self.collision.cast_vehicle(
                    p,
                    car.state.motor.yaw.0,
                    car.template.dimensions.map(|v| v as f32),
                    forward.map(|v| v * sensor),
                )?;
                if !cast.contacts.is_empty() {
                    speed = 0.0;
                }
                for actor in observers.iter().map(|o| o.position).chain(
                    self.population
                        .pedestrians
                        .values()
                        .map(|p| p.state.motor.position.0.map(|v| v.0)),
                ) {
                    let delta = [actor[0] - p[0], actor[2] - p[2]];
                    let along = delta[0] * forward[0] + delta[1] * forward[2];
                    let across = (delta[0] * forward[2] - delta[1] * forward[0]).abs();
                    if along > 0.0
                        && along < car.template.dimensions[2] as f32 / 2.0 + sensor
                        && across < car.template.dimensions[0] as f32 / 2.0 + 0.6
                    {
                        speed = 0.0;
                    }
                }
            }
            if speed <= 0.1 {
                input.buttons = Buttons(4);
            }
            unit.target_speed = speed;
            unit.input = input;
        }
        self.collision.exclude_vehicle(None);
        Ok(())
    }
    pub(crate) fn advance_police(&mut self) -> Result<(), String> {
        let crimes = std::mem::take(&mut self.police.crimes);
        for peer in self.sessions.keys().copied().collect::<Vec<_>>() {
            let mut s = self.sessions.remove(&peer).expect("police session");
            if let Some(id) = s.player {
                let player = &self.players[&id.0];
                if matches!(
                    player.state.mode,
                    CharacterMode::Downed | CharacterMode::Arrested
                ) || player.state.health.0 == 0
                {
                    if s.wanted.heat > 0 {
                        s.wanted = Wanted::default();
                        s.status_dirty = true;
                    }
                    self.police.release(id.0);
                } else {
                    let before = (s.wanted.heat, s.wanted.pursuit);
                    for (&(actor, cause), &at) in &crimes {
                        if actor == id.0 {
                            s.wanted.observe(cause, self.tick, at);
                        }
                    }
                    let p = player
                        .state
                        .vehicle_id
                        .map(|id| self.vehicles[&id.0].state.motor.position.0.map(|v| v.0))
                        .unwrap_or_else(|| player.state.motor.position.0.map(|v| v.0));
                    let seen = self.tick.is_multiple_of(6)
                        && self.police.units.keys().any(|unit| {
                            let car = &self.vehicles[unit];
                            car.state.health.0 > 0
                                && sight(
                                    &self.collision,
                                    car.state.motor.position.0.map(|v| v.0),
                                    p,
                                    70.0,
                                )
                        });
                    let speed = player
                        .state
                        .vehicle_id
                        .map(|id| self.vehicles[&id.0].state.motor.speed.0.abs())
                        .unwrap_or_else(|| {
                            player
                                .state
                                .motor
                                .velocity
                                .0
                                .iter()
                                .map(|v| v.0 * v.0)
                                .sum::<f32>()
                                .sqrt()
                        });
                    let arrest = s.phase == SessionPhase::Playing
                        && speed < 0.5
                        && self.police.units.iter().any(|(unit, u)| {
                            let car = &self.vehicles[unit];
                            let front = sim_core::anchor(
                                &car.state.motor,
                                [0.0, 0.0, -car.template.dimensions[2] / 2.0],
                            );
                            u.target == Some(id.0)
                                && car.state.health.0 > 0
                                && distance(front, p) <= 3.0
                                && sight(&self.collision, front, p, 3.0)
                        });
                    if s.wanted.advance(self.tick, seen.then_some(p), arrest) {
                        self.arrest_player(peer, &mut s)?;
                    }
                    s.status_dirty |= before != (s.wanted.heat, s.wanted.pursuit);
                }
            }
            self.sessions.insert(peer, s);
        }
        Ok(())
    }
    fn arrest_player(
        &mut self,
        peer: crate::transport::PeerKey,
        s: &mut crate::world::Session,
    ) -> Result<(), String> {
        let id = s.player.ok_or("arrest owner missing")?;
        let car_id = self.players[&id.0].state.vehicle_id;
        let mut position = Some(self.players[&id.0].state.motor.position.0.map(|v| v.0));
        if let Some(car) = car_id {
            self.collision.exclude_vehicle(Some(car.0));
            position =
                crate::vehicles::safe_exit_with(&self.vehicles[&car.0], &self.collision, |p| {
                    self.clear_spawn(p, Some(id.0))
                });
            self.collision.exclude_vehicle(None);
            position = position.or_else(|| {
                self.safe_spawn(
                    self.vehicles[&car.0].state.motor.position.0.map(|v| v.0),
                    Some(id.0),
                )
            });
        }
        let Some(position) = position else {
            // Freeze pending arrest and retry safe ejection; never materialize in a wall.
            if let Some(car) = car_id {
                self.vehicles.get_mut(&car.0).expect("arrest car").stop();
            }
            s.inputs.clear();
            if !s.wanted.arrest_pending {
                s.wanted.arrest_pending = true;
                s.weapon.reload_end = None;
                s.weapon.protected_until = None;
                if let Some(m) = &mut s.mission
                    && m.phase == crate::missions::Phase::Active
                {
                    m.fail(self.tick);
                }
                s.status_dirty = true;
                let affected = [id.0, car_id.expect("blocked driving exit").0];
                self.transition(peer, s, &affected)?;
                self.reliable(
                    peer,
                    s.id.as_deref(),
                    ServerMessage::ControlChanged(self.owner(s)?),
                );
            }
            return Ok(());
        };
        if let Some(car) = car_id {
            let car = self.vehicles.get_mut(&car.0).expect("arrest car");
            car.state.driver_id = None;
            car.stop();
        }
        let player = self.players.get_mut(&id.0).expect("arrest player");
        player.state.mode = CharacterMode::Arrested;
        player.state.vehicle_id = None;
        player.state.motor = sim_core::spawn_character(position, 0.0)?;
        s.weapon.reload_end = None;
        s.weapon.protected_until = None;
        s.weapon.respawn = Some(self.tick.saturating_add(150));
        s.wanted = Wanted::default();
        self.police.release(id.0);
        if let Some(m) = &mut s.mission
            && m.phase == crate::missions::Phase::Active
        {
            m.fail(self.tick);
        }
        s.status_dirty = true;
        let mut affected = vec![id.0];
        if let Some(car) = car_id {
            affected.push(car.0);
        }
        self.transition(peer, s, &affected)?;
        self.reliable(
            peer,
            s.id.as_deref(),
            ServerMessage::ControlChanged(self.owner(s)?),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn causal_dedup_cooldown_search_reacquisition_and_decay_are_exact() {
        let mut w = Wanted::default();
        let a = [10.0, 0.0, 20.0];
        w.observe(CrimeId::Shot(1), 0, a);
        w.observe(CrimeId::Shot(1), 30, [99.0; 3]);
        assert_eq!((w.heat, w.last_seen, w.last_known), (1, 0, a));
        w.observe(CrimeId::Shot(2), 29, a);
        assert_eq!((w.heat, w.last_seen), (1, 29));
        w.observe(CrimeId::Shot(3), 30, a);
        w.observe(CrimeId::Shot(4), 60, a);
        w.observe(CrimeId::Shot(5), 90, a);
        assert_eq!(w.heat, 3);
        w.advance(239, None, false);
        assert_eq!(w.pursuit, Pursuit::Pursuing);
        w.advance(240, None, false);
        assert_eq!(w.pursuit, Pursuit::Searching);
        w.advance(839, None, false);
        assert_eq!(w.heat, 3);
        w.advance(840, None, false);
        assert_eq!(w.heat, 2);
        w.advance(900, Some(a), false);
        assert_eq!(w.pursuit, Pursuit::Pursuing);
        w.advance(1050, None, false);
        w.advance(1650, None, false);
        assert_eq!(w.heat, 1);
        w.advance(2250, None, false);
        assert_eq!((w.heat, w.pursuit), (0, Pursuit::Clear));
        assert!(w.seen.len() <= 64);
    }
    #[test]
    fn arrest_requires_ninety_continuous_ticks_and_resets_on_any_invalid_tick() {
        let mut w = Wanted::default();
        w.observe(CrimeId::Shot(1), 0, [0.0; 3]);
        for tick in 1..90 {
            assert!(!w.advance(tick, Some([0.0; 3]), true));
        }
        assert_eq!(w.pursuit, Pursuit::Arresting);
        assert!(!w.advance(90, None, false));
        assert_eq!(w.arrest_ticks, 0);
        assert_eq!(w.pursuit, Pursuit::Pursuing);
        for tick in 91..180 {
            assert!(!w.advance(tick, None, true));
        }
        assert!(w.advance(180, None, true));
    }
    fn city() -> World {
        World::new(
            crate::content_loader::WorldContent::load_index(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../apps/client/public/content"),
                "city.json",
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn retired_patrol_replacement_preserves_both_home_districts() {
        let mut w = city();
        for cycle in 0..6 {
            let home = if cycle % 2 == 0 {
                [-192.0, 0.0, 160.0]
            } else {
                [0.0, 0.0, -160.0]
            };
            let id = *w
                .police
                .units
                .iter()
                .find(|(_, u)| u.home == home)
                .unwrap()
                .0;
            w.police.units.remove(&id);
            w.vehicles.remove(&id);
            w.collision
                .set_vehicle_proxies(&w.vehicles.values().map(Vehicle::proxy).collect::<Vec<_>>())
                .unwrap();
            w.direct_police((cycle + 1) * 30, &[]).unwrap();
            assert_eq!(w.police.units.len(), 2);
            assert_eq!(
                w.police
                    .units
                    .values()
                    .filter(|u| u.home == [-192.0, 0.0, 160.0])
                    .count(),
                1,
                "Quay must receive its replacement"
            );
            assert_eq!(
                w.police
                    .units
                    .values()
                    .filter(|u| u.home == [0.0, 0.0, -160.0])
                    .count(),
                1,
                "Civic must receive its replacement"
            );
        }
    }
    #[test]
    fn patrols_use_shared_motor_and_remain_collision_safe_without_visible_recycling() {
        let mut w = city();
        assert_eq!(w.police.units.len(), 2);
        let initial: BTreeMap<_, _> = w
            .police
            .units
            .keys()
            .map(|id| (*id, w.vehicles[id].state.motor.position.0.map(|v| v.0)))
            .collect();
        let mut travelled = BTreeMap::<u32, f32>::new();
        for tick in 1..=900 {
            w.collision
                .set_vehicle_proxies(&w.vehicles.values().map(Vehicle::proxy).collect::<Vec<_>>())
                .unwrap();
            // Observers at each unit prevent the stuck-retirement branch from hiding failures.
            let observers: Vec<_> = w
                .police
                .units
                .keys()
                .map(|id| Observer {
                    position: w.vehicles[id].state.motor.position.0.map(|v| v.0),
                    yaw: 0.0,
                })
                .collect();
            w.direct_police(tick, &observers).unwrap();
            w.decide_police(tick, &[]).unwrap();
            for id in w.police.units.keys().copied().collect::<Vec<_>>() {
                let car = w.vehicles.get_mut(&id).unwrap();
                let before = car.state.motor.position.0.map(|v| v.0);
                let input = w
                    .police
                    .vehicle_input(id, tick, car.state.motor.speed.0)
                    .unwrap();
                w.collision.exclude_vehicle(Some(id));
                car.state.motor = sim_core::step_vehicle(
                    &car.state.motor,
                    &input,
                    &car.template,
                    &sim_core::StepContext {
                        tick,
                        collision: &w.collision,
                        config: &sim_core::SimConfig::from_content(&w.content.config).unwrap(),
                    },
                )
                .unwrap()
                .motor;
                assert!(
                    !w.collision
                        .overlaps_vehicle(
                            car.state.motor.position.0.map(|v| v.0),
                            car.state.motor.yaw.0,
                            car.template.dimensions.map(|v| v as f32)
                        )
                        .unwrap()
                );
                assert!(car.state.motor.speed.0.abs() <= 12.01);
                *travelled.entry(id).or_default() +=
                    distance(before, car.state.motor.position.0.map(|v| v.0));
                w.collision
                    .set_vehicle_proxies(
                        &w.vehicles.values().map(Vehicle::proxy).collect::<Vec<_>>(),
                    )
                    .unwrap();
            }
            w.collision.exclude_vehicle(None);
        }
        assert_eq!(
            w.police.units.keys().copied().collect::<Vec<_>>(),
            initial.keys().copied().collect::<Vec<_>>()
        );
        println!("patrol distance over 900 ticks: {travelled:?}");
        assert!(travelled.values().all(|d| *d > 40.0));
    }
    #[test]
    fn witnesses_distinguish_acoustics_static_occlusion_range_and_fatal_police_hits() {
        let mut w = city();
        let id = *w.police.units.keys().next().unwrap();
        // The ray crosses a real building; a sound still reaches the same police car.
        let collider = w
            .content
            .collision
            .colliders
            .iter()
            .find(|c| c.flags & 2 != 0 && c.half_extents[1] > 2.0)
            .unwrap();
        let center = collider.center.map(|v| v as f32);
        let left = [
            center[0] - collider.half_extents[0] as f32 - 2.0,
            0.0,
            center[2],
        ];
        let right = [
            center[0] + collider.half_extents[0] as f32 + 2.0,
            0.0,
            center[2],
        ];
        assert!(!sight(&w.collision, left, right, 60.0));
        assert!(distance(left, right) < 80.0);
        w.vehicles.get_mut(&id).unwrap().state.motor = sim_core::spawn_vehicle(left, 0.0).unwrap();
        // Report needs an actor in EntityStore, but never trusts a client crime message.
        let player = 999;
        w.players.insert(
            player,
            crate::world::Player {
                id: EntityId(player),
                display_name: DisplayName("fixture".into()),
                slot: 0,
                state: CharacterState {
                    motor: sim_core::spawn_character(right, 0.0).unwrap(),
                    health: CharacterHealth(100),
                    mode: CharacterMode::OnFoot,
                    vehicle_id: None,
                },
            },
        );
        let others: Vec<_> = w
            .police
            .units
            .keys()
            .copied()
            .filter(|u| *u != id)
            .collect();
        for other in others {
            w.vehicles.get_mut(&other).unwrap().state.health = VehicleHealth(0);
        }
        w.report_crime(player, CrimeId::Theft(1, 1), CrimeKind::Theft, right);
        w.report_crime(player, CrimeId::Impact(1, 1), CrimeKind::Harm, right);
        assert!(w.police.crimes.is_empty());
        w.report_crime(player, CrimeId::Shot(1), CrimeKind::Shot, right);
        w.report_crime(player, CrimeId::Shot(1), CrimeKind::Shot, right);
        assert_eq!(w.police.crimes.len(), 1);
        w.police.crimes.clear();
        w.report_crime(
            player,
            CrimeId::Shot(2),
            CrimeKind::Shot,
            [left[0] + 81.0, 0.0, left[2]],
        );
        assert!(w.police.crimes.is_empty());
        w.vehicles.get_mut(&id).unwrap().state.health = VehicleHealth(0);
        w.report_crime(
            player,
            CrimeId::Shot(3),
            CrimeKind::PoliceCar(id),
            [left[0] + 1.0, left[1], left[2]],
        );
        assert_eq!(w.police.crimes.len(), 1);
    }
}
