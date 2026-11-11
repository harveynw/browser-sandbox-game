#[cfg(test)]
use crate::systems::population::PopulationSystem;
#[cfg(test)]
use crate::world::World;
pub(crate) mod driving;
pub(crate) mod native;
pub(crate) mod perception;
use crate::entities::pedestrian::Pedestrian as PedestrianComponent;
use crate::entities::player::Player as PlayerComponent;
pub(crate) mod officers;
use crate::entities::vehicle::Vehicle;
use crate::sessions::{SessionPhase, Sessions};
use crate::systems::population::{Navigation, hidden};
#[cfg(test)]
use crate::systems::{combat::CombatSystem, population::Observer};
pub(crate) use officers::Officer;
pub(crate) use officers::OfficerShot;
use protocol::*;
use sim_core::CollisionWorld;
use sim_core::{CollisionQueries, QueryFilter};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[cfg(test)]
use std::result::Result;
pub const POLICE_CAP: usize = 8;
const PURSUIT_SPEED: f32 = 29.0;
const INTERCEPT_TRAVEL_SPEED: f32 = 21.0;
const PATROL_HOMES: [[f32; 3]; 4] = [
    [-192.0, 0.0, 160.0], // Old Quay
    [0.0, 0.0, -160.0],   // Civic Center
    [192.0, 0.0, -160.0], // Ironworks
    [192.0, 0.0, 160.0],  // East Docks
];
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
    Officer,
}
const STAR_POINTS: [u8; 3] = [1, 5, 10];
struct ResponseProfile {
    units: u8,
    search_ticks: u32,
    warn_before_engagement: bool,
}

const RESPONSE: [ResponseProfile; 3] = [
    ResponseProfile {
        units: 1,
        search_ticks: 600,
        warn_before_engagement: false,
    },
    ResponseProfile {
        units: 2,
        search_ticks: 1050,
        warn_before_engagement: true,
    },
    ResponseProfile {
        units: 3,
        search_ticks: 1500,
        warn_before_engagement: true,
    },
];

