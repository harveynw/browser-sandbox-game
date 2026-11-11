//! Bounded, deterministic ambient population. Only World commits motor results.
pub(crate) mod native;
pub(crate) mod pedestrians;
pub(crate) mod traffic;
use crate::components::Vehicle;
use content::{EdgeKind, RoadGraph, WorldManifest};
use protocol::*;
use sim_core::{CollisionQueries, CollisionWorld, QueryFilter};
#[cfg(test)]
use sim_core::{SimConfig, StepContext};
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

use crate::components::Pedestrian;
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
#[derive(bevy_ecs::prelude::Resource)]
pub struct PopulationSystem {
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
    #[cfg(test)]
    pub fn new(
        manifest: &WorldManifest,
        config: &content::SimConfig,
        parked: impl Iterator<Item = u32>,
        seed: u64,
        vehicles: &mut crate::ecs::EntityStore,
    ) -> Result<Self, String> {
        let parked: Vec<_> = parked.collect();
        let population = Self::from_content(manifest, config, parked.len(), seed)?;
        for id in parked {
            vehicles.insert_value::<crate::components::PoolMembership>(id, VehiclePool::Parked);
        }
        Ok(population)
    }
    pub(crate) fn from_content(
        manifest: &WorldManifest,
        config: &content::SimConfig,
        parked_count: usize,
        seed: u64,
    ) -> Result<Self, String> {
        if parked_count > PARKED_CAP - RECOVERY_RESERVED
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
    #[cfg(test)]
    pub fn direct(
        &mut self,
        tick: u32,
        observers: &[Observer],
        vehicles: &mut crate::ecs::EntityStore,
        manifest: &WorldManifest,
        collision: &mut CollisionWorld,
        next_id: &mut u32,
    ) -> Result<(), String> {
        let mut state =
            bevy_ecs::system::SystemState::<native::DirectorActors>::new(vehicles.ecs_mut());
        let result = native::direct_actors(
            self,
            tick,
            observers,
            manifest,
            collision,
            next_id,
            &mut state
                .get_mut(vehicles.ecs_mut())
                .expect("native actor queries"),
        );
        state.apply(vehicles.ecs_mut());
        result
    }
    #[cfg(test)]
    pub fn release_ai(&mut self, id: u32, vehicles: &mut crate::ecs::EntityStore) {
        if let Some(b) = vehicles.get_mut_by_id::<Traffic>(&id) {
            b.phase = TrafficPhase::Available;
        }
        self.intersections.release(id);
    }
    #[cfg(test)]
    pub fn can_enter(&self, id: u32, vehicles: &crate::ecs::EntityStore) -> bool {
        vehicles
            .view::<Traffic>()
            .get(&id)
            .is_none_or(|b| b.phase == TrafficPhase::Available)
    }
    #[cfg(test)]
    pub fn vehicle_input(
        &self,
        id: u32,
        tick: u32,
        speed: f32,
        vehicles: &crate::ecs::EntityStore,
    ) -> Option<InputFrame> {
        vehicles.view::<Traffic>().get(&id).and_then(|traffic| {
            crate::systems::population::input::motor_input(&traffic, tick, speed)
        })
    }
    #[cfg(test)]
    pub fn decide(
        &mut self,
        tick: u32,
        vehicles: &mut crate::ecs::EntityStore,
        observers: &[Observer],
        manifest: &WorldManifest,
        collision: &mut CollisionWorld,
    ) -> Result<(), String> {
        let mut routes =
            bevy_ecs::system::SystemState::<traffic::RouteActors>::new(vehicles.ecs_mut());
        traffic::plan_actors(
            self,
            tick,
            &mut routes.get_mut(vehicles.ecs_mut()).expect("route queries"),
        );
        let mut driving =
            bevy_ecs::system::SystemState::<traffic::DrivingActors>::new(vehicles.ecs_mut());
        traffic::drive_actors(
            self,
            tick,
            observers,
            manifest,
            collision,
            &mut driving
                .get_mut(vehicles.ecs_mut())
                .expect("traffic queries"),
        )?;
        let mut pedestrians =
            bevy_ecs::system::SystemState::<pedestrians::PedestrianActors>::new(vehicles.ecs_mut());
        pedestrians::decide_actors(
            self,
            tick,
            &mut pedestrians
                .get_mut(vehicles.ecs_mut())
                .expect("pedestrian queries"),
        );
        Ok(())
    }
    #[cfg(test)]
    pub fn step_pedestrians(
        &mut self,
        tick: u32,
        collision: &CollisionWorld,
        config: &SimConfig,
        vehicles: &mut crate::ecs::EntityStore,
    ) -> Result<(), String> {
        for ped in vehicles.values_mut::<Pedestrian>() {
            crate::systems::movement::pedestrians::step_motor(&mut *ped, tick, collision, config)?;
        }
        Ok(())
    }
    fn react_to_noise(&self, ped: &mut Pedestrian, position: [f32; 3], tick: u32) {
        let p = ped.state.motor.position.0.map(|v| v.0);
        if distance(p, position) > 20.0 || ped.phase == PedestrianPhase::Incapacitated {
            return;
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
    #[cfg(test)]
    pub fn frighten(
        &mut self,
        position: [f32; 3],
        tick: u32,
        vehicles: &mut crate::ecs::EntityStore,
    ) {
        for ped in vehicles.values_mut::<Pedestrian>() {
            self.react_to_noise(ped, position, tick);
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
            &mut crate::ecs::EntityStore::new(),
        )
        .unwrap()
    }
    #[test]
    fn route_schedule_bounds_work_without_running_driving_or_pedestrian_decisions() {
        use bevy_ecs::prelude::*;
        let c = content();
        let mut core = crate::world::World::new(c).unwrap();
        let population = population(&core.content);
        let ids: Vec<_> = core
            .entities
            .view::<Vehicle>()
            .keys()
            .take(6)
            .copied()
            .collect();
        assert_eq!(ids.len(), 6);
        for &id in &ids {
            let car = core.entities.get::<Vehicle>(EntityId(id)).unwrap();
            let input = sim_core::neutral_input(0, car.state.motor.yaw);
            let position = car.state.motor.position.0.map(|v| v.0);
            core.entities.insert(
                id,
                Traffic {
                    phase: TrafficPhase::Driving,
                    route: VecDeque::new(),
                    input,
                    target_speed: 0.0,
                    next_plan: 0,
                    progress_at: 0,
                    progress_position: position,
                    far_since: None,
                    parking: None,
                    born: 0,
                },
            );
        }
        let mut world = core.entities.into_world();
        world.insert_resource(population);
        world.init_resource::<crate::systems::planning::Planning>();
        let mut schedule = Schedule::default();
        schedule.add_systems(traffic::plan_routes);
        for (tick, expected) in [(1, 4), (2, 6)] {
            world
                .resource_mut::<crate::systems::planning::Planning>()
                .tick = tick;
            schedule.run(&mut world);
            let population = world.resource::<PopulationSystem>();
            assert_eq!(population.metrics.path_jobs, expected);
            assert_eq!(population.metrics.max_path_jobs_per_tick, PATHS_PER_TICK);
            assert!(
                world
                    .query::<&Traffic>()
                    .iter(&world)
                    .all(|b| b.input.tick == 0)
            );
        }
    }
    #[test]
    fn pedestrian_schedule_keeps_five_hz_cadence_and_incapacitated_rng_consumption() {
        use bevy_ecs::prelude::*;
        let c = content();
        let mut population = population(&c);
        let (&from, edges) = population
            .sidewalk
            .edges
            .iter()
            .find(|(_, e)| !e.is_empty())
            .unwrap();
        let to = edges[0].0;
        let position = population.sidewalk.points[&from];
        let initial_rng = population.rng;
        population.random();
        let expected_rng = population.rng;
        population.rng = initial_rng;
        let mut world = World::new();
        world.insert_resource(population);
        world.init_resource::<crate::systems::planning::Planning>();
        let entity = world
            .spawn((
                crate::ecs::GameId(EntityId(1)),
                Pedestrian {
                    id: EntityId(1),
                    state: CharacterState {
                        equipment: None,
                        motor: sim_core::spawn_character(position, 0.0).unwrap(),
                        health: CharacterHealth(0),
                        mode: CharacterMode::Downed,
                        vehicle_id: None,
                    },
                    phase: PedestrianPhase::Incapacitated,
                    from,
                    to,
                    input: sim_core::neutral_input(0, Yaw(0.0)),
                    wait_until: 0,
                    far_since: None,
                    progress_at: 0,
                    progress_position: position,
                    flee_until: 0,
                    threat: [0.0; 3],
                },
            ))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(pedestrians::decide);
        world
            .resource_mut::<crate::systems::planning::Planning>()
            .tick = 5;
        schedule.run(&mut world);
        assert_eq!(world.resource::<PopulationSystem>().rng, initial_rng);
        assert_eq!(world.get::<Pedestrian>(entity).unwrap().input.tick, 0);
        world
            .resource_mut::<crate::systems::planning::Planning>()
            .tick = 6;
        schedule.run(&mut world);
        assert_eq!(world.resource::<PopulationSystem>().rng, expected_rng);
        let ped = world.get::<Pedestrian>(entity).unwrap();
        assert_eq!(ped.phase, PedestrianPhase::Incapacitated);
        assert_eq!(ped.input, sim_core::neutral_input(6, ped.state.motor.yaw));
    }
    #[test]
    fn noise_reader_uses_incident_tick_skips_incapacitated_and_does_not_replay() {
        use crate::gameplay_messages::NoiseEmitted;
        use bevy_ecs::prelude::*;
        let content = content();
        let population = population(&content);
        let (&from, edges) = population
            .sidewalk
            .edges
            .iter()
            .find(|(_, edges)| !edges.is_empty())
            .unwrap();
        let to = edges[0].0;
        let position = population.sidewalk.points[&from];
        let ped = |id, phase| Pedestrian {
            id: EntityId(id),
            state: CharacterState {
                equipment: None,
                motor: sim_core::spawn_character(position, 0.0).unwrap(),
                health: CharacterHealth(100),
                mode: CharacterMode::OnFoot,
                vehicle_id: None,
            },
            phase,
            from,
            to,
            input: sim_core::neutral_input(0, Yaw(0.0)),
            wait_until: 99,
            far_since: None,
            progress_at: 0,
            progress_position: position,
            flee_until: 0,
            threat: [0.0; 3],
        };
        let mut world = bevy_ecs::world::World::new();
        world.init_resource::<Messages<NoiseEmitted>>();
        world.insert_resource(population);
        let walking = world
            .spawn((
                crate::ecs::GameId(EntityId(1)),
                ped(1, PedestrianPhase::Walking),
            ))
            .id();
        let down = world
            .spawn((
                crate::ecs::GameId(EntityId(2)),
                ped(2, PedestrianPhase::Incapacitated),
            ))
            .id();
        world.write_message(NoiseEmitted { position, tick: 42 });
        let mut schedule = Schedule::default();
        schedule.add_systems(hear_noise);
        schedule.run(&mut world);
        let state = world.get::<Pedestrian>(walking).unwrap();
        assert_eq!(state.flee_until, 192);
        assert_eq!(state.threat, position);
        assert_eq!(state.wait_until, 0);
        assert_eq!(world.get::<Pedestrian>(down).unwrap().flee_until, 0);
        world.get_mut::<Pedestrian>(walking).unwrap().flee_until = 0;
        schedule.run(&mut world);
        assert_eq!(world.get::<Pedestrian>(walking).unwrap().flee_until, 0);
        world.write_message(NoiseEmitted { position, tick: 43 });
        schedule.run(&mut world);
        assert_eq!(world.get::<Pedestrian>(walking).unwrap().flee_until, 193);
    }
    fn collision(content: &WorldContent) -> (SimConfig, CollisionWorld) {
        let config = SimConfig::from_content(&content.config).unwrap();
        let collision = CollisionWorld::from_content(&content.collision, &config).unwrap();
        (config, collision)
    }
    fn advance(
        pop: &mut PopulationSystem,
        cars: &mut crate::ecs::EntityStore,
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
            .set_vehicle_proxies(
                &cars
                    .view::<Vehicle>()
                    .values()
                    .map(crate::systems::vehicle::proxy)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        pop.decide(tick, cars, observers, &content.manifest, collision)
            .unwrap();
        for id in cars.view::<Vehicle>().keys().copied().collect::<Vec<_>>() {
            let car = cars.view::<Vehicle>().get(&id).unwrap();
            let input = pop
                .vehicle_input(id, tick, car.state.motor.speed.0, cars)
                .unwrap_or_else(|| sim_core::neutral_input(tick, car.state.motor.yaw));
            let car = cars.get_mut_by_id::<Vehicle>(&id).unwrap();
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
                .set_vehicle_proxies(
                    &cars
                        .view::<Vehicle>()
                        .values()
                        .map(crate::systems::vehicle::proxy)
                        .collect::<Vec<_>>(),
                )
                .unwrap();
        }
        collision.exclude_vehicle(None);
        pop.step_pedestrians(tick, collision, config, cars).unwrap();
    }
    #[test]
    fn traffic_parks_in_an_authored_free_slot_and_becomes_available() {
        let mut c = content();
        c.manifest.parking.retain(|p| !p.mission_reserved);
        c.manifest.parking.truncate(1);
        let (config, mut collision) = collision(&c);
        let mut pop = population(&c);
        let mut cars = crate::ecs::EntityStore::new();
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
        let id = *cars.view::<Vehicle>().keys().next().unwrap();
        cars.retain::<Vehicle>(|k, _| *k == id);
        cars.retain::<Traffic>(|k, _| *k == id);
        cars.retain_value::<crate::components::PoolMembership>(|k, _| *k == id);
        cars.clear::<Pedestrian>();
        let slot = c.manifest.parking[0].position.map(|v| v as f32);
        cars.get_mut_by_id::<Vehicle>(&id).unwrap().state.motor =
            sim_core::spawn_vehicle([slot[0], 0.0, slot[2] + 12.0], 0.0).unwrap();
        let brain = cars.get_mut_by_id::<Traffic>(&id).unwrap();
        brain.born = 0;
        brain.progress_at = 900;
        for tick in 901..=1500 {
            collision
                .set_vehicle_proxies(
                    &cars
                        .view::<Vehicle>()
                        .values()
                        .map(crate::systems::vehicle::proxy)
                        .collect::<Vec<_>>(),
                )
                .unwrap();
            pop.decide(tick, &mut cars, &[], &c.manifest, &mut collision)
                .unwrap();
            let car = cars.view::<Vehicle>().get(&id).unwrap();
            let input = pop
                .vehicle_input(id, tick, car.state.motor.speed.0, &cars)
                .unwrap_or_else(|| sim_core::neutral_input(tick, car.state.motor.yaw));
            let available = pop.can_enter(id, &cars);
            let car = cars.get_mut_by_id::<Vehicle>(&id).unwrap();
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
            if available {
                assert!(distance(car.state.motor.position.0.map(|v| v.0), slot) < 2.0);
                assert!(car.state.motor.speed.0.abs() < 0.3);
                assert_eq!(
                    cars.values::<crate::components::PoolMembership>()[&id],
                    VehiclePool::Traffic
                );
                return;
            }
        }
        panic!(
            "failed to park: {:?}, phase {:?}",
            cars.view::<Vehicle>()[&id].state.motor,
            cars.view::<Traffic>()[&id].phase
        );
    }
    #[test]
    fn same_seed_and_observers_produce_identical_population_and_motion() {
        let c = content();
        let (config, mut a_collision) = collision(&c);
        let (_, mut b_collision) = collision(&c);
        let mut a = population(&c);
        let mut b = population(&c);
        let mut a_cars = crate::ecs::EntityStore::new();
        let mut b_cars = crate::ecs::EntityStore::new();
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
                .view::<Vehicle>()
                .iter()
                .map(|(id, v)| (*id, v.state.clone()))
                .collect::<Vec<_>>(),
            b_cars
                .view::<Vehicle>()
                .iter()
                .map(|(id, v)| (*id, v.state.clone()))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            a_cars
                .view::<Pedestrian>()
                .iter()
                .map(|(id, p)| (*id, p.state.clone()))
                .collect::<Vec<_>>(),
            b_cars
                .view::<Pedestrian>()
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
        assert!(
            PopulationSystem::new(
                &c.manifest,
                &c.config,
                1..=16,
                1,
                &mut crate::ecs::EntityStore::new()
            )
            .is_ok()
        );
        assert!(
            PopulationSystem::new(
                &c.manifest,
                &c.config,
                1..=17,
                1,
                &mut crate::ecs::EntityStore::new()
            )
            .is_err()
        );
        let mut invalid = c.manifest.clone();
        let slot = invalid
            .parking
            .iter()
            .find(|p| p.mission_reserved)
            .unwrap()
            .clone();
        invalid.parking.push(slot);
        assert!(
            PopulationSystem::new(
                &invalid,
                &c.config,
                1..=4,
                1,
                &mut crate::ecs::EntityStore::new()
            )
            .is_err()
        );
        let mut p = population(&c);
        let (_, mut collision) = collision(&c);
        let mut cars = crate::ecs::EntityStore::new();
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
        assert!(!cars.view::<Vehicle>().is_empty());
        let id = *cars.view::<Vehicle>().keys().next().unwrap();
        assert!(!p.can_enter(id, &cars));
        cars.get_mut_by_id::<Traffic>(&id).unwrap().phase = TrafficPhase::Available;
        assert!(p.can_enter(id, &cars));
        cars.get_mut_by_id::<Vehicle>(&id).unwrap().state.driver_id = Some(EntityId(1000));
        let tag = cars.values::<crate::components::PoolMembership>()[&id];
        p.direct(1500, &[], &mut cars, &c.manifest, &mut collision, &mut next)
            .unwrap();
        p.direct(3000, &[], &mut cars, &c.manifest, &mut collision, &mut next)
            .unwrap();
        assert!(cars.view::<Vehicle>().contains_key(&id));
        assert_eq!(cars.values::<crate::components::PoolMembership>()[&id], tag);
        assert_eq!(cars.view::<Traffic>().len(), 1);
        assert!(cars.view::<Pedestrian>().is_empty());
    }
    #[test]
    fn seeded_population_traversal_is_bounded_hidden_collision_safe_and_recycles() {
        let c = content();
        let (config, mut collision) = collision(&c);
        let mut pop = population(&c);
        let mut cars = crate::ecs::EntityStore::new();
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
            max_cars = max_cars.max(cars.view::<Vehicle>().len());
            max_peds = max_peds.max(cars.view::<Pedestrian>().len());
            assert!(cars.view::<Vehicle>().len() <= TRAFFIC_CAP);
            assert!(cars.view::<Pedestrian>().len() <= PEDESTRIAN_CAP);
            assert_eq!(
                cars.view::<Vehicle>().len(),
                cars.values::<crate::components::PoolMembership>()
                    .values()
                    .filter(|p| **p == VehiclePool::Traffic)
                    .count()
            );
            for (id, p) in cars
                .view::<Vehicle>()
                .iter()
                .map(|(&id, c)| (id, c.state.motor.position.0.map(|v| v.0)))
                .chain(
                    cars.view::<Pedestrian>()
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
                if cars.view::<Vehicle>().contains_key(&id) && distance(p, initial[&id]) > 20.0 {
                    moved_cars.insert(id);
                }
            }
            for ped in cars.view::<Pedestrian>().values() {
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
        assert!(cars.view::<Vehicle>().is_empty());
        assert!(cars.view::<Pedestrian>().is_empty());
        assert!(
            cars.values::<crate::components::PoolMembership>()
                .is_empty()
        );
    }
    #[test]
    fn sensor_brakes_and_unreachable_graph_stops_without_moving_through_static_geometry() {
        let c = content();
        let (_, mut collision) = collision(&c);
        let mut p = population(&c);
        let mut cars = crate::ecs::EntityStore::new();
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
        let id = *cars.view::<Vehicle>().keys().next().unwrap();
        let car = &cars.view::<Vehicle>()[&id];
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
            .set_vehicle_proxies(
                &cars
                    .view::<Vehicle>()
                    .values()
                    .map(crate::systems::vehicle::proxy)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        p.decide(36, &mut cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(cars.view::<Traffic>()[&id].phase, TrafficPhase::Yielding);
        assert_eq!(p.vehicle_input(id, 37, 0.0, &cars).unwrap().buttons.0, 4);
        p.road.edges.clear();
        cars.get_mut_by_id::<Traffic>(&id).unwrap().route.clear();
        p.decide(54, &mut cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(cars.view::<Traffic>()[&id].phase, TrafficPhase::Stopped);
        assert_eq!(p.vehicle_input(id, 54, 0.0, &cars).unwrap().buttons.0, 4);
    }
    #[test]
    fn pedestrians_use_authored_edges_wait_for_crossings_and_flee_on_graph() {
        let c = content();
        let (_, mut collision) = collision(&c);
        let mut p = population(&c);
        let mut cars = crate::ecs::EntityStore::new();
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
        let id = *cars.view::<Pedestrian>().keys().next().unwrap();
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
        let ped = cars.get_mut_by_id::<Pedestrian>(&id).unwrap();
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
        cars.clear::<Vehicle>();
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
                template: template.into(),
                idle_ticks: 30,
            },
        );
        cars.clear::<Traffic>();
        p.decide(36, &mut cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(
            cars.view::<Pedestrian>()[&id].phase,
            PedestrianPhase::Waiting
        );
        assert_eq!(cars.view::<Pedestrian>()[&id].input.move_z.0, 0);
        cars.clear::<Vehicle>();
        p.decide(42, &mut cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(
            cars.view::<Pedestrian>()[&id].phase,
            PedestrianPhase::Walking
        );
        let position = cars.view::<Pedestrian>()[&id]
            .state
            .motor
            .position
            .0
            .map(|v| v.0);
        p.frighten(position, 42, &mut cars);
        p.decide(48, &mut cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(
            cars.view::<Pedestrian>()[&id].phase,
            PedestrianPhase::Fleeing
        );
        p.decide(198, &mut cars, &[], &c.manifest, &mut collision)
            .unwrap();
        assert_eq!(
            cars.view::<Pedestrian>()[&id].phase,
            PedestrianPhase::Walking
        );
        let ped = &cars.view::<Pedestrian>()[&id];
        assert!(
            p.sidewalk.edges[&ped.from]
                .iter()
                .any(|(to, _, _)| *to == ped.to)
        );
    }
}

pub(crate) fn hear_noise(
    mut messages: bevy_ecs::prelude::MessageReader<crate::gameplay_messages::NoiseEmitted>,
    population: bevy_ecs::prelude::Res<PopulationSystem>,
    mut pedestrians: bevy_ecs::prelude::Query<(&crate::ecs::GameId, &mut Pedestrian)>,
) {
    if messages.is_empty() {
        return;
    }
    let mut actors: Vec<_> = pedestrians.iter_mut().collect();
    actors.sort_by_key(|(id, _)| id.0.0);
    for message in messages.read() {
        for (_, ped) in &mut actors {
            population.react_to_noise(ped, message.position, message.tick);
        }
    }
}

/// Restoration releases AI ownership and intersection claims through an ECS message.
pub(crate) fn release_restored_vehicles(
    mut messages: bevy_ecs::prelude::MessageReader<crate::gameplay_messages::VehicleRestored>,
    mut population: bevy_ecs::prelude::ResMut<PopulationSystem>,
    mut vehicles: bevy_ecs::prelude::Query<
        (&crate::ecs::GameId, Option<&mut Traffic>),
        bevy_ecs::prelude::With<Vehicle>,
    >,
    clock: bevy_ecs::prelude::Res<crate::ecs::state::Clock>,
) {
    for message in messages.read() {
        if message.tick != clock.0 {
            continue;
        }
        let Ok((id, traffic)) = vehicles.get_mut(message.entity) else {
            continue;
        };
        if id.0 != message.vehicle {
            continue;
        }
        if let Some(mut traffic) = traffic {
            traffic.phase = TrafficPhase::Available;
        }
        population.intersections.release(message.vehicle.0);
    }
}

pub(crate) mod input;
pub use crate::components::Traffic;
