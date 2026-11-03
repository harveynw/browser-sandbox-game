use crate::transport::*;
use crate::{
    sessions::{SessionPhase, Sessions},
    systems::{
        combat::CombatSystem, missions::MissionSystem, police::PoliceSystem,
        population::PopulationSystem,
    },
    world::World,
};
use protocol::*;
use sim_core::{CollisionQueries, StepContext};
use std::collections::BTreeMap;
use std::result::Result;
/// Inputs and pre-movement positions retained only for the current tick.
pub(crate) struct MovementFrame {
    pub next: u32,
    pub failed: Vec<PeerKey>,
    pub owners: BTreeMap<u32, PeerKey>,
    pub actor_inputs: BTreeMap<u32, InputFrame>,
    pub previous_cars: BTreeMap<u32, VehicleMotor>,
    pub previous_peds: BTreeMap<u32, [f32; 3]>,
    pub impacts: Vec<(u32, f32, Vec<sim_core::collision::Contact>)>,
}
/// Coordinates character and vehicle movement within each tick.
#[derive(Default)]
pub struct MovementSystem;
impl MovementSystem {
    pub(crate) fn prepare_tick(
        &mut self,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        combat: &mut CombatSystem,
        missions: &mut MissionSystem,
    ) -> Result<MovementFrame, String> {
        let next = world.tick + 1;
        let mut failed = Vec::new();
        // Entity order, not network arrival order, determines simulation iteration.
        let owners: BTreeMap<_, _> = sessions
            .peers
            .iter()
            .filter(|(_, s)| s.phase == SessionPhase::Playing)
            .filter_map(|(peer, s)| s.player.map(|id| (id.0, *peer)))
            .collect();
        let mut actor_inputs = BTreeMap::new();
        for (&id, &peer) in &owners {
            let s = sessions.peers.get_mut(&peer).ok_or("missing session")?;
            let player = &world.players[&id];
            let input = s
                .inputs
                .remove(&next)
                .unwrap_or_else(|| sim_core::neutral_input(next, player.state.motor.yaw));
            let retiring = missions.mission(s.player_id()).as_ref().is_some_and(|m| {
                m.phase == crate::systems::missions::Phase::Failed
                    && m.reserved_vehicle == player.state.vehicle_id.map(|v| v.0)
                    && m.reserved_vehicle.is_some()
            });
            let input = if police.wanted(s.player_id()).arrest_pending || retiring {
                let mut neutral = sim_core::neutral_input(next, player.state.motor.yaw);
                neutral.buttons = Buttons(4);
                neutral
            } else {
                input
            };
            s.inputs.retain(|tick, _| *tick > next);
            actor_inputs.insert(player.state.vehicle_id.map_or(id, |v| v.0), input);
        }
        let observers: Vec<_> = sessions
            .peers
            .values()
            .filter(|s| s.phase == SessionPhase::Playing)
            .filter_map(|s| s.player)
            .filter_map(|id| world.players.get(&id.0))
            .map(|p| crate::systems::population::Observer {
                position: p.state.motor.position.0.map(|v| v.0),
                yaw: p.state.motor.yaw.0,
            })
            .collect();
        police.spawn_patrols(next, &observers, world, sessions, combat)?;
        population.direct(
            next,
            &observers,
            &mut world.vehicles,
            &world.content.manifest,
            &mut world.collision,
            &mut world.next_entity,
        )?;
        world.collision.set_vehicle_proxies(
            &world
                .vehicles
                .values()
                .map(|v| v.proxy())
                .collect::<Vec<_>>(),
        )?;
        combat.register_vehicle_homes(world, population);
        let previous_cars: BTreeMap<_, _> = world
            .vehicles
            .iter()
            .map(|(&id, v)| (id, v.state.motor.clone()))
            .collect();
        let previous_peds: BTreeMap<_, _> = population
            .pedestrians
            .iter()
            .map(|(&id, p)| (id, p.state.motor.position.0.map(|v| v.0)))
            .collect();
        let mut impacts = Vec::new();
        population.decide(
            next,
            &world.vehicles,
            &observers,
            &world.content.manifest,
            &mut world.collision,
        )?;
        police.prepare_tick(next, &observers, world, sessions, population)?;
        // Preserve intended pedestrian motion for swept impact queries. Resolving
        // new car overlaps first would erase the contact before damage observes it.
        Ok(MovementFrame {
            next,
            failed,
            owners,
            actor_inputs,
            previous_cars,
            previous_peds,
            impacts,
        })
    }
    pub(crate) fn update(
        &mut self,
        frame: &mut MovementFrame,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        combat: &mut CombatSystem,
        missions: &mut MissionSystem,
    ) -> Result<(), String> {
        let MovementFrame {
            next,
            failed,
            owners,
            actor_inputs,
            previous_cars,
            previous_peds,
            impacts,
        } = frame;
        let next = *next;
        population.step_pedestrians(next, &world.collision, &world.config)?;
        for car_id in world.vehicles.keys().copied().collect::<Vec<_>>() {
            let car = world.vehicles.get_mut(&car_id).ok_or("missing vehicle")?;
            if car.state.disabled || car.state.health.0 == 0 {
                car.idle_ticks = car.idle_ticks.saturating_add(1);
                car.stop();
                continue;
            }
            world.collision.exclude_vehicle(Some(car.id.0));
            let input = actor_inputs
                .get(&car.id.0)
                .cloned()
                .or_else(|| police.vehicle_input(car.id.0, next, car.state.motor.speed.0))
                .or_else(|| population.vehicle_input(car.id.0, next, car.state.motor.speed.0))
                .unwrap_or_else(|| sim_core::neutral_input(next, car.state.motor.yaw));
            match sim_core::step_vehicle(
                &car.state.motor,
                &input,
                &car.template,
                &StepContext {
                    tick: next,
                    collision: &world.collision,
                    config: &world.config,
                },
            ) {
                Ok(result) => {
                    impacts.push((car_id, result.impact_delta_speed, result.contacts));
                    car.state.motor = result.motor;
                    car.idle_ticks = if car.state.motor.speed.0.abs() < 0.1 {
                        car.idle_ticks.saturating_add(1)
                    } else {
                        0
                    };
                }
                Err(_) => {
                    car.stop();
                    if let Some(driver) = car.state.driver_id
                        && let Some(peer) = owners.get(&driver.0)
                    {
                        failed.push(*peer);
                    }
                }
            }
            // Later entity IDs observe earlier cars' committed positions in this tick.
            world.collision.set_vehicle_proxies(
                &world
                    .vehicles
                    .values()
                    .map(|v| v.proxy())
                    .collect::<Vec<_>>(),
            )?;
        }
        world.collision.exclude_vehicle(None);
        for (id, peer) in std::mem::take(owners) {
            let player = world.players.get_mut(&id).ok_or("player missing")?;
            player.weapon.aiming = false;
            if let Some(car_id) = player.state.vehicle_id {
                let car = &world.vehicles[&car_id.0];
                player.state.motor = sim_core::presentation_motor(&car.state.motor);
                player.state.motor.position = Vec3(car.seat().map(Scalar));
            } else if player.state.health.0 > 0
                && player.state.mode == CharacterMode::OnFoot
                && let Some(input) = actor_inputs.get(&id)
            {
                player.weapon.aiming = input.buttons.0 & 8 != 0;
                player.weapon.aim_pitch = input.look_pitch;
                match sim_core::step_character(
                    &player.state.motor,
                    input,
                    &StepContext {
                        tick: next,
                        collision: &world.collision,
                        config: &world.config,
                    },
                ) {
                    Ok(result) => player.state.motor = result.motor,
                    Err(_) => failed.push(peer),
                }
            }
        }
        world.tick = next;
        combat.vehicle_impacts(
            previous_cars,
            std::mem::take(impacts),
            previous_peds,
            world,
            sessions,
            population,
            police,
            missions,
        );
        for ped in population
            .pedestrians
            .values_mut()
            .filter(|p| p.state.health.0 > 0)
        {
            if let Ok(position) = world
                .collision
                .recover_character(ped.state.motor.position.0.map(|v| v.0))
            {
                ped.state.motor.position = Vec3(position.map(Scalar));
            }
        }
        Ok(())
    }
}
