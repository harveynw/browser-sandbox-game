//! Pedestrian navigation, waiting, crossing and fleeing decisions.
use super::*;
use crate::{ecs::GameId, systems::planning::Planning};
use bevy_ecs::prelude::*;
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct PedestrianActors<'w, 's> {
    cars: Query<'w, 's, (&'static GameId, &'static Vehicle)>,
    peds: Query<'w, 's, (&'static GameId, &'static mut Pedestrian)>,
}
pub(crate) fn decide_actors(
    population: &mut PopulationSystem,
    tick: u32,
    actors: &mut PedestrianActors,
) {
    if !tick.is_multiple_of(6) {
        return;
    }
    let cars: BTreeMap<_, _> = actors.cars.iter().map(|(id, c)| (id.0.0, c)).collect();
    let mut peds: BTreeMap<_, _> = actors
        .peds
        .iter_mut()
        .map(|(id, p)| (id.0.0, p.into_inner()))
        .collect();
    let crossing_cars: Vec<_> = cars
        .values()
        .map(|v| (v.state.motor.position.0.map(|v| v.0), v.state.motor.speed.0))
        .collect();
    let ped_ids: Vec<_> = peds.keys().copied().collect();
    for id in ped_ids {
        let random = population.random();
        let ped = peds.get_mut(&id).expect("pedestrian");
        if ped.phase == PedestrianPhase::Incapacitated {
            ped.input = sim_core::neutral_input(tick, ped.state.motor.yaw);
            continue;
        }
        let p = ped.state.motor.position.0.map(|v| v.0);
        if distance(p, ped.progress_position) >= 1.0 {
            ped.progress_position = p;
            ped.progress_at = tick;
        }
        if distance(p, population.sidewalk.points[&ped.to]) < 0.4 {
            let choices: Vec<_> = population
                .sidewalk
                .edges
                .get(&ped.to)
                .into_iter()
                .flatten()
                .filter(|(n, _, _)| *n != ped.from)
                .collect();
            let choice = if tick < ped.flee_until {
                choices.iter().copied().max_by(|a, b| {
                    distance(population.sidewalk.points[&a.0], ped.threat)
                        .total_cmp(&distance(population.sidewalk.points[&b.0], ped.threat))
                })
            } else {
                choices.get(random % choices.len().max(1)).copied()
            }
            .or_else(|| {
                population
                    .sidewalk
                    .edges
                    .get(&ped.to)
                    .and_then(|e| e.first())
            });
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
        let target = population.sidewalk.points[&ped.to];
        let crossing = population
            .sidewalk
            .edges
            .get(&ped.from)
            .into_iter()
            .flatten()
            .any(|(n, _, kind)| *n == ped.to && *kind == EdgeKind::Crossing);
        let blocked = crossing
            && distance(p, population.sidewalk.points[&ped.from]) < 0.75
            && crossing_cars
                .iter()
                .any(|&(q, speed)| distance(p, q) < (speed.abs() * 2.0 + 6.0).max(8.0));
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
                    population.walk_axis
                })
                .min(1.0)
                .mul_add(32767.0, 0.0) as i16,
            );
        }
        ped.input = input;
    }
}
pub(crate) fn decide(
    mut population: ResMut<PopulationSystem>,
    planning: Res<Planning>,
    mut actors: PedestrianActors,
) {
    decide_actors(&mut population, planning.tick, &mut actors);
}
