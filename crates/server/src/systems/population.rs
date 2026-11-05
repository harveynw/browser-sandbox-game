//! Bounded, deterministic ambient population. Only World commits motor results.
use crate::entities::vehicle::Vehicle;
use content::{EdgeKind, RoadGraph, WorldManifest};
use protocol::*;
use sim_core::{CollisionQueries, CollisionWorld, QueryFilter, SimConfig, StepContext};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::result::Result;

pub const PARKED_CAP: usize = 24;
pub const RECOVERY_RESERVED: usize = 8;
pub const TRAFFIC_CAP: usize = 24;
pub const PEDESTRIAN_CAP: usize = 32;
const PATHS_PER_TICK: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrafficPhase {
    Driving,
    Yielding,
    Stopped,
    Parking,
    Available,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PedestrianPhase {
    Walking,
    Waiting,
    Fleeing,
    Incapacitated,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VehiclePool {
    Parked,
    Traffic,
}
#[derive(Clone, Copy)]
pub struct Observer {
    pub position: [f32; 3],
    pub yaw: f32,
}
pub struct Traffic {
    pub phase: TrafficPhase,
    route: VecDeque<u32>,
    input: InputFrame,
    target_speed: f32,
    next_plan: u32,
    progress_at: u32,
    progress_position: [f32; 3],
    far_since: Option<u32>,
    parking: Option<usize>,
    born: u32,
}
use crate::entities::pedestrian::Pedestrian;
#[derive(Default, Debug)]
pub struct PopulationMetrics {
    pub spawned: u64,
    pub despawned: u64,
    pub path_jobs: u64,
    pub max_path_jobs_per_tick: usize,
    pub replans: u64,
    pub blocked_spawns: u64,
}
pub(crate) struct Navigation {
    pub(crate) points: BTreeMap<u32, [f32; 3]>,
    edges: BTreeMap<u32, Vec<(u32, f32, EdgeKind)>>,
    pub(crate) candidates: Vec<(u32, u32, [f32; 3], f32)>,
}
impl Navigation {
    pub(crate) fn new(graph: &RoadGraph, walking: bool) -> Self {
        let points: BTreeMap<_, _> = graph
            .nodes
            .iter()
            .map(|n| (n.id, n.position.map(|v| v as f32)))
            .collect();
        let mut edges: BTreeMap<_, Vec<_>> = BTreeMap::new();
        let mut candidates = Vec::new();
        for e in &graph.edges {
            if e.kind == EdgeKind::Driveway || (!walking && e.kind != EdgeKind::Lane) {
                continue;
            }
            let a = points[&e.from];
            let b = points[&e.to];
            edges
                .entry(e.from)
                .or_default()
                .push((e.to, e.speed_limit as f32, e.kind));
            // Multiple spaced candidates on long segments avoid junction spawning.
            let length = distance(a, b);
            if length > 24.0 && (!walking || e.kind == EdgeKind::Sidewalk) {
                let count = (length / 24.0) as u32;
                for i in 0..count {
                    let t = (i as f32 + 0.5) / count as f32;
                    let p = std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t);
                    candidates.push((e.from, e.to, p, yaw_to(a, b)));
                }
            }
        }
        Self {
            points,
            edges,
            candidates,
        }
    }
    pub(crate) fn nearest(&self, p: [f32; 3]) -> Option<u32> {
        self.points
            .keys()
            .filter(|id| self.edges.contains_key(id))
            .min_by(|a, b| {
                distance(p, self.points[a])
                    .total_cmp(&distance(p, self.points[b]))
                    .then(a.cmp(b))
            })
            .copied()
    }
    pub(crate) fn path(&self, from: u32, goal: u32) -> VecDeque<u32> {
        self.path_avoiding(from, goal, None)
    }

    pub(crate) fn path_avoiding(
        &self,
        from: u32,
        goal: u32,
        blocked: Option<u32>,
    ) -> VecDeque<u32> {
        let mut cost = BTreeMap::from([(from, 0.0_f32)]);
        let mut previous = BTreeMap::new();
        let mut visited = BTreeSet::new();
        while let Some((&id, &d)) = cost
            .iter()
            .filter(|(id, _)| !visited.contains(*id))
            .min_by(|a, b| a.1.total_cmp(b.1).then(a.0.cmp(b.0)))
        {
            if id == goal {
                let mut path = VecDeque::from([goal]);
                let mut current = goal;
                while current != from {
                    let Some(&p) = previous.get(&current) else {
                        return VecDeque::new();
                    };
                    current = p;
                    path.push_front(current);
                }
                return path;
            }
            visited.insert(id);
            for &(to, speed, _) in self.edges.get(&id).into_iter().flatten() {
                let penalty = if Some(to) == blocked { 20.0 } else { 0.0 };
                let next =
                    d + distance(self.points[&id], self.points[&to]) / speed.max(0.1) + penalty;
                if next < *cost.get(&to).unwrap_or(&f32::INFINITY) {
                    cost.insert(to, next);
                    previous.insert(to, id);
                }
            }
        }
        VecDeque::new()
    }
}
#[derive(Default)]
pub struct Intersections {
    reservations: BTreeMap<(i32, i32), (u32, u32)>,
}
impl Intersections {
    pub fn request(&mut self, key: (i32, i32), id: u32, tick: u32) -> bool {
        self.reservations.retain(|_, (_, expiry)| *expiry > tick);
        match self.reservations.get(&key) {
            Some((owner, _)) => *owner == id,
            None => {
                self.reservations
                    .insert(key, (id, tick.saturating_add(120)));
                true
            }
        }
    }
    fn release(&mut self, id: u32) {
        self.reservations.retain(|_, (owner, _)| *owner != id);
    }
}
pub struct PopulationSystem {
    pub traffic: BTreeMap<u32, Traffic>,
    pub pedestrians: BTreeMap<u32, Pedestrian>,
    pub pools: BTreeMap<u32, VehiclePool>,
    pub metrics: PopulationMetrics,
    pub intersections: Intersections,
    road: Navigation,
    sidewalk: Navigation,
    queue: VecDeque<u32>,
    rng: u64,
    car_cursor: usize,
    pedestrian_cursor: usize,
    walk_axis: f32,
}
impl PopulationSystem {
    pub fn new(
        manifest: &WorldManifest,
        config: &content::SimConfig,
        parked: impl Iterator<Item = u32>,
        seed: u64,
    ) -> Result<Self, String> {
        let pools: BTreeMap<_, _> = parked.map(|id| (id, VehiclePool::Parked)).collect();
        if pools.len() > PARKED_CAP - RECOVERY_RESERVED
            || manifest
                .parking
                .iter()
                .filter(|p| p.mission_reserved)
                .count()
                > RECOVERY_RESERVED
        {
            return Err("parked/recovery population budget exceeded".into());
        }
        Ok(Self {
            traffic: BTreeMap::new(),
            pedestrians: BTreeMap::new(),
            pools,
            metrics: PopulationMetrics::default(),
            intersections: Intersections::default(),
            road: Navigation::new(&manifest.road_graph, false),
            sidewalk: Navigation::new(&manifest.pedestrian_graph, true),
            queue: VecDeque::new(),
            rng: seed.max(1),
            car_cursor: 0,
            pedestrian_cursor: 0,
            walk_axis: 1.4 / config.character.walk_speed as f32,
        })
    }
    fn random(&mut self) -> usize {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng as usize
    }
    /// Only hidden actors are recycled. Possession never changes a pool tag.
    pub fn direct(
        &mut self,
        tick: u32,
        observers: &[Observer],
        vehicles: &mut BTreeMap<u32, Vehicle>,
        manifest: &WorldManifest,
        collision: &mut CollisionWorld,
        next_id: &mut u32,
    ) -> Result<(), String> {
        if !tick.is_multiple_of(30) {
            return Ok(());
        }
        let mut remove = Vec::new();
        for (&id, brain) in &mut self.traffic {
            let Some(car) = vehicles.get(&id) else {
                continue;
            };
            let p = car.state.motor.position.0.map(|v| v.0);
            if car.state.driver_id.is_some() || car.state.disabled || car.state.health.0 == 0 {
                brain.far_since = None;
                continue;
            }
            let far = aged_far(tick, p, observers, &mut brain.far_since);
            let stuck = tick.saturating_sub(brain.progress_at) >= 450
                && brain.phase != TrafficPhase::Available;
            if (far || stuck) && hidden(p, observers, collision) {
                remove.push(id);
            }
        }
        for id in remove {
            self.traffic.remove(&id);
            self.pools.remove(&id);
            vehicles.remove(&id);
            self.intersections.release(id);
            self.metrics.despawned += 1;
        }
        let mut remove = Vec::new();
        for (&id, ped) in &mut self.pedestrians {
            let p = ped.state.motor.position.0.map(|v| v.0);
            if ped.state.health.0 == 0 {
                continue;
            }
            let far = aged_far(tick, p, observers, &mut ped.far_since);
            let stuck = tick.saturating_sub(ped.progress_at) >= 450;
            if (far || stuck) && hidden(p, observers, collision) {
                remove.push(id);
            }
        }
        for id in remove {
            self.pedestrians.remove(&id);
            self.metrics.despawned += 1;
        }
        collision
            .set_vehicle_proxies(&vehicles.values().map(Vehicle::proxy).collect::<Vec<_>>())?;
        if observers.is_empty() {
            return Ok(());
        }
        // A bounded scan each second; lack of a safe candidate leaves a slot empty.
        let attempts = self.road.candidates.len().min(128);
        for _ in 0..attempts {
            if self.traffic.len() >= TRAFFIC_CAP || self.road.candidates.is_empty() {
                break;
            }
            let (_, to, p, yaw) =
                self.road.candidates[self.car_cursor % self.road.candidates.len()];
            self.car_cursor += 1;
            if !spawn_region(p, observers)
                || !hidden(p, observers, collision)
                || vehicles
                    .values()
                    .any(|v| distance(p, v.state.motor.position.0.map(|v| v.0)) < 12.0)
            {
                continue;
            }
            let template = manifest.vehicle_templates
                [self.traffic.len() % manifest.vehicle_templates.len()]
            .clone();
            if collision.overlaps_vehicle(p, yaw, template.dimensions.map(|v| v as f32))? {
                self.metrics.blocked_spawns += 1;
                continue;
            }
            let id = allocate(next_id)?;
            let motor = sim_core::spawn_vehicle(p, yaw)?;
            vehicles.insert(
                id,
                Vehicle {
                    id: EntityId(id),
                    state: VehicleState {
                        motor,
                        health: VehicleHealth(template.health),
                        driver_id: None,
                        disabled: false,
                    },
                    template,
                    idle_ticks: 0,
                },
            );
            self.pools.insert(id, VehiclePool::Traffic);
            self.traffic.insert(
                id,
                Traffic {
                    phase: TrafficPhase::Driving,
                    route: VecDeque::from([to]),
                    input: sim_core::neutral_input(tick, Yaw(yaw)),
                    target_speed: 0.0,
                    next_plan: tick,
                    progress_at: tick,
                    progress_position: p,
                    far_since: None,
                    parking: None,
                    born: tick,
                },
            );
            self.metrics.spawned += 1;
            collision
                .set_vehicle_proxies(&vehicles.values().map(Vehicle::proxy).collect::<Vec<_>>())?;
        }
        let attempts = self.sidewalk.candidates.len().min(256);
        for _ in 0..attempts {
            if self.pedestrians.len() >= PEDESTRIAN_CAP || self.sidewalk.candidates.is_empty() {
                break;
            }
            let (from, to, mut p, yaw) =
                self.sidewalk.candidates[self.pedestrian_cursor % self.sidewalk.candidates.len()];
            self.pedestrian_cursor += 1;
            p[1] = 0.02;
            if !spawn_region(p, observers)
                || !hidden(p, observers, collision)
                || self
                    .pedestrians
                    .values()
                    .any(|ped| distance(p, ped.state.motor.position.0.map(|v| v.0)) < 3.0)
            {
                continue;
            }
            if collision.overlaps_character(p, QueryFilter::MOVEMENT)? {
                self.metrics.blocked_spawns += 1;
                continue;
            }
            let id = allocate(next_id)?;
            self.pedestrians.insert(
                id,
                Pedestrian {
                    id: EntityId(id),
                    state: CharacterState {
                        equipment: None,
                        motor: sim_core::spawn_character(p, yaw)?,
                        health: CharacterHealth(100),
                        mode: CharacterMode::OnFoot,
                        vehicle_id: None,
                    },
                    phase: PedestrianPhase::Walking,
                    from,
                    to,
                    input: sim_core::neutral_input(tick, Yaw(yaw)),
                    wait_until: 0,
                    far_since: None,
                    progress_at: tick,
                    progress_position: p,
                    flee_until: 0,
                    threat: p,
                },
            );
            self.metrics.spawned += 1;
        }
        Ok(())
    }
    /// A traffic car's AI driver is retained until it parks in an authored spot.
    pub fn release_ai(&mut self, id: u32) {
        if let Some(b) = self.traffic.get_mut(&id) {
            b.phase = TrafficPhase::Available;
        }
        self.intersections.release(id);
    }
    pub fn can_enter(&self, id: u32) -> bool {
        self.traffic
            .get(&id)
            .is_none_or(|b| b.phase == TrafficPhase::Available)
    }
    pub fn vehicle_input(&self, id: u32, tick: u32, speed: f32) -> Option<InputFrame> {
        self.traffic
            .get(&id)
            .filter(|b| b.phase != TrafficPhase::Available)
            .map(|b| {
                let mut i = b.input.clone();
                i.tick = tick;
                i.move_z = InputAxis(
                    ((b.target_speed - speed) * 2.0)
                        .clamp(-1.0, 1.0)
                        .mul_add(32767.0, 0.0) as i16,
                );
                i
            })
    }
    pub fn decide(
        &mut self,
        tick: u32,
        vehicles: &BTreeMap<u32, Vehicle>,
        observers: &[Observer],
        manifest: &WorldManifest,
        collision: &mut CollisionWorld,
    ) -> Result<(), String> {
        let ids: Vec<_> = self.traffic.keys().copied().collect();
        for id in &ids {
            let car = &vehicles[id];
            let b = self.traffic.get_mut(id).expect("traffic brain");
            if car.state.driver_id.is_some()
                || car.state.disabled
                || car.state.health.0 == 0
                || b.phase == TrafficPhase::Available
            {
                continue;
            }
            let p = car.state.motor.position.0.map(|v| v.0);
            if distance(p, b.progress_position) >= 2.0 {
                b.progress_position = p;
                b.progress_at = tick;
            }
            if tick >= b.next_plan
                && (b.route.len() <= 1 || tick.saturating_sub(b.progress_at) >= 150)
                && !self.queue.contains(id)
            {
                self.queue.push_back(*id);
            }
        }
        let mut jobs = 0;
        while jobs < PATHS_PER_TICK {
            let Some(id) = self.queue.pop_front() else {
                break;
            };
            let Some(brain) = self.traffic.get(&id) else {
                continue;
            };
            if tick < brain.next_plan
                || brain.phase == TrafficPhase::Available
                || vehicles[&id].state.driver_id.is_some()
            {
                continue;
            }
            let p = vehicles[&id].state.motor.position.0.map(|v| v.0);
            let start = brain
                .route
                .front()
                .copied()
                .or_else(|| self.road.nearest(p));
            let nodes: Vec<_> = self
                .road
                .edges
                .keys()
                .copied()
                .filter(|n| distance(p, self.road.points[n]) > 60.0)
                .collect();
            let random = self.random();
            let goal = nodes.get(random % nodes.len().max(1)).copied();
            let route = start
                .zip(goal)
                .map_or_else(VecDeque::new, |(a, b)| self.road.path(a, b));
            let brain = self.traffic.get_mut(&id).expect("traffic brain");
            brain.route = route;
            brain.next_plan = tick.saturating_add(15);
            if tick.saturating_sub(brain.progress_at) >= 150 {
                self.metrics.replans += 1;
            }
            jobs += 1;
            self.metrics.path_jobs += 1;
        }
        self.metrics.max_path_jobs_per_tick = self.metrics.max_path_jobs_per_tick.max(jobs);
        if !tick.is_multiple_of(6) {
            return Ok(());
        }
        // IDs decide simultaneous reservation ties, independent of network arrival.
        let mut parking_claims: BTreeSet<_> = self
            .traffic
            .values()
            .filter(|b| b.phase == TrafficPhase::Parking)
            .filter_map(|b| b.parking)
            .collect();
        for id in ids {
            let car = &vehicles[&id];
            let b = self.traffic.get_mut(&id).expect("traffic brain");
            if car.state.driver_id.is_some()
                || car.state.disabled
                || car.state.health.0 == 0
                || b.phase == TrafficPhase::Available
            {
                self.intersections.release(id);
                continue;
            }
            let p = car.state.motor.position.0.map(|v| v.0);
            while b
                .route
                .front()
                .is_some_and(|n| distance(p, self.road.points[n]) < 5.0)
            {
                b.route.pop_front();
            }
            let mut target = b.route.front().map(|n| self.road.points[n]);
            // Vacated authored public spots are the only parking destinations.
            if b.parking.is_none() && tick.saturating_sub(b.born) > 900 {
                b.parking = manifest
                    .parking
                    .iter()
                    .enumerate()
                    .find(|(index, slot)| {
                        !slot.mission_reserved
                            && distance(p, slot.position.map(|v| v as f32)) < 18.0
                            && vehicles.values().all(|v| {
                                v.id.0 == id
                                    || distance(
                                        slot.position.map(|v| v as f32),
                                        v.state.motor.position.0.map(|v| v.0),
                                    ) > 8.0
                            })
                            && !parking_claims.contains(index)
                    })
                    .map(|(index, _)| index);
            }
            if let Some(index) = b.parking {
                parking_claims.insert(index);
                target = Some(manifest.parking[index].position.map(|v| v as f32));
                if distance(p, target.expect("parking target")) < 2.0
                    && car.state.motor.speed.0.abs() < 0.3
                {
                    b.phase = TrafficPhase::Available;
                    self.intersections.release(id);
                    continue;
                }
            }
            let mut input = sim_core::neutral_input(tick, car.state.motor.yaw);
            let Some(target) = target else {
                input.buttons = Buttons(4);
                b.phase = TrafficPhase::Stopped;
                b.input = input;
                continue;
            };
            let error = wrap(yaw_to(p, target) - car.state.motor.yaw.0);
            input.move_x = InputAxis((-error * 1.8).clamp(-1.0, 1.0).mul_add(32767.0, 0.0) as i16);
            let mut target_speed = if error.abs() > 0.2 || distance(p, target) < 25.0 {
                4.0
            } else {
                12.0
            };
            if b.parking.is_some() {
                target_speed = (distance(p, target) - 1.8).clamp(0.0, 3.0);
            }
            let mut yield_now = false;
            // All four lane corners of an intersection share a reservation key.
            if b.parking.is_none() {
                let key = (
                    (target[0] / 16.0).round() as i32,
                    (target[2] / 16.0).round() as i32,
                );
                let center = [key.0 as f32 * 16.0, 0.0, key.1 as f32 * 16.0];
                let d = distance(p, center);
                if d < car.state.motor.speed.0.abs() * 1.5 + 16.0 {
                    yield_now = !self.intersections.request(key, id, tick);
                }
                // Release past intersections, retaining only the current approach.
                self.intersections.reservations.retain(|k, (owner, _)| {
                    *owner != id
                        || *k == key
                        || distance(p, [k.0 as f32 * 16.0, 0.0, k.1 as f32 * 16.0]) < 11.0
                });
            }
            collision.exclude_vehicle(Some(id));
            let sensor = car.state.motor.speed.0.abs().mul_add(1.5, 0.0).max(4.0);
            let forward = [
                -car.state.motor.yaw.0.sin(),
                0.0,
                -car.state.motor.yaw.0.cos(),
            ];
            for lateral in [-0.45, 0.0, 0.45] {
                let origin = sim_core::anchor(
                    &car.state.motor,
                    [
                        car.template.dimensions[0] * lateral,
                        0.75,
                        -car.template.dimensions[2] / 2.0 - 0.05,
                    ],
                );
                if collision
                    .raycast(origin, forward, sensor, QueryFilter::MOVEMENT)?
                    .is_some()
                {
                    yield_now = true;
                }
            }
            // Players and pedestrians are not car collision proxies; sense them explicitly.
            for actor in observers.iter().map(|o| o.position).chain(
                self.pedestrians
                    .values()
                    .map(|p| p.state.motor.position.0.map(|v| v.0)),
            ) {
                let delta = [actor[0] - p[0], actor[2] - p[2]];
                let along = delta[0] * forward[0] + delta[1] * forward[2];
                let across = (delta[0] * forward[2] - delta[1] * forward[0]).abs();
                if along > 0.0
                    && along < sensor + car.template.dimensions[2] as f32 / 2.0
                    && across < car.template.dimensions[0] as f32 / 2.0 + 0.6
                {
                    yield_now = true;
                }
            }
            if b.parking.is_some() && distance(p, target) < 2.0 {
                input.buttons = Buttons(4);
            }
            b.target_speed = target_speed;
            if yield_now {
                input.buttons = Buttons(4);
                b.phase = TrafficPhase::Yielding;
            } else {
                input.move_z = InputAxis(if car.state.motor.speed.0 > target_speed + 0.2 {
                    -32767
                } else if car.state.motor.speed.0 < target_speed {
                    32767
                } else {
                    0
                });
                b.phase = if b.parking.is_some() {
                    TrafficPhase::Parking
                } else {
                    TrafficPhase::Driving
                };
            }
            b.input = input;
        }
        collision.exclude_vehicle(None);
        let ped_ids: Vec<_> = self.pedestrians.keys().copied().collect();
        for id in ped_ids {
            let random = self.random();
            let ped = self.pedestrians.get_mut(&id).expect("pedestrian");
            if ped.phase == PedestrianPhase::Incapacitated {
                ped.input = sim_core::neutral_input(tick, ped.state.motor.yaw);
                continue;
            }
            let p = ped.state.motor.position.0.map(|v| v.0);
            if distance(p, ped.progress_position) >= 1.0 {
                ped.progress_position = p;
                ped.progress_at = tick;
            }
            if distance(p, self.sidewalk.points[&ped.to]) < 0.4 {
                let choices: Vec<_> = self
                    .sidewalk
                    .edges
                    .get(&ped.to)
                    .into_iter()
                    .flatten()
                    .filter(|(n, _, _)| *n != ped.from)
                    .collect();
                let choice = if tick < ped.flee_until {
                    choices.iter().copied().max_by(|a, b| {
                        distance(self.sidewalk.points[&a.0], ped.threat)
                            .total_cmp(&distance(self.sidewalk.points[&b.0], ped.threat))
                    })
                } else {
                    choices.get(random % choices.len().max(1)).copied()
                }
                .or_else(|| self.sidewalk.edges.get(&ped.to).and_then(|e| e.first()));
                if let Some(&(to, _, _)) = choice {
                    ped.from = ped.to;
                    ped.to = to;
                    ped.wait_until = tick
                        + if tick >= ped.flee_until && random.is_multiple_of(4) {
                            30
                        } else {
                            0
                        };
                }
            }
            let target = self.sidewalk.points[&ped.to];
            let crossing = self
                .sidewalk
                .edges
                .get(&ped.from)
                .into_iter()
                .flatten()
                .any(|(n, _, kind)| *n == ped.to && *kind == EdgeKind::Crossing);
            let blocked = crossing
                && distance(p, self.sidewalk.points[&ped.from]) < 0.75
                && vehicles.values().any(|v| {
                    let q = v.state.motor.position.0.map(|v| v.0);
                    distance(p, q) < (v.state.motor.speed.0.abs() * 2.0 + 6.0).max(8.0)
                });
            let mut input = sim_core::neutral_input(tick, Yaw(yaw_to(p, target)));
            if tick < ped.wait_until || blocked {
                ped.phase = PedestrianPhase::Waiting;
            } else {
                ped.phase = if tick < ped.flee_until {
                    PedestrianPhase::Fleeing
                } else {
                    PedestrianPhase::Walking
                };
                input.move_z = InputAxis(
                    (if tick < ped.flee_until {
                        1.0
                    } else {
                        self.walk_axis
                    })
                    .min(1.0)
                    .mul_add(32767.0, 0.0) as i16,
                );
            }
            ped.input = input;
        }
        Ok(())
    }
    pub fn step_pedestrians(
        &mut self,
        tick: u32,
        collision: &CollisionWorld,
        config: &SimConfig,
    ) -> Result<(), String> {
        for ped in self.pedestrians.values_mut() {
            if ped.state.health.0 == 0 {
                continue;
            }
            let mut input = ped.input.clone();
            input.tick = tick;
            let result = sim_core::step_character(
                &ped.state.motor,
                &input,
                &StepContext {
                    tick,
                    collision,
                    config,
                },
            )?;
            ped.state.motor = result.motor;
        }
        Ok(())
    }
    /// Future authoritative shot/impact observations can request a graph-bound flee.
    pub fn frighten(&mut self, position: [f32; 3], tick: u32) {
        for ped in self.pedestrians.values_mut() {
            let p = ped.state.motor.position.0.map(|v| v.0);
            if distance(p, position) > 20.0 || ped.phase == PedestrianPhase::Incapacitated {
                continue;
            }
            ped.flee_until = tick.saturating_add(150);
            ped.threat = position;
            ped.wait_until = 0;
            // Reverse only on the current authored edge; never cut across a block.
            if distance(self.sidewalk.points[&ped.from], position)
                > distance(self.sidewalk.points[&ped.to], position)
                && self
                    .sidewalk
                    .edges
                    .get(&ped.to)
                    .into_iter()
                    .flatten()
                    .any(|(to, _, _)| *to == ped.from)
            {
                std::mem::swap(&mut ped.from, &mut ped.to);
            }
        }
    }
}
fn allocate(next: &mut u32) -> Result<u32, String> {
    let id = *next;
    *next = next.checked_add(1).ok_or("entity IDs exhausted")?;
    Ok(id)
}
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}
fn yaw_to(a: [f32; 3], b: [f32; 3]) -> f32 {
    (-(b[0] - a[0])).atan2(-(b[2] - a[2]))
}
fn wrap(yaw: f32) -> f32 {
    (yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}
fn spawn_region(p: [f32; 3], observers: &[Observer]) -> bool {
    observers.iter().any(|o| distance(p, o.position) <= 160.0)
        && observers.iter().all(|o| distance(p, o.position) >= 40.0)
}
pub(crate) fn hidden(p: [f32; 3], observers: &[Observer], collision: &CollisionWorld) -> bool {
    observers.iter().all(|o| {
        let a = [o.position[0], o.position[1] + 1.7, o.position[2]];
        // Check head and body to avoid spawning a tall actor behind a low obstacle.
        [0.8, 2.0].into_iter().all(|height| {
            let b = [p[0], p[1] + height, p[2]];
            let delta = std::array::from_fn(|i| b[i] - a[i]);
            let d = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
            collision
                .raycast(a, delta, d, QueryFilter::VISION)
                .is_ok_and(|hit| hit.is_some_and(|h| h.distance < d - 1.0))
        })
    })
}
fn aged_far(tick: u32, p: [f32; 3], observers: &[Observer], since: &mut Option<u32>) -> bool {
    if observers.iter().any(|o| distance(p, o.position) <= 220.0) {
        *since = None;
        false
    } else {
        tick.saturating_sub(*since.get_or_insert(tick)) >= 900
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_loader::WorldContent;
    use std::path::Path;
    fn content() -> WorldContent {
        WorldContent::load_index(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
            "city.json",
        )
        .unwrap()
    }
    fn population(content: &WorldContent) -> PopulationSystem {
        PopulationSystem::new(
            &content.manifest,
            &content.config,
            std::iter::empty(),
            424242,
        )
        .unwrap()
    }
    fn collision(content: &WorldContent) -> (SimConfig, CollisionWorld) {
        let config = SimConfig::from_content(&content.config).unwrap();
        let collision = CollisionWorld::from_content(&content.collision, &config).unwrap();
        (config, collision)
    }
    fn advance(
        pop: &mut PopulationSystem,
        cars: &mut BTreeMap<u32, Vehicle>,
        collision: &mut CollisionWorld,
        setup: (&SimConfig, &WorldContent),
        tick: u32,
        observers: &[Observer],
        next: &mut u32,
    ) {
        let (config, content) = setup;
        pop.direct(tick, observers, cars, &content.manifest, collision, next)
            .unwrap();
        collision
            .set_vehicle_proxies(&cars.values().map(Vehicle::proxy).collect::<Vec<_>>())
            .unwrap();
        pop.decide(tick, cars, observers, &content.manifest, collision)
            .unwrap();
        for id in cars.keys().copied().collect::<Vec<_>>() {
            let car = cars.get_mut(&id).unwrap();
            let input = pop
                .vehicle_input(id, tick, car.state.motor.speed.0)
                .unwrap_or_else(|| sim_core::neutral_input(tick, car.state.motor.yaw));
            collision.exclude_vehicle(Some(id));
            car.state.motor = sim_core::step_vehicle(
                &car.state.motor,
                &input,
                &car.template,
                &StepContext {
                    tick,
                    collision,
                    config,
                },
            )
            .unwrap()
            .motor;
            assert!(car.state.motor.speed.0 <= 12.001, "traffic exceeded 12 m/s");
            assert!(
                !collision
                    .overlaps_vehicle(
                        car.state.motor.position.0.map(|v| v.0),
                        car.state.motor.yaw.0,
                        car.template.dimensions.map(|v| v as f32)
                    )
                    .unwrap()
            );
            collision
                .set_vehicle_proxies(&cars.values().map(Vehicle::proxy).collect::<Vec<_>>())
                .unwrap();
        }
        collision.exclude_vehicle(None);
        pop.step_pedestrians(tick, collision, config).unwrap();
    }
    #[test]
    fn traffic_parks_in_an_authored_free_slot_and_becomes_available() {
        let mut c = content();
        c.manifest.parking.retain(|p| !p.mission_reserved);
        c.manifest.parking.truncate(1);
        let (config, mut collision) = collision(&c);
        let mut pop = population(&c);
        let mut cars = BTreeMap::new();
        let mut next = 1;
        let observers = [Observer {
            position: [-181.0, 0.02, 177.0],
            yaw: 0.0,
        }];
        pop.direct(
            30,
            &observers,
            &mut cars,
            &c.manifest,
            &mut collision,
            &mut next,
        )
        .unwrap();
        let id = *cars.keys().next().unwrap();
        cars.retain(|k, _| *k == id);
        pop.traffic.retain(|k, _| *k == id);
        pop.pools.retain(|k, _| *k == id);
        pop.pedestrians.clear();
        let slot = c.manifest.parking[0].position.map(|v| v as f32);
        cars.get_mut(&id).unwrap().state.motor =
            sim_core::spawn_vehicle([slot[0], 0.0, slot[2] + 12.0], 0.0).unwrap();
        let brain = pop.traffic.get_mut(&id).unwrap();
        brain.born = 0;
        brain.progress_at = 900;
        for tick in 901..=1500 {
            collision
                .set_vehicle_proxies(&cars.values().map(Vehicle::proxy).collect::<Vec<_>>())
                .unwrap();
            pop.decide(tick, &cars, &[], &c.manifest, &mut collision)
                .unwrap();
            let car = cars.get_mut(&id).unwrap();
            let input = pop
                .vehicle_input(id, tick, car.state.motor.speed.0)
                .unwrap_or_else(|| sim_core::neutral_input(tick, car.state.motor.yaw));
            collision.exclude_vehicle(Some(id));
            car.state.motor = sim_core::step_vehicle(
                &car.state.motor,
                &input,
                &car.template,
                &StepContext {
                    tick,
                    collision: &collision,
                    config: &config,
                },
            )
            .unwrap()
            .motor;
            collision.exclude_vehicle(None);
            if pop.can_enter(id) {
                assert!(distance(car.state.motor.position.0.map(|v| v.0), slot) < 2.0);
                assert!(car.state.motor.speed.0.abs() < 0.3);
                assert_eq!(pop.pools[&id], VehiclePool::Traffic);
                return;
            }
        }
        panic!(
            "failed to park: {:?}, phase {:?}",
            cars[&id].state.motor, pop.traffic[&id].phase
        );
    }
    #[test]
    fn same_seed_and_observers_produce_identical_population_and_motion() {
        let c = content();
        let (config, mut a_collision) = collision(&c);
        let (_, mut b_collision) = collision(&c);
        let mut a = population(&c);
        let mut b = population(&c);
        let mut a_cars = BTreeMap::new();
        let mut b_cars = BTreeMap::new();
        let mut a_id = 1;
        let mut b_id = 1;
        let observers = [Observer {
            position: [-181.0, 0.02, 177.0],
            yaw: 0.0,
        }];
        for tick in 1..=180 {
            advance(
                &mut a,
                &mut a_cars,
                &mut a_collision,
                (&config, &c),
                tick,
                &observers,
                &mut a_id,
            );
            advance(
                &mut b,
                &mut b_cars,
                &mut b_collision,
                (&config, &c),
                tick,
                &observers,
                &mut b_id,
            );
        }
        assert_eq!(a_id, b_id);
        assert_eq!(
            a_cars
                .iter()
                .map(|(id, v)| (*id, v.state.clone()))
                .collect::<Vec<_>>(),
            b_cars
                .iter()
                .map(|(id, v)| (*id, v.state.clone()))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            a.pedestrians
                .iter()
                .map(|(id, p)| (*id, p.state.clone()))
                .collect::<Vec<_>>(),
            b.pedestrians
                .iter()
                .map(|(id, p)| (*id, p.state.clone()))
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn intersections_first_request_wins_stable_ties_expire_and_release() {
        let mut r = Intersections::default();
        assert!(r.request((1, 2), 7, 6));
        assert!(!r.request((1, 2), 8, 6));
        assert!(r.request((1, 2), 7, 100));
        assert!(!r.request((1, 2), 8, 125));
        assert!(r.request((1, 2), 8, 126));
        r.release(8);
        assert!(r.request((1, 2), 7, 127));
    }
    #[test]
    fn parked_budget_includes_eight_reserved_and_tags_survive_possession() {
        let c = content();
        assert!(PopulationSystem::new(&c.manifest, &c.config, 1..=16, 1).is_ok());
        assert!(PopulationSystem::new(&c.manifest, &c.config, 1..=17, 1).is_err());
        let mut invalid = c.manifest.clone();
        let slot = invalid
            .parking
            .iter()
            .find(|p| p.mission_reserved)
            .unwrap()
            .clone();
        invalid.parking.push(slot);
        assert!(PopulationSystem::new(&invalid, &c.config, 1..=4, 1).is_err());
        let mut p = population(&c);
        let (_, mut collision) = collision(&c);
        let mut cars = BTreeMap::new();
        let mut next = 1;
        let observers = [Observer {
            position: [-181.0, 0.02, 177.0],
            yaw: 0.0,
        }];
        for tick in (30..=300).step_by(30) {
            p.direct(
                tick,
                &observers,
                &mut cars,
                &c.manifest,
                &mut collision,
                &mut next,
            )
            .unwrap();
        }
        assert!(!cars.is_empty());
        let id = *cars.keys().next().unwrap();
        assert!(!p.can_enter(id));
        p.traffic.get_mut(&id).unwrap().phase = TrafficPhase::Available;
        assert!(p.can_enter(id));
        cars.get_mut(&id).unwrap().state.driver_id = Some(EntityId(1000));
        let tag = p.pools[&id];
        p.direct(1500, &[], &mut cars, &c.manifest, &mut collision, &mut next)
            .unwrap();
        p.direct(3000, &[], &mut cars, &c.manifest, &mut collision, &mut next)
            .unwrap();
        assert!(cars.contains_key(&id));
        assert_eq!(p.pools[&id], tag);
        assert_eq!(p.traffic.len(), 1);
        assert!(p.pedestrians.is_empty());
    }
    #[test]
    fn seeded_population_traversal_is_bounded_hidden_collision_safe_and_recycles() {
        let c = content();
        let (config, mut collision) = collision(&c);
        let mut pop = population(&c);
        let mut cars = BTreeMap::new();
        let mut next = 1;
        let mut known = BTreeSet::new();
        let mut moved = BTreeSet::new();
        let mut moved_cars = BTreeSet::new();
        let mut initial = BTreeMap::new();
        let mut max_cars = 0;
        let mut max_peds = 0;
        for tick in 1..=2100 {
            let location = if tick < 1000 {
                [-181.0, 0.02, 177.0]
            } else {
                [140.0, 0.02, -140.0]
            };
            let observers = [Observer {
                position: location,
                yaw: 0.0,
            }];
            advance(
                &mut pop,
                &mut cars,
                &mut collision,
                (&config, &c),
                tick,
                &observers,
                &mut next,
            );
            max_cars = max_cars.max(cars.len());
            max_peds = max_peds.max(pop.pedestrians.len());
            assert!(cars.len() <= TRAFFIC_CAP);
            assert!(pop.pedestrians.len() <= PEDESTRIAN_CAP);
            assert_eq!(
                cars.len(),
                pop.pools
                    .values()
                    .filter(|p| **p == VehiclePool::Traffic)
                    .count()
            );
            for (id, p) in cars
                .iter()
                .map(|(&id, c)| (id, c.state.motor.position.0.map(|v| v.0)))
                .chain(
                    pop.pedestrians
                        .iter()
                        .map(|(&id, p)| (id, p.state.motor.position.0.map(|v| v.0))),
                )
            {
                if known.insert(id) {
                    assert!(spawn_region(p, &observers));
                    assert!(hidden(p, &observers, &collision));
                    initial.insert(id, p);
                }
                if distance(p, initial[&id]) > 5.0 {
                    moved.insert(id);
                }
                if cars.contains_key(&id) && distance(p, initial[&id]) > 20.0 {
                    moved_cars.insert(id);
                }
            }
            for ped in pop.pedestrians.values() {
                let p = ped.state.motor.position.0.map(|v| v.0);
                assert!(
                    !collision
                        .overlaps_character(p, QueryFilter::MOVEMENT)
                        .unwrap()
                );
                assert!(
                    ped.state.motor.velocity.0[0]
                        .0
                        .hypot(ped.state.motor.velocity.0[2].0)
                        <= 4.01
                );
            }
        }
        println!(
            "population peak cars={max_cars}, pedestrians={max_peds}, actors moved >5m={}, cars moved >20m={}, spawned={}, recycled={}, path jobs={}, max/tick={}",
            moved.len(),
            moved_cars.len(),
            pop.metrics.spawned,
            pop.metrics.despawned,
            pop.metrics.path_jobs,
            pop.metrics.max_path_jobs_per_tick
        );
        assert_eq!(max_cars, TRAFFIC_CAP);
        assert_eq!(max_peds, PEDESTRIAN_CAP);
        assert!(moved.len() > 24);
        assert!(moved_cars.len() >= 12);
        assert!(pop.metrics.despawned > 0);
        assert!(pop.metrics.max_path_jobs_per_tick <= PATHS_PER_TICK);
        for tick in (2130..=3090).step_by(30) {
            pop.direct(tick, &[], &mut cars, &c.manifest, &mut collision, &mut next)
                .unwrap();
        }
        assert!(cars.is_empty());
        assert!(pop.pedestrians.is_empty());
        assert!(pop.pools.is_empty());
    }
    #[test]
    fn sensor_brakes_and_unreachable_graph_stops_without_moving_through_static_geometry() {
        let c = content();
        let (_, mut collision) = collision(&c);
        let mut p = population(&c);
        let mut cars = BTreeMap::new();
        let mut next = 1;
        let observers = [Observer {
            position: [-181.0, 0.02, 177.0],
            yaw: 0.0,
        }];
        p.direct(
            30,
            &observers,
            &mut cars,
            &c.manifest,
            &mut collision,
            &mut next,
        )
        .unwrap();
        let id = *cars.keys().next().unwrap();
        let car = &cars[&id];
        let mut obstacle = sim_core::anchor(&car.state.motor, [0.0, 0.0, -8.0]);
        obstacle[1] = 0.0;
        let other = 9999;
        let template = car.template.clone();
        cars.insert(
            other,
            Vehicle {
                id: EntityId(other),
                state: VehicleState {
                    motor: sim_core::spawn_vehicle(obstacle, car.state.motor.yaw.0).unwrap(),
                    health: VehicleHealth(100),
                    driver_id: None,
                    disabled: false,
                },
                template,
                idle_ticks: 30,
            },
        );
        collision
            .set_vehicle_proxies(&cars.values().map(Vehicle::proxy).collect::<Vec<_>>())
            .unwrap();
        p.decide(36, &cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(p.traffic[&id].phase, TrafficPhase::Yielding);
        assert_eq!(p.vehicle_input(id, 37, 0.0).unwrap().buttons.0, 4);
        p.road.edges.clear();
        p.traffic.get_mut(&id).unwrap().route.clear();
        p.decide(54, &cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(p.traffic[&id].phase, TrafficPhase::Stopped);
        assert_eq!(p.vehicle_input(id, 54, 0.0).unwrap().buttons.0, 4);
    }
    #[test]
    fn pedestrians_use_authored_edges_wait_for_crossings_and_flee_on_graph() {
        let c = content();
        let (_, mut collision) = collision(&c);
        let mut p = population(&c);
        let mut cars = BTreeMap::new();
        let mut next = 1;
        let observers = [Observer {
            position: [-181.0, 0.02, 177.0],
            yaw: 0.0,
        }];
        p.direct(
            30,
            &observers,
            &mut cars,
            &c.manifest,
            &mut collision,
            &mut next,
        )
        .unwrap();
        let id = *p.pedestrians.keys().next().unwrap();
        let (from, to) = p
            .sidewalk
            .edges
            .iter()
            .find_map(|(&from, e)| {
                e.iter()
                    .find(|(_, _, k)| *k == EdgeKind::Crossing)
                    .map(|(to, _, _)| (from, *to))
            })
            .unwrap();
        let start = p.sidewalk.points[&from];
        let target = p.sidewalk.points[&to];
        let ped = p.pedestrians.get_mut(&id).unwrap();
        ped.from = from;
        ped.to = to;
        ped.state.motor =
            sim_core::spawn_character([start[0], 0.02, start[2]], yaw_to(start, target)).unwrap();
        let template = c.manifest.vehicle_templates[0].clone();
        let car_position = [
            (start[0] + target[0]) / 2.0,
            0.0,
            (start[2] + target[2]) / 2.0,
        ];
        cars.clear();
        cars.insert(
            9999,
            Vehicle {
                id: EntityId(9999),
                state: VehicleState {
                    motor: sim_core::spawn_vehicle(car_position, 0.0).unwrap(),
                    health: VehicleHealth(100),
                    driver_id: None,
                    disabled: false,
                },
                template,
                idle_ticks: 30,
            },
        );
        p.traffic.clear();
        p.decide(36, &cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(p.pedestrians[&id].phase, PedestrianPhase::Waiting);
        assert_eq!(p.pedestrians[&id].input.move_z.0, 0);
        cars.clear();
        p.decide(42, &cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(p.pedestrians[&id].phase, PedestrianPhase::Walking);
        let position = p.pedestrians[&id].state.motor.position.0.map(|v| v.0);
        p.frighten(position, 42);
        p.decide(48, &cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(p.pedestrians[&id].phase, PedestrianPhase::Fleeing);
        p.decide(198, &cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(p.pedestrians[&id].phase, PedestrianPhase::Walking);
        let ped = &p.pedestrians[&id];
        assert!(
            p.sidewalk.edges[&ped.from]
                .iter()
                .any(|(to, _, _)| *to == ped.to)
        );
    }
}