impl CrimeKind {
    fn points(self) -> u8 {
        match self {
            Self::Theft | Self::Shot => 1,
            Self::Harm => 5,
            Self::PoliceCar(_) | Self::Officer => 10,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CrimeReport {
    kind: CrimeKind,
    position: [f32; 3],
    confirmed: bool,
}

#[derive(bevy_ecs::prelude::Component, Clone)]
pub struct Wanted {
    pub heat: u8,
    pub pursuit: Pursuit,
    pub last_known: [f32; 3],
    pub last_seen: u32,
    vehicle_sighting: Option<VehicleSighting>,
    search_since: Option<u32>,
    points: u8,
    last_police_attack: Option<u32>,
    last_gunfire: Option<u32>,
    next_officer_shot: u32,
    fire_authorized: bool,
    last_noise_increase: Option<u32>,
    arrest_ticks: u32,
    pub(crate) arrest_pending: bool,
    pub(crate) surrendering: bool,
    seen: VecDeque<CrimeId>,
}
impl Default for Wanted {
    fn default() -> Self {
        Self {
            heat: 0,
            pursuit: Pursuit::Clear,
            last_known: [0.0; 3],
            last_seen: 0,
            vehicle_sighting: None,
            search_since: None,
            points: 0,
            last_police_attack: None,
            last_gunfire: None,
            next_officer_shot: 0,
            fire_authorized: false,
            last_noise_increase: None,
            arrest_ticks: 0,
            arrest_pending: false,
            surrendering: false,
            seen: VecDeque::new(),
        }
    }
}
impl Wanted {
    fn observe(&mut self, event: CrimeId, tick: u32, report: CrimeReport) {
        if self.seen.contains(&event) {
            return;
        }
        self.seen.push_back(event);
        if self.seen.len() > 64 {
            self.seen.pop_front();
        }
        if matches!(event, CrimeId::Shot(_))
            && matches!(report.kind, CrimeKind::PoliceCar(_) | CrimeKind::Officer)
        {
            self.last_police_attack = Some(tick);
            self.fire_authorized = true;
        }
        if matches!(event, CrimeId::Shot(_)) {
            self.last_gunfire = Some(tick);
        }
        let noise = matches!(report.kind, CrimeKind::Shot);
        if !noise
            || self
                .last_noise_increase
                .is_none_or(|t| tick.saturating_sub(t) >= 30)
        {
            let ceiling = match report.kind {
                CrimeKind::Shot => STAR_POINTS[1] - 1,
                CrimeKind::Theft | CrimeKind::Harm => STAR_POINTS[2] - 1,
                CrimeKind::PoliceCar(_) | CrimeKind::Officer => STAR_POINTS[2],
            }
            .max(self.points);
            self.points = self
                .points
                .saturating_add(report.kind.points())
                .min(ceiling);
            self.heat = STAR_POINTS
                .iter()
                .filter(|&&threshold| self.points >= threshold)
                .count() as u8;
            if noise {
                self.last_noise_increase = Some(tick);
            }
        }
        // A noise report opens an investigation but cannot refresh visual contact.
        if report.confirmed || self.pursuit == Pursuit::Clear {
            self.last_seen = tick;
            self.last_known = report.position;
            self.search_since = None;
            self.pursuit = Pursuit::Pursuing;
        }
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
        if self.search_since.is_some_and(|t| {
            tick.saturating_sub(t) >= RESPONSE[usize::from(self.heat - 1)].search_ticks
        }) {
            self.heat -= 1;
            self.points = if self.heat == 0 {
                0
            } else {
                STAR_POINTS[usize::from(self.heat - 1)]
            };
            self.search_since = Some(tick);
            if self.heat == 0 {
                self.last_police_attack = None;
                self.fire_authorized = false;
                self.last_gunfire = None;
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
#[derive(Clone, Copy)]
struct VehicleSighting {
    id: u32,
    tick: u32,
    position: [f32; 3],
    yaw: f32,
    speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Role {
    #[default]
    Patrol,
    Lead,
    Intercept,
    Contain,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Self::Patrol => "patrol",
            Self::Lead => "lead",
            Self::Intercept => "intercept",
            Self::Contain => "contain",
        }
    }

    fn search_offset(self) -> usize {
        match self {
            Self::Intercept => 1,
            Self::Contain => 3,
            _ => 0,
        }
    }
}

impl VehicleSighting {
    fn prediction(self, tick: u32, role: Role) -> Option<([f32; 3], f32)> {
        let horizon = match role {
            Role::Intercept => 4.0,
            Role::Contain => 6.0,
            _ => return None,
        };
        let age = tick.saturating_sub(self.tick);
        if age > 18 || self.speed < 3.0 {
            return None;
        }
        let travel = (self.speed * horizon).min(105.0);
        Some((
            [
                self.position[0] - self.yaw.sin() * travel,
                self.position[1],
                self.position[2] - self.yaw.cos() * travel,
            ],
            travel / self.speed - age as f32 / 30.0,
        ))
    }
}

fn approach_speed(gap: f32, brake: f32, lead_speed: f32) -> f32 {
    let headway = lead_speed * 0.8;
    let closing_gap = (gap - headway).max(0.0);
    let stopping_limit = (lead_speed.powi(2) + 2.0 * brake * closing_gap).sqrt();
    let following_limit = (lead_speed + (gap - headway) * 1.3).max(0.0);
    stopping_limit.min(following_limit)
}

#[derive(bevy_ecs::prelude::Component)]
pub struct Unit {
    pub target: Option<u32>,
    role: Role,
    intercept_retry: u32,
    intercepting: bool,
    home: [f32; 3],
    route: VecDeque<u32>,
    input: InputFrame,
    target_speed: f32,
    blocked: bool,
    blocked_since: Option<u32>,
    reverse_until: u32,
    retry_after: u32,
    avoided_node: Option<u32>,
    avoid_until: u32,
    route_goal: Option<u32>,
    lane_entry: Option<[f32; 3]>,
    next_plan: u32,
    progress_at: u32,
    progress_position: [f32; 3],
    wreck_until: Option<u32>,
    patrol_leg: usize,
    search_leg: Option<usize>,
}
impl Unit {
    pub(crate) fn release_target(&mut self) {
        self.target = None;
        self.route.clear();
        self.lane_entry = None;
        self.blocked_since = None;
        self.reverse_until = 0;
        self.avoided_node = None;
        self.next_plan = 0;
    }

    pub(crate) fn motor_input(&self, tick: u32, speed: f32) -> InputFrame {
        let mut input = self.input.clone();
        input.tick = tick;
        input.move_z = InputAxis(
            ((self.target_speed - speed) * 2.0)
                .clamp(-1.0, 1.0)
                .mul_add(32767.0, 0.0) as i16,
        );
        input
    }
}
#[derive(bevy_ecs::prelude::Resource)]
pub struct PoliceSystem {
    sidewalk: Navigation,
    road: Navigation,
    enabled: bool,
    initialized: bool,
    cursor: usize,
    crimes: BTreeMap<(u32, CrimeId), CrimeReport>,
    path_jobs: u64,
    assignment_changes: u64,
}
impl PoliceSystem {
    pub fn new(manifest: &content::WorldManifest) -> Self {
        Self {
            sidewalk: Navigation::new(&manifest.pedestrian_graph, true),
            road: Navigation::new(&manifest.road_graph, false),
            // The regression map has no police population.
            enabled: manifest.districts.len() == 6,
            initialized: false,
            cursor: 0,
            crimes: BTreeMap::new(),
            path_jobs: 0,
            assignment_changes: 0,
        }
    }
    pub(crate) fn forget_crimes(&mut self, actor: u32) {
        self.crimes.retain(|(id, _), _| *id != actor);
    }
    #[cfg(test)]
    pub(crate) fn vehicle_input(
        &self,
        id: u32,
        tick: u32,
        speed: f32,
        world: &World,
    ) -> Option<InputFrame> {
        world
            .entities
            .view::<Unit>()
            .get(&id)
            .map(|unit| unit.motor_input(tick, speed))
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

impl PoliceSystem {
    #[cfg(test)]
    pub(crate) fn report_crime(
        &mut self,
        actor: u32,
        cause: CrimeId,
        kind: CrimeKind,
        at: [f32; 3],
        world: &mut World,
    ) {
        if let Some(report) = observe_crime(actor, kind, at, world) {
            self.record_crime(actor, cause, report);
        }
    }
    fn record_crime(&mut self, actor: u32, cause: CrimeId, report: CrimeReport) {
        let key = (actor, cause);
        if self.crimes.len() >= 256 && !self.crimes.contains_key(&key) {
            return;
        }
        self.crimes
            .entry(key)
            .and_modify(|previous| {
                if report.kind.points() > previous.kind.points() {
                    *previous = report;
                }
            })
            .or_insert(report);
    }

    pub(crate) fn diagnostics(
        &self,
        officers: &BTreeMap<u32, &Officer>,
        wanted: &BTreeMap<u32, &Wanted>,
        units: &BTreeMap<u32, &Unit>,
    ) -> serde_json::Value {
        serde_json::json!({
            "path_jobs": self.path_jobs,
            "officers": officers.iter().map(|(id, officer)| serde_json::json!({
                "id": id, "unit": officer.unit, "mounted": officer.mounted,
                "health": officer.state.health.0, "equipment": officer.state.equipment,
            })).collect::<Vec<_>>(),
            "assignment_changes": self.assignment_changes,
            "wanted": wanted.iter().filter(|(_, wanted)| wanted.heat > 0)
                .map(|(id, wanted)| serde_json::json!({
                    "id": id, "stars": wanted.heat, "phase": wanted.pursuit,
                    "last_contact_tick": wanted.last_seen,
                    "last_police_attack_tick": wanted.last_police_attack,
                    "warn_before_engagement": RESPONSE[usize::from(wanted.heat - 1)].warn_before_engagement,
                })).collect::<Vec<_>>(),
            "units": units.iter().map(|(id, unit)| serde_json::json!({
                "id": id, "target": unit.target, "role": unit.role.label(),
                "route_goal": unit.route_goal, "route_nodes": unit.route.len(),
                "target_speed": unit.target_speed, "last_progress_tick": unit.progress_at,
                "blocked": unit.blocked,
            })).collect::<Vec<_>>(),
        })
    }

    // Focused fixtures use the same query implementation as the tick schedule.
    #[cfg(test)]
    pub(crate) fn spawn_patrols(
        &mut self,
        tick: u32,
        observers: &[Observer],
        world: &mut World,
        sessions: &mut Sessions,
        _combat: &mut CombatSystem,
    ) -> Result<(), String> {
        let mut state =
            bevy_ecs::system::SystemState::<native::RosterActors>::new(world.entities.ecs_mut());
        let result = native::roster_actors(
            self,
            tick,
            observers,
            sessions,
            &world.content.manifest,
            &world.collision,
            &mut world.next_entity,
            &mut state
                .get_mut(world.entities.ecs_mut())
                .expect("native actor queries"),
        );
        state.apply(world.entities.ecs_mut());
        result
    }
    #[cfg(test)]
    pub(crate) fn prepare_tick(
        &mut self,
        tick: u32,
        observers: &[Observer],
        world: &mut World,
        sessions: &mut Sessions,
        _population: &mut PopulationSystem,
    ) -> Result<(), String> {
        let mut state =
            bevy_ecs::system::SystemState::<driving::DriveActors>::new(world.entities.ecs_mut());
        let mut actors = state
            .get_mut(world.entities.ecs_mut())
            .expect("native actor queries");
        driving::drive_actors(
            self,
            tick,
            observers,
            sessions,
            &mut world.collision,
            &mut actors,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_chase_can_close_on_a_moving_compact() {
        assert!(approach_speed(28.0, 16.0, 22.0) > 27.0);
        assert!((approach_speed(17.6, 16.0, 22.0) - 22.0).abs() < 0.01);
        assert_eq!(approach_speed(0.0, 16.0, 0.0), 0.0);
    }

    #[test]
    fn support_roles_project_distinct_exits_and_drop_stale_sightings() {
        let sighting = VehicleSighting {
            id: 1,
            tick: 100,
            position: [0.0; 3],
            yaw: 0.0,
            speed: 10.0,
        };
        assert!(sighting.prediction(106, Role::Lead).is_none());
        let (intercept, _) = sighting.prediction(106, Role::Intercept).unwrap();
        let (contain, _) = sighting.prediction(106, Role::Contain).unwrap();
        assert_eq!(intercept, [0.0, 0.0, -40.0]);
        assert_eq!(contain, [0.0, 0.0, -60.0]);
        assert!(sighting.prediction(119, Role::Intercept).is_none());
    }

    #[test]
    fn causal_dedup_cooldown_search_reacquisition_and_decay_are_exact() {
        let mut w = Wanted::default();
        let a = [10.0, 0.0, 20.0];
        let shot = CrimeReport {
            kind: CrimeKind::Shot,
            position: a,
            confirmed: false,
        };
        w.observe(CrimeId::Shot(1), 0, shot);
        for tick in 1..=90 {
            w.observe(CrimeId::Shot(tick + 1), tick, shot);
        }
        assert_eq!((w.heat, w.last_seen, w.last_known), (1, 0, a));
        let harm = CrimeReport {
            kind: CrimeKind::Harm,
            position: a,
            confirmed: true,
        };
        w.observe(CrimeId::Shot(100), 91, harm);
        assert_eq!(w.heat, 2);
        let attack = CrimeReport {
            kind: CrimeKind::PoliceCar(1),
            position: a,
            confirmed: true,
        };
        w.observe(CrimeId::Shot(100), 92, attack);
        assert_eq!(w.heat, 2);
        for id in 110..120 {
            w.observe(CrimeId::Shot(id), 92, harm);
            assert_eq!(w.heat, 2);
        }
        w.observe(CrimeId::Shot(101), 92, attack);
        w.observe(CrimeId::Shot(120), 92, harm);
        assert_eq!(w.heat, 3);
        w.advance(242, None, false);
        assert_eq!(w.pursuit, Pursuit::Searching);
        w.advance(1741, None, false);
        assert_eq!(w.heat, 3);
        w.advance(1742, None, false);
        assert_eq!(w.heat, 2);
        w.advance(1800, Some(a), false);
        assert_eq!(w.pursuit, Pursuit::Pursuing);
        w.advance(1950, None, false);
        w.advance(3000, None, false);
        assert_eq!(w.heat, 1);
        w.advance(3600, None, false);
        assert_eq!((w.heat, w.pursuit), (0, Pursuit::Clear));
        assert!(w.seen.len() <= 64);
    }
    #[test]
    fn arrest_requires_ninety_continuous_ticks_and_resets_on_any_invalid_tick() {
        let mut w = Wanted::default();
        w.observe(
            CrimeId::Shot(1),
            0,
            CrimeReport {
                kind: CrimeKind::Shot,
                position: [0.0; 3],
                confirmed: false,
            },
        );
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
}

#[cfg(test)]
#[path = "tests.rs"]
mod restored_tests;

/// Capture incident-time visibility before later damage, movement or disconnects.
#[cfg(test)]
pub(crate) fn observe_crime(
    actor: u32,
    kind: CrimeKind,
    at: [f32; 3],
    world: &World,
) -> Option<CrimeReport> {
    let player = world.entities.view::<PlayerComponent>().get(&actor)?;
    let actor_position = player
        .state
        .vehicle_id
        .and_then(|id| {
            world
                .entities
                .view::<crate::entities::vehicle::Vehicle>()
                .get(&id.0)
        })
        .map_or(player.state.motor.position.0, |car| {
            car.state.motor.position.0
        })
        .map(|v| v.0);
    observe_crime_snapshot(
        kind,
        at,
        actor_position,
        world
            .entities
            .view::<Unit>()
            .keys()
            .map(|id| {
                let car = &world.entities.view::<crate::entities::vehicle::Vehicle>()[id];
                (
                    *id,
                    car.state.health.0,
                    car.state.motor.position.0.map(|v| v.0),
                )
            })
            .collect::<Vec<_>>()
            .as_slice(),
        &world.collision,
    )
}

/// Shared incident-time evidence rules for legacy and native ECS producers.
pub(crate) fn observe_crime_snapshot(
    kind: CrimeKind,
    at: [f32; 3],
    actor_position: [f32; 3],
    police: &[(u32, u16, [f32; 3])],
    collision: &sim_core::CollisionWorld,
) -> Option<CrimeReport> {
    let witnessed = matches!(kind, CrimeKind::Officer)
        || police.iter().any(|(id, health, p)| {
            if matches!(kind, CrimeKind::PoliceCar(target) if target == *id) {
                return true;
            }
            if *health == 0 {
                return false;
            }
            match kind {
                CrimeKind::Shot => distance(*p, at) <= 80.0,
                CrimeKind::Theft => sight(collision, *p, at, 50.0),
                CrimeKind::Harm => sight(collision, *p, at, 60.0),
                CrimeKind::PoliceCar(_) | CrimeKind::Officer => false,
            }
        });
    if !witnessed {
        return None;
    }
    let confirmed = !matches!(kind, CrimeKind::Shot)
        && police
            .iter()
            .any(|(_, health, p)| *health > 0 && sight(collision, *p, actor_position, 70.0));
    let position = if confirmed {
        actor_position
    } else {
        [at[0], 0.0, at[2]].map(|v| (v / 16.0).round() * 16.0)
    };
    Some(CrimeReport {
        kind,
        position,
        confirmed,
    })
}

pub(crate) fn receive_crimes(
    mut messages: bevy_ecs::prelude::MessageReader<crate::gameplay_messages::CrimeReported>,
    mut police: bevy_ecs::prelude::ResMut<PoliceSystem>,
    actors: bevy_ecs::prelude::Query<&crate::ecs::GameId, bevy_ecs::prelude::With<PlayerComponent>>,
) {
    for message in messages.read() {
        // A disconnect or entity replacement cannot revive a stale report.
        if actors
            .get(message.actor_entity)
            .is_ok_and(|id| id.0 == message.actor)
        {
            police.record_crime(message.actor.0, message.cause, message.report);
        }
    }
}

pub(crate) fn reset_respawned_players(
    mut messages: bevy_ecs::prelude::MessageReader<crate::gameplay_messages::PlayerRespawned>,
    mut actors: bevy_ecs::prelude::Query<(&crate::ecs::GameId, &mut Wanted)>,
    mut units: bevy_ecs::prelude::Query<&mut Unit>,
    mut police: bevy_ecs::prelude::ResMut<PoliceSystem>,
    clock: bevy_ecs::prelude::Res<crate::ecs::state::Clock>,
) {
    for message in messages.read() {
        if message.tick != clock.0 {
            continue;
        }
        let Ok((id, mut wanted)) = actors.get_mut(message.entity) else {
            continue;
        };
        if id.0 != message.player {
            continue;
        }
        *wanted = Default::default();
        for mut unit in &mut units {
            if unit.target != Some(message.player.0) {
                continue;
            }
            unit.target = None;
            unit.route.clear();
            unit.lane_entry = None;
            unit.blocked_since = None;
            unit.reverse_until = 0;
            unit.avoided_node = None;
            unit.next_plan = 0;
        }
        police.crimes.retain(|(id, _), _| *id != message.player.0);
    }
}
