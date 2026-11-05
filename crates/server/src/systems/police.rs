mod officers;
use crate::entities::vehicle::Vehicle;
use crate::systems::population::{Navigation, Observer, hidden};
use crate::{
    replication::Replication,
    sessions::{SessionPhase, Sessions},
    systems::{combat::CombatSystem, missions::MissionSystem, population::PopulationSystem},
    world::World,
};
use officers::Officer;
pub(crate) use officers::OfficerShot;
use protocol::*;
use sim_core::CollisionWorld;
use sim_core::{CollisionQueries, QueryFilter};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
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
struct CrimeReport {
    kind: CrimeKind,
    position: [f32; 3],
    confirmed: bool,
}

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
pub struct PoliceSystem {
    pub(crate) wanted: BTreeMap<u32, Wanted>,
    pub units: BTreeMap<u32, Unit>,
    pub(crate) officers: BTreeMap<u32, Officer>,
    sidewalk: Navigation,
    pub(crate) secured: BTreeSet<u32>,
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
            units: BTreeMap::new(),
            officers: BTreeMap::new(),
            sidewalk: Navigation::new(&manifest.pedestrian_graph, true),
            wanted: BTreeMap::new(),
            secured: BTreeSet::new(),
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
    pub fn release(&mut self, actor: u32) {
        for unit in self.units.values_mut().filter(|u| u.target == Some(actor)) {
            unit.target = None;
            unit.route.clear();
            unit.lane_entry = None;
            unit.blocked_since = None;
            unit.reverse_until = 0;
            unit.avoided_node = None;
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

impl PoliceSystem {
    pub(crate) fn report_crime(
        &mut self,
        actor: u32,
        cause: CrimeId,
        kind: CrimeKind,
        at: [f32; 3],
        world: &mut World,
    ) {
        let witnessed = matches!(kind, CrimeKind::Officer)
            || self.units.keys().any(|id| {
                let car = &world.vehicles[id];
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
                    CrimeKind::Theft => sight(&world.collision, p, at, 50.0),
                    CrimeKind::Harm => sight(&world.collision, p, at, 60.0),
                    CrimeKind::PoliceCar(_) | CrimeKind::Officer => false,
                }
            });
        let key = (actor, cause);
        if !witnessed || (self.crimes.len() >= 256 && !self.crimes.contains_key(&key)) {
            return;
        }
        let Some(player) = world.players.get(&actor) else {
            return;
        };
        let actor_position = player
            .state
            .vehicle_id
            .and_then(|id| world.vehicles.get(&id.0))
            .map_or(player.state.motor.position.0, |car| {
                car.state.motor.position.0
            })
            .map(|v| v.0);
        let confirmed = !matches!(kind, CrimeKind::Shot)
            && self.units.keys().any(|id| {
                let car = &world.vehicles[id];
                car.state.health.0 > 0
                    && sight(
                        &world.collision,
                        car.state.motor.position.0.map(|v| v.0),
                        actor_position,
                        70.0,
                    )
            });
        let position = if confirmed {
            actor_position
        } else {
            [at[0], 0.0, at[2]].map(|v| (v / 16.0).round() * 16.0)
        };
        let report = CrimeReport {
            kind,
            position,
            confirmed,
        };
        self.crimes
            .entry(key)
            .and_modify(|previous| {
                if kind.points() > previous.kind.points() {
                    *previous = report;
                }
            })
            .or_insert(report);
    }

    pub(crate) fn diagnostics(&self) -> serde_json::Value {
        serde_json::json!({
            "path_jobs": self.path_jobs,
            "officers": self.officers.iter().map(|(id, officer)| serde_json::json!({
                "id": id, "unit": officer.unit, "mounted": officer.mounted,
                "health": officer.state.health.0, "equipment": officer.state.equipment,
            })).collect::<Vec<_>>(),
            "assignment_changes": self.assignment_changes,
            "wanted": self.wanted.iter().filter(|(_, wanted)| wanted.heat > 0)
                .map(|(id, wanted)| serde_json::json!({
                    "id": id, "stars": wanted.heat, "phase": wanted.pursuit,
                    "last_contact_tick": wanted.last_seen,
                    "last_police_attack_tick": wanted.last_police_attack,
                    "warn_before_engagement": RESPONSE[usize::from(wanted.heat - 1)].warn_before_engagement,
                })).collect::<Vec<_>>(),
            "units": self.units.iter().map(|(id, unit)| serde_json::json!({
                "id": id, "target": unit.target, "role": unit.role.label(),
                "route_goal": unit.route_goal, "route_nodes": unit.route.len(),
                "target_speed": unit.target_speed, "last_progress_tick": unit.progress_at,
                "blocked": unit.blocked,
            })).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn spawn_patrols(
        &mut self,
        tick: u32,
        observers: &[Observer],
        world: &mut World,
        sessions: &mut Sessions,
        combat: &mut CombatSystem,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let initializing = !self.initialized;
        self.initialized = true;
        let wanted: Vec<_> = sessions
            .peers
            .values()
            .filter(|s| s.phase == SessionPhase::Playing)
            .filter_map(|s| s.player.map(|id| (id.0, self.wanted(s.player_id()).heat)))
            .filter(|(id, heat)| {
                *heat > 0
                    && matches!(
                        world.players[id].state.mode,
                        CharacterMode::OnFoot | CharacterMode::Driving
                    )
            })
            .map(|(id, heat)| (id, RESPONSE[usize::from(heat - 1)].units))
            .collect();
        // Rotate the first recipient every five seconds only when demand exceeds
        // supply. Allocate one unit per player before assigning second/third units.
        if tick.is_multiple_of(150)
            && wanted
                .iter()
                .map(|(_, heat)| usize::from(*heat))
                .sum::<usize>()
                > POLICE_CAP
        {
            self.cursor = (self.cursor + 1) % wanted.len();
        }
        let mut assignments = Vec::new();
        for level in 0..3 {
            for offset in 0..wanted.len() {
                let (id, heat) = wanted[(offset + self.cursor) % wanted.len()];
                if heat > level && assignments.len() < POLICE_CAP {
                    assignments.push(id);
                }
            }
        }
        let desired = assignments.len().max(PATROL_HOMES.len());
        let mut remove = Vec::new();
        for (&id, u) in &mut self.units {
            let car = &world.vehicles[&id];
            let p = car.state.motor.position.0.map(|v| v.0);
            if distance(p, u.progress_position) >= 2.0 {
                u.progress_position = p;
                u.progress_at = tick;
            }
            if car.state.health.0 == 0 {
                let until = *u.wreck_until.get_or_insert(tick.saturating_add(90));
                if tick >= until
                    && hidden(p, observers, &world.collision)
                    && self
                        .officers
                        .values()
                        .filter(|officer| officer.unit == id && !officer.mounted)
                        .all(|officer| {
                            hidden(
                                officer.state.motor.position.0.map(|v| v.0),
                                observers,
                                &world.collision,
                            )
                        })
                {
                    remove.push(id);
                }
            }
        }
        for id in remove {
            self.units.remove(&id);
            world.vehicles.remove(&id);
            combat.disabled.remove(&id);
            combat.homes.remove(&id);
        }
        if initializing || tick.is_multiple_of(30) {
            let mut template = world
                .content
                .manifest
                .vehicle_templates
                .iter()
                .find(|t| t.id == "compact")
                .ok_or("police compact template missing")?
                .clone();
            template.top_speed = f64::from(PURSUIT_SPEED);
            template.acceleration = 10.0;
            template.brake = 16.0;
            while self.units.len() < desired {
                let index = self.units.len();
                // Fill uncovered districts before assigning another car to a home.
                let home = *PATROL_HOMES
                    .iter()
                    .min_by_key(|home| {
                        self.units
                            .values()
                            .filter(|unit| unit.home == **home)
                            .count()
                    })
                    .expect("patrol homes");
                let near = assignments
                    .get(index)
                    .and_then(|id| {
                        sessions
                            .peers
                            .values()
                            .find(|s| s.player == Some(EntityId(*id)))
                    })
                    .map_or(home, |s| self.wanted(s.player_id()).last_known);
                let mut candidates = self.road.candidates.clone();
                candidates.sort_by(|a, b| distance(a.2, near).total_cmp(&distance(b.2, near)));
                let spot = candidates.into_iter().find(|(_, _, p, yaw)| {
                    (initializing
                        || (observers.iter().all(|o| distance(*p, o.position) >= 40.0)
                            && hidden(*p, observers, &world.collision)))
                        && distance(*p, near) < 180.0
                        && world
                            .vehicles
                            .values()
                            .all(|v| distance(*p, v.state.motor.position.0.map(|v| v.0)) > 12.0)
                        && !world
                            .collision
                            .overlaps_vehicle(*p, *yaw, template.dimensions.map(|v| v as f32))
                            .unwrap_or(true)
                });
                let Some((_, to, p, yaw)) = spot else {
                    break;
                };
                let id = world.next_entity;
                world.next_entity = id.checked_add(1).ok_or("police entity IDs exhausted")?;
                world.vehicles.insert(
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
                self.units.insert(
                    id,
                    Unit {
                        target: None,
                        role: Role::Patrol,
                        intercept_retry: 0,
                        intercepting: false,
                        home,
                        route: VecDeque::from([to]),
                        input: sim_core::neutral_input(tick, Yaw(yaw)),
                        target_speed: 0.0,
                        blocked: false,
                        blocked_since: None,
                        reverse_until: 0,
                        retry_after: 0,
                        avoided_node: None,
                        avoid_until: 0,
                        route_goal: None,
                        lane_entry: None,
                        next_plan: tick,
                        progress_at: tick,
                        progress_position: p,
                        wreck_until: None,
                        patrol_leg: 0,
                        search_leg: None,
                    },
                );
            }
        }
        // Keep existing assignments whenever the new quota can accommodate them.
        let mut free = Vec::new();
        for (&id, unit) in &self.units {
            if world.vehicles[&id].state.health.0 > 0
                && let Some(index) = assignments
                    .iter()
                    .position(|target| Some(*target) == unit.target)
            {
                assignments.remove(index);
            } else {
                free.push(id);
            }
        }
        let mut pending = assignments.into_iter();
        for id in free {
            let unit = self.units.get_mut(&id).expect("listed police unit");
            let target = if world.vehicles[&id].state.health.0 > 0 {
                pending.next()
            } else {
                None
            };
            if target != unit.target {
                self.assignment_changes += 1;
                unit.target = target;
                unit.route.clear();
                unit.lane_entry = None;
                unit.blocked_since = None;
                unit.reverse_until = 0;
                unit.avoided_node = None;
                unit.search_leg = None;
                unit.intercept_retry = 0;
                unit.next_plan = tick;
                unit.progress_at = tick;
            }
        }
        // Retire surplus units only outside every player's static clear view.
        let surplus: Vec<_> = self
            .units
            .keys()
            .copied()
            .filter(|id| self.units[id].target.is_none())
            .filter(|id| {
                self.officers
                    .values()
                    .filter(|officer| officer.unit == *id && !officer.mounted)
                    .all(|officer| {
                        hidden(
                            officer.state.motor.position.0.map(|v| v.0),
                            observers,
                            &world.collision,
                        )
                    })
            })
            .filter(|id| {
                hidden(
                    world.vehicles[id].state.motor.position.0.map(|v| v.0),
                    observers,
                    &world.collision,
                )
            })
            .take(self.units.len().saturating_sub(desired))
            .collect();
        for id in surplus {
            self.units.remove(&id);
            world.vehicles.remove(&id);
            combat.disabled.remove(&id);
            combat.homes.remove(&id);
        }
        Ok(())
    }
    pub(crate) fn prepare_tick(
        &mut self,
        tick: u32,
        observers: &[Observer],
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
    ) -> Result<(), String> {
        let targets: BTreeMap<_, _> = sessions
            .peers
            .values()
            .filter_map(|s| s.player.map(|id| (id.0, &self.wanted[&s.player_id()])))
            .collect();
        let mut plans_remaining = 2;
        let mut target_counts = BTreeMap::new();
        let mut claimed_goals = BTreeSet::new();
        for (&id, unit) in &mut self.units {
            let car = &world.vehicles[&id];
            if car.state.health.0 == 0 {
                continue;
            }
            if self
                .officers
                .values()
                .any(|officer| officer.unit == id && !officer.mounted && officer.state.health.0 > 0)
            {
                unit.target_speed = 0.0;
                unit.input = sim_core::neutral_input(tick, car.state.motor.yaw);
                unit.input.buttons = Buttons(4);
                continue;
            }
            let role = unit.target.map_or(Role::Patrol, |target| {
                let count = target_counts.entry(target).or_insert(0);
                let role = match *count {
                    0 => Role::Lead,
                    1 => Role::Intercept,
                    _ => Role::Contain,
                };
                *count += 1;
                role
            });
            if unit.role != role {
                unit.role = role;
                unit.route.clear();
                unit.lane_entry = None;
                unit.search_leg = None;
                unit.intercept_retry = 0;
                unit.next_plan = tick;
            }
            let p = car.state.motor.position.0.map(|v| v.0);
            let yaw = car.state.motor.yaw.0;
            let dimensions = car.template.dimensions.map(|v| v as f32);
            if unit
                .lane_entry
                .is_some_and(|point| distance(p, point) < 3.0)
            {
                unit.lane_entry = None;
            }
            let reached_goal =
                unit.route.len() == 1 && distance(p, self.road.points[&unit.route[0]]) < 4.0;
            while unit
                .route
                .front()
                .is_some_and(|n| distance(p, self.road.points[n]) < 4.0)
            {
                unit.route.pop_front();
            }
            let wanted = unit.target.and_then(|id| targets.get(&id)).copied();
            let known = wanted.map(|target| target.last_known);
            let deploy = unit.target.zip(wanted).is_some_and(|(target, wanted)| {
                wanted.heat >= 2
                    && tick.saturating_sub(wanted.last_seen) <= 6
                    && world
                        .players
                        .get(&target)
                        .is_some_and(|player| player.state.mode == CharacterMode::OnFoot)
                    && distance(p, wanted.last_known) <= 14.0
                    && sight(&world.collision, p, wanted.last_known, 14.0)
                    && (self
                        .officers
                        .values()
                        .filter(|officer| officer.unit == id)
                        .count()
                        < 2
                        || self.officers.values().any(|officer| {
                            officer.unit == id && officer.mounted && officer.state.health.0 > 0
                        }))
            });
            if deploy {
                unit.target_speed = 0.0;
                unit.input = sim_core::neutral_input(tick, car.state.motor.yaw);
                unit.input.buttons = Buttons(4);
                continue;
            }
            let searching = wanted.is_some_and(|target| {
                let unseen = tick.saturating_sub(target.last_seen) >= 30;
                unseen
                    && (unit.search_leg.is_some()
                        || target.pursuit == Pursuit::Searching
                        || distance(p, target.last_known) <= 12.0)
            });
            // Start with a search point in the direction the car is already facing.
            const LEGS: [[f32; 3]; 4] = [
                [-48.0, 0.0, -48.0],
                [48.0, 0.0, -48.0],
                [48.0, 0.0, 48.0],
                [-48.0, 0.0, 48.0],
            ];
            if searching != unit.search_leg.is_some() {
                unit.search_leg = searching.then(|| {
                    (0..LEGS.len())
                        .min_by(|&a, &b| {
                            let error = |i| wrap(yaw_to([0.0; 3], LEGS[i]) - yaw).abs();
                            error(a).total_cmp(&error(b))
                        })
                        .map_or(0, |leg| (leg + role.search_offset()) % LEGS.len())
                });
                unit.route.clear();
                unit.lane_entry = None;
                unit.next_plan = tick;
            } else if reached_goal {
                if let Some(leg) = &mut unit.search_leg {
                    *leg = (*leg + 1) % LEGS.len();
                } else if known.is_none() {
                    unit.patrol_leg = (unit.patrol_leg + 1) % LEGS.len();
                }
            }
            world.collision.exclude_vehicle(Some(id));
            // Road routing gets us into the area. Leave the lane once the
            // whole car has a clear approach to the last observed position.
            let prediction = wanted
                .and_then(|target| target.vehicle_sighting)
                .filter(|_| !searching && tick >= unit.intercept_retry)
                .and_then(|sighting| sighting.prediction(tick, role))
                .and_then(|(point, time)| {
                    let node = self.road.nearest(point)?;
                    let position = self.road.points[&node];
                    // Reject a snapped point too far from the projected approach,
                    // or one already covered by another car on this pursuit.
                    (distance(position, point) <= 32.0
                        && distance(p, position) <= INTERCEPT_TRAVEL_SPEED * time
                        && !claimed_goals.contains(&(unit.target, node)))
                    .then_some((node, time))
                });
            if unit.intercepting != prediction.is_some() {
                unit.intercepting = prediction.is_some();
                unit.route.clear();
                unit.lane_entry = None;
                unit.next_plan = tick;
            }
            let holding =
                prediction.is_some_and(|(node, _)| distance(p, self.road.points[&node]) < 4.0);
            if let Some((node, _)) = prediction {
                claimed_goals.insert((unit.target, node));
            }
            let direct = known
                .filter(|target| !searching && prediction.is_none() && distance(p, *target) <= 45.0)
                .filter(|target| {
                    let d = distance(p, *target);
                    let stop = car.template.dimensions[2] as f32 / 2.0 + 2.8;
                    let delta = [
                        (target[0] - p[0]) * (1.0 - stop / d.max(stop)),
                        0.0,
                        (target[2] - p[2]) * (1.0 - stop / d.max(stop)),
                    ];
                    world
                        .collision
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
            // A low-speed nudge is only available to the lead car behind a
            // confirmed occupied vehicle. Every other collider still stops it.
            let contact = wanted
                .filter(|target| target.heat >= 2 && role == Role::Lead && !searching)
                .and_then(|target| target.vehicle_sighting)
                .filter(|sighting| tick.saturating_sub(sighting.tick) <= 6)
                .and_then(|sighting| {
                    let target = world.vehicles.get(&sighting.id)?;
                    let driver = target.state.driver_id?;
                    if Some(driver.0) != unit.target || target.state.disabled {
                        return None;
                    }
                    let motor = &target.state.motor;
                    let position = motor.position.0.map(|v| v.0);
                    let distance_to_target = distance(p, position);
                    let safe_geometry = distance_to_target <= 11.0
                        && sight(&world.collision, p, position, 11.0)
                        && wrap(yaw_to(p, position) - yaw).abs() <= 0.15
                        && wrap(motor.yaw.0 - yaw).abs() <= 0.25
                        && (0.0..=5.0).contains(&motor.speed.0)
                        && (0.0..=5.5).contains(&car.state.motor.speed.0)
                        && car.state.motor.speed.0 - motor.speed.0 <= 3.0;
                    let nearby =
                        |point| distance(p, point) < 10.0 || distance(position, point) < 8.0;
                    let bystander = world.players.iter().any(|(&id, player)| {
                        id != driver.0 && nearby(player.state.motor.position.0.map(|v| v.0))
                    }) || population
                        .pedestrians
                        .values()
                        .any(|pedestrian| nearby(pedestrian.state.motor.position.0.map(|v| v.0)));
                    (safe_geometry && !bystander).then(|| {
                        (
                            sighting.id,
                            position,
                            (motor.speed.0 + 2.5).min(5.5),
                            world
                                .players
                                .get(&driver.0)
                                .map(|player| player.state.motor.position.0.map(|v| v.0)),
                        )
                    })
                });
            let reversing = tick < unit.reverse_until;
            if !reversing
                && !holding
                && direct.is_none()
                && tick >= unit.next_plan
                && plans_remaining > 0
            {
                let goal = match (known, unit.search_leg) {
                    (Some(last), Some(leg)) => std::array::from_fn(|i| last[i] + LEGS[leg][i]),
                    (Some(last), None) => last,
                    (None, _) => std::array::from_fn(|i| unit.home[i] + LEGS[unit.patrol_leg][i]),
                };
                let end = prediction
                    .map(|(node, _)| node)
                    .or_else(|| self.road.nearest(goal));
                if unit.route.is_empty() || end != unit.route_goal {
                    self.path_jobs += 1;
                    plans_remaining -= 1;
                    // Attach to a reachable forward point on a directed lane.
                    let mut candidates: Vec<_> = self
                        .road
                        .candidates
                        .iter()
                        .filter_map(|&(from, to, _, heading)| {
                            let a = self.road.points[&from];
                            let b = self.road.points[&to];
                            let length = distance(a, b);
                            let along = ((p[0] - a[0]) * (b[0] - a[0])
                                + (p[2] - a[2]) * (b[2] - a[2]))
                                / length;
                            let t = ((along + 6.0) / length).clamp(0.0, 1.0);
                            let point = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
                            let gap = distance(p, point);
                            (gap < 35.0).then_some((
                                gap + 6.0 * wrap(heading - yaw).abs(),
                                from,
                                to,
                                point,
                            ))
                        })
                        .collect();
                    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
                    candidates.dedup_by_key(|c| (c.1, c.2));
                    let entry = candidates.into_iter().take(8).find(|&(_, _, _, point)| {
                        let delta = [point[0] - p[0], 0.0, point[2] - p[2]];
                        world
                            .collision
                            .cast_vehicle(p, yaw_to(p, point), dimensions, delta)
                            .is_ok_and(|cast| cast.contacts.is_empty())
                    });
                    if let Some((_, _, start, point)) = entry
                        && let Some(end) = end
                    {
                        let avoided = unit.avoided_node.filter(|_| tick < unit.avoid_until);
                        let route = self.road.path_avoiding(start, end, avoided);
                        let route_distance = route
                            .iter()
                            .fold((p, 0.0), |(previous, total), node| {
                                let point = self.road.points[node];
                                (point, total + distance(previous, point))
                            })
                            .1;
                        if prediction.is_some_and(|(_, time)| {
                            route.is_empty() || route_distance > time * INTERCEPT_TRAVEL_SPEED
                        }) {
                            unit.intercept_retry = tick + 30;
                            unit.route.clear();
                            unit.lane_entry = None;
                        } else if !route.is_empty() {
                            unit.route_goal = Some(end);
                            unit.route = route;
                            unit.lane_entry = (distance(p, point) >= 3.0).then_some(point);
                        }
                    } else if prediction.is_some() {
                        unit.intercept_retry = tick + 30;
                        unit.route.clear();
                        unit.lane_entry = None;
                    }
                }
                unit.next_plan = tick.saturating_add(15 + id % 5);
            }
            let target = contact
                .map(|(_, position, _, _)| position)
                .or(direct)
                .or(unit.lane_entry)
                .or_else(|| unit.route.front().map(|n| self.road.points[n]));
            let mut input = sim_core::neutral_input(tick, car.state.motor.yaw);
            let mut speed = 0.0;
            unit.blocked = false;
            if holding {
                // Cover the predicted exit until the sighting changes.
            } else if reversing {
                speed = -2.0;
            } else if let Some(target) = target {
                let approach = yaw_to(p, target);
                let next = (contact.is_none() && direct.is_none() && unit.lane_entry.is_none())
                    .then(|| unit.route.get(1).map(|id| self.road.points[id]))
                    .flatten();
                let corner = next.map_or(0.0, |point| wrap(yaw_to(target, point) - approach).abs());
                let lookahead = (car.state.motor.speed.0.abs() * 0.6).clamp(4.0, 12.0);
                let aim = next.filter(|_| corner < 0.35).map_or(target, |point| {
                    let t = ((lookahead - distance(p, target)) / distance(target, point).max(0.1))
                        .clamp(0.0, 1.0);
                    std::array::from_fn(|i| target[i] + (point[i] - target[i]) * t)
                });
                let error = wrap(yaw_to(p, aim) - yaw);
                input.move_x =
                    InputAxis((-error * 1.8).clamp(-1.0, 1.0).mul_add(32767.0, 0.0) as i16);
                let pursuing = known.is_some() && !searching;
                let cruise = if pursuing { PURSUIT_SPEED } else { 12.0 };
                let turn_penalty = if pursuing { 3.0 } else { 5.0 };
                let turning = (cruise / (1.0 + turn_penalty * error.abs())).max(2.0);
                let exit_speed = if next.is_some() && corner < 0.35 {
                    cruise
                } else {
                    4.0
                };
                speed = turning.min(
                    (exit_speed.powi(2)
                        + 2.0 * car.template.brake as f32 * (distance(p, target) - 4.0).max(0.0))
                    .sqrt(),
                );
                if direct.is_some() {
                    let gap = (distance(p, target) - dimensions[2] / 2.0 - 2.8).max(0.0);
                    // Follow a moving vehicle using its recent observed motion.
                    // Stationary and on-foot targets still use a stopping approach.
                    let lead_speed = wanted
                        .and_then(|target| target.vehicle_sighting)
                        .filter(|sighting| {
                            tick.saturating_sub(sighting.tick) <= 6
                                && wrap(sighting.yaw - approach).abs() < 0.35
                        })
                        .map_or(0.0, |sighting| sighting.speed.max(0.0));
                    speed = turning.min(approach_speed(gap, car.template.brake as f32, lead_speed));
                }
            }
            let contact = contact.filter(|_| !reversing && !holding);
            if let Some((_, _, approach_speed, _)) = contact {
                speed = approach_speed;
            }
            if speed.abs() > 0.1 || car.state.motor.speed.0.abs() > 0.1 {
                let current_speed = car.state.motor.speed.0;
                let direction = if current_speed.abs() >= 0.25 {
                    current_speed.signum()
                } else if reversing {
                    -1.0
                } else {
                    1.0
                };
                let forward = [-yaw.sin() * direction, 0.0, -yaw.cos() * direction];
                let sensor = (car.state.motor.speed.0.powi(2) / (2.0 * car.template.brake as f32)
                    + 0.1)
                    .max(if reversing { 0.8 } else { 0.2 });
                let cast = world.collision.cast_vehicle(
                    p,
                    car.state.motor.yaw.0,
                    car.template.dimensions.map(|v| v as f32),
                    forward.map(|v| v * sensor),
                )?;
                if cast.contacts.iter().any(|hit| {
                    !hit.vehicle
                        || contact.is_none_or(|(target, _, _, _)| target != hit.collider_id)
                }) {
                    unit.blocked = true;
                    speed = 0.0;
                }
                for actor in observers.iter().map(|o| o.position).chain(
                    population
                        .pedestrians
                        .values()
                        .map(|p| p.state.motor.position.0.map(|v| v.0)),
                ) {
                    if contact.is_some_and(|(_, _, _, driver)| driver == Some(actor)) {
                        continue;
                    }
                    let delta = [actor[0] - p[0], actor[2] - p[2]];
                    let along = delta[0] * forward[0] + delta[1] * forward[2];
                    let across = (delta[0] * forward[2] - delta[1] * forward[0]).abs();
                    if along > 0.0
                        && along < car.template.dimensions[2] as f32 / 2.0 + sensor
                        && across < car.template.dimensions[0] as f32 / 2.0 + 0.6
                    {
                        unit.blocked = true;
                        speed = 0.0;
                    }
                }
            }
            if reversing && unit.blocked && car.state.motor.speed.0 < 0.25 {
                unit.reverse_until = tick;
            }
            let struggling = !holding
                && direct.is_none()
                && (unit.blocked
                    || target.is_none()
                    || target.is_some_and(|target| wrap(yaw_to(p, target) - yaw).abs() > 1.7));
            if !reversing && struggling {
                let since = *unit.blocked_since.get_or_insert(tick);
                if tick.saturating_sub(since) >= 45 && tick >= unit.retry_after {
                    unit.avoided_node = unit.route.front().copied();
                    unit.avoid_until = tick + 300;
                    unit.route.clear();
                    unit.lane_entry = None;
                    unit.next_plan = tick;
                    unit.reverse_until = tick + 45;
                    unit.retry_after = tick + 150;
                    unit.blocked_since = None;
                }
            } else {
                unit.blocked_since = None;
            }
            if speed.abs() <= 0.1 {
                input.buttons = Buttons(4);
            }
            unit.target_speed = speed;
            unit.input = input;
        }
        world.collision.exclude_vehicle(None);
        Ok(())
    }
    pub(crate) fn finish_tick(
        &mut self,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        missions: &mut MissionSystem,
    ) -> Result<(), String> {
        let crimes = std::mem::take(&mut self.crimes);
        for peer in sessions.peers.keys().copied().collect::<Vec<_>>() {
            let mut s = sessions.peers.remove(&peer).expect("police session");
            if let Some(id) = s.player {
                let player = &world.players[&id.0];
                if matches!(
                    player.state.mode,
                    CharacterMode::Downed | CharacterMode::Arrested
                ) || player.state.health.0 == 0
                {
                    if self.wanted(s.player_id()).heat > 0 {
                        *self.wanted_mut(s.player_id()) = Wanted::default();
                        s.status_dirty = true;
                    }
                    self.release(id.0);
                } else {
                    let before = (
                        self.wanted(s.player_id()).heat,
                        self.wanted(s.player_id()).pursuit,
                    );
                    for (&(actor, cause), &report) in &crimes {
                        if actor == id.0 {
                            self.wanted_mut(s.player_id())
                                .observe(cause, world.tick, report);
                        }
                    }
                    let p = player
                        .state
                        .vehicle_id
                        .map(|id| world.vehicles[&id.0].state.motor.position.0.map(|v| v.0))
                        .unwrap_or_else(|| player.state.motor.position.0.map(|v| v.0));
                    let seen = s.phase == SessionPhase::Playing
                        && world.tick.is_multiple_of(6)
                        && (self.officers.values().any(|officer| {
                            !officer.mounted
                                && officer.state.health.0 > 0
                                && sight(
                                    &world.collision,
                                    officer.state.motor.position.0.map(|v| v.0),
                                    p,
                                    45.0,
                                )
                        }) || self.units.keys().any(|unit| {
                            let car = &world.vehicles[unit];
                            car.state.health.0 > 0
                                && sight(
                                    &world.collision,
                                    car.state.motor.position.0.map(|v| v.0),
                                    p,
                                    70.0,
                                )
                        }));
                    if seen {
                        self.wanted_mut(s.player_id()).vehicle_sighting =
                            player.state.vehicle_id.map(|id| {
                                let motor = &world.vehicles[&id.0].state.motor;
                                VehicleSighting {
                                    id: id.0,
                                    tick: world.tick,
                                    position: p,
                                    yaw: motor.yaw.0,
                                    speed: motor.speed.0,
                                }
                            });
                    }
                    let speed = player
                        .state
                        .vehicle_id
                        .map(|id| world.vehicles[&id.0].state.motor.speed.0.abs())
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
                        && (self.officers.values().any(|officer| {
                            self.units
                                .get(&officer.unit)
                                .is_some_and(|unit| unit.target == Some(id.0))
                                && !officer.mounted
                                && officer.state.health.0 > 0
                                && !officer
                                    .state
                                    .equipment
                                    .as_ref()
                                    .is_some_and(|weapon| weapon.aiming)
                                && sight(
                                    &world.collision,
                                    officer.state.motor.position.0.map(|v| v.0),
                                    p,
                                    3.0,
                                )
                        }) || self.units.iter().any(|(unit, u)| {
                            let car = &world.vehicles[unit];
                            let front = sim_core::anchor(
                                &car.state.motor,
                                [0.0, 0.0, -car.template.dimensions[2] / 2.0],
                            );
                            u.target == Some(id.0)
                                && car.state.health.0 > 0
                                && distance(front, p) <= 3.0
                                && sight(&world.collision, front, p, 3.0)
                        }));
                    if self
                        .wanted_mut(s.player_id())
                        .advance(world.tick, seen.then_some(p), arrest)
                    {
                        self.arrest_player(peer, &mut s, world, sessions, population, missions)?;
                    }
                    s.status_dirty |= before
                        != (
                            self.wanted(s.player_id()).heat,
                            self.wanted(s.player_id()).pursuit,
                        );
                }
            }
            sessions.peers.insert(peer, s);
        }
        Ok(())
    }
    pub(crate) fn arrest_player(
        &mut self,
        peer: crate::transport::PeerKey,
        s: &mut crate::sessions::Session,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        missions: &mut MissionSystem,
    ) -> Result<(), String> {
        let id = s.player.ok_or("arrest owner missing")?;
        let car_id = world.players[&id.0].state.vehicle_id;
        let mut position = Some(world.players[&id.0].state.motor.position.0.map(|v| v.0));
        if let Some(car) = car_id {
            world.collision.exclude_vehicle(Some(car.0));
            position = crate::entities::vehicle::safe_exit_with(
                &world.vehicles[&car.0],
                &world.collision,
                |p| CombatSystem::clear_spawn(p, Some(id.0), world, population),
            );
            world.collision.exclude_vehicle(None);
            position = position.or_else(|| {
                CombatSystem::safe_spawn(
                    world.vehicles[&car.0].state.motor.position.0.map(|v| v.0),
                    Some(id.0),
                    world,
                    population,
                )
            });
        }
        let Some(position) = position else {
            // Freeze pending arrest and retry safe ejection; never materialize in a wall.
            if let Some(car) = car_id {
                world.vehicles.get_mut(&car.0).expect("arrest car").stop();
            }
            s.inputs.clear();
            if !self.wanted(s.player_id()).arrest_pending {
                self.wanted_mut(s.player_id()).arrest_pending = true;
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .cancel();
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .lifecycle
                    .protected_until = None;
                if let Some(m) = missions.mission_mut(s.player_id())
                    && m.phase == crate::systems::missions::Phase::Active
                {
                    m.fail(world.tick);
                }
                s.status_dirty = true;
                let affected = [id.0, car_id.expect("blocked driving exit").0];
                Replication::transition(peer, s, &affected, world, sessions, population, self)?;
                Replication::reliable(
                    peer,
                    s.id.as_deref(),
                    ServerMessage::ControlChanged(Replication::owner(s, world)?),
                    world,
                    sessions,
                );
            }
            return Ok(());
        };
        if let Some(car) = car_id {
            let car = world.vehicles.get_mut(&car.0).expect("arrest car");
            car.state.driver_id = None;
            car.stop();
        }
        let player = world.players.get_mut(&id.0).expect("arrest player");
        player.state.mode = CharacterMode::Arrested;
        player.state.vehicle_id = None;
        player.state.motor = sim_core::spawn_character(position, 0.0)?;
        world
            .players
            .get_mut(&s.player_id())
            .expect("player equipment")
            .weapon
            .cancel();
        world
            .players
            .get_mut(&s.player_id())
            .expect("player equipment")
            .lifecycle
            .protected_until = None;
        world
            .players
            .get_mut(&s.player_id())
            .expect("player equipment")
            .lifecycle
            .respawn = Some(world.tick.saturating_add(150));
        *self.wanted_mut(s.player_id()) = Wanted::default();
        self.release(id.0);
        if let Some(m) = missions.mission_mut(s.player_id())
            && m.phase == crate::systems::missions::Phase::Active
        {
            m.fail(world.tick);
        }
        s.status_dirty = true;
        let mut affected = vec![id.0];
        if let Some(car) = car_id {
            affected.push(car.0);
        }
        Replication::transition(peer, s, &affected, world, sessions, population, self)?;
        Replication::reliable(
            peer,
            s.id.as_deref(),
            ServerMessage::ControlChanged(Replication::owner(s, world)?),
            world,
            sessions,
        );
        Ok(())
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

impl PoliceSystem {
    pub(crate) fn wanted(&self, id: u32) -> &Wanted {
        &self.wanted[&id]
    }
    pub(crate) fn wanted_mut(&mut self, id: u32) -> &mut Wanted {
        self.wanted.get_mut(&id).expect("admitted wanted state")
    }
}

#[cfg(test)]
#[path = "police_tests.rs"]
mod restored_tests;
