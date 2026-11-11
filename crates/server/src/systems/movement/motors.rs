//! Native ECS motor systems. They require neither the legacy adapter nor subsystem managers.
use crate::{
    ecs::runtime::Scratch,
    ecs::state::{Collision, SimulationConfig},
    ecs::{GameId, TickOutcome, TickPhase},
    entities::{pedestrian::Pedestrian, vehicle::Vehicle},
    gameplay_messages::VehicleImpact,
    performance::WorldTimings,
    systems::{police::Unit, population::Traffic},
};
use bevy_ecs::prelude::*;
use sim_core::{CollisionQueries, StepContext};
use std::time::Instant;

pub(crate) fn begin_movement(mut scratch: ResMut<Scratch>, mut outcome: ResMut<TickOutcome>) {
    if scratch.enter(TickPhase::Movement, &mut outcome) {
        scratch.movement_started = Some(Instant::now());
    }
}
pub(crate) fn move_pedestrians(
    mut pedestrians: Query<(&GameId, &mut Pedestrian)>,
    collision: NonSend<Collision>,
    config: Res<SimulationConfig>,
    mut scratch: ResMut<Scratch>,
    mut outcome: ResMut<TickOutcome>,
) {
    let result = (|| -> Result<(), String> {
        let tick = scratch.frame()?.next;
        let mut actors: Vec<_> = pedestrians.iter_mut().collect();
        actors.sort_by_key(|(id, _)| id.0.0);
        for (_, mut ped) in actors {
            ped.step_motor(tick, &collision.0, &config.0)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        outcome.fail(error);
    }
}

type VehicleMotorQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static GameId,
        &'static mut Vehicle,
        Option<&'static Unit>,
        Option<&'static Traffic>,
    ),
>;

pub(crate) fn move_vehicles(
    mut impacts: MessageWriter<VehicleImpact>,
    mut vehicles: VehicleMotorQuery<'_, '_>,
    mut collision: NonSendMut<Collision>,
    config: Res<SimulationConfig>,
    mut scratch: ResMut<Scratch>,
    mut outcome: ResMut<TickOutcome>,
) {
    let result = (|| -> Result<(), String> {
        let frame = scratch.frame()?;
        let mut actors: Vec<_> = vehicles.iter_mut().collect();
        actors.sort_by_key(|(_, id, _, _, _)| id.0.0);
        for i in 0..actors.len() {
            {
                let (entity, id, car, police, traffic) = &mut actors[i];
                if car.state.disabled || car.state.health.0 == 0 {
                    car.idle_ticks = car.idle_ticks.saturating_add(1);
                    car.stop();
                    continue;
                }
                collision.0.exclude_vehicle(Some(car.id.0));
                // Player input takes precedence over police AI, then ambient AI.
                let input = frame
                    .actor_inputs
                    .get(&car.id.0)
                    .cloned()
                    .or_else(|| {
                        police.map(|unit| unit.motor_input(frame.next, car.state.motor.speed.0))
                    })
                    .or_else(|| {
                        traffic.and_then(|ai| ai.motor_input(frame.next, car.state.motor.speed.0))
                    })
                    .unwrap_or_else(|| sim_core::neutral_input(frame.next, car.state.motor.yaw));
                match sim_core::step_vehicle(
                    &car.state.motor,
                    &input,
                    &car.template,
                    &StepContext {
                        tick: frame.next,
                        collision: &collision.0,
                        config: &config.0,
                    },
                ) {
                    Ok(result) => {
                        impacts.write(VehicleImpact {
                            vehicle: id.0,
                            entity: *entity,
                            tick: frame.next,
                            delta_speed: result.impact_delta_speed,
                            contacts: result.contacts,
                        });
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
                            && let Some(peer) = frame.owners.get(&driver.0)
                        {
                            frame.failed.push(*peer);
                        }
                    }
                }
            }
            // Refresh after EACH car: later game IDs see earlier committed poses.
            collision.0.set_vehicle_proxies(
                &actors
                    .iter()
                    .map(|(_, _, car, _, _)| car.proxy())
                    .collect::<Vec<_>>(),
            )?;
        }
        Ok(())
    })();
    // Restore the collision filter even when proxy validation aborts the phase.
    collision.0.exclude_vehicle(None);
    if let Err(error) = result {
        outcome.fail(error);
    }
}

/// Player motors follow vehicle commits; owners retain deterministic game-ID order.
pub(crate) fn move_players(
    mut players: Query<&mut crate::entities::player::Player>,
    vehicles: Query<&crate::entities::vehicle::Vehicle>,
    index: Res<crate::ecs::EntityIndex>,
    collision: NonSend<crate::ecs::state::Collision>,
    config: Res<crate::ecs::state::SimulationConfig>,
    mut scratch: ResMut<Scratch>,
    mut outcome: ResMut<TickOutcome>,
) {
    use protocol::{CharacterMode, Scalar, Vec3};
    let result = (|| -> Result<(), String> {
        let frame = scratch.frame()?;
        for (id, peer) in std::mem::take(&mut frame.owners) {
            let entity = *index.0.get(&id).ok_or("player missing")?;
            let mut player = players.get_mut(entity).map_err(|_| "player missing")?;
            let vehicle_motor = player
                .state
                .vehicle_id
                .map(|car_id| {
                    let entity = index.0.get(&car_id.0).ok_or("controlled vehicle missing")?;
                    let car = vehicles
                        .get(*entity)
                        .map_err(|_| "controlled vehicle missing")?;
                    let mut motor = sim_core::vehicle::presentation_motor(&car.state.motor);
                    motor.position = Vec3(car.seat().map(Scalar));
                    Ok::<_, String>(motor)
                })
                .transpose()?;
            player.weapon.aiming = false;
            if let Some(motor) = vehicle_motor {
                player.state.motor = motor;
            } else if player.state.health.0 > 0
                && player.state.mode == CharacterMode::OnFoot
                && let Some(input) = frame.actor_inputs.get(&id)
            {
                player.weapon.aiming = input.buttons.0 & 8 != 0;
                player.weapon.aim_pitch = input.look_pitch;
                match sim_core::step_character(
                    &player.state.motor,
                    input,
                    &sim_core::StepContext {
                        tick: frame.next,
                        collision: &collision.0,
                        config: &config.0,
                    },
                ) {
                    Ok(result) => player.state.motor = result.motor,
                    Err(_) => frame.failed.push(peer),
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        outcome.fail(error);
    }
}

/// Recover overlaps only AFTER swept impacts have observed intended pedestrian motion.
pub(crate) fn recover_pedestrians(
    mut pedestrians: Query<(&GameId, &mut Pedestrian)>,
    collision: NonSend<Collision>,
    mut scratch: ResMut<Scratch>,
    mut timings: ResMut<WorldTimings>,
) {
    let mut actors: Vec<_> = pedestrians.iter_mut().collect();
    actors.sort_by_key(|(id, _)| id.0.0);
    for (_, mut ped) in actors {
        if ped.state.health.0 == 0 {
            continue;
        }
        if let Ok(position) = collision
            .0
            .recover_character(ped.state.motor.position.0.map(|v| v.0))
        {
            ped.state.motor.position = protocol::Vec3(position.map(protocol::Scalar));
        }
    }
    if let Some(started) = scratch.movement_started.take() {
        timings.motors.observe(started.elapsed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{content_loader::WorldContent, systems::movement::MovementFrame};
    use std::{collections::BTreeMap, path::Path};

    fn fixture() -> bevy_ecs::world::World {
        let content = WorldContent::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
        )
        .unwrap();
        let mut core = crate::world::World::new(content).unwrap();
        // Deliberately make archetype insertion order the opposite of game-ID order.
        let cars: Vec<_> = core.entities.view::<Vehicle>().values().cloned().collect();
        for car in &cars {
            core.entities.despawn(car.id);
        }
        for car in cars.into_iter().rev() {
            core.entities.insert(car.id.0, car);
        }
        let position = crate::world::spawn_position(&core.content, 0).unwrap();
        core.entities.insert(
            20000,
            Pedestrian {
                id: protocol::EntityId(20000),
                state: protocol::CharacterState {
                    equipment: None,
                    motor: sim_core::spawn_character(position, 0.0).unwrap(),
                    health: protocol::CharacterHealth(100),
                    mode: protocol::CharacterMode::OnFoot,
                    vehicle_id: None,
                },
                phase: crate::systems::population::PedestrianPhase::Walking,
                from: 0,
                to: 0,
                input: sim_core::neutral_input(0, protocol::Yaw(0.0)),
                wait_until: 0,
                far_since: None,
                progress_at: 0,
                progress_position: position,
                flee_until: 0,
                threat: position,
            },
        );
        let mut world = crate::ecs::state::install(core);
        world.init_resource::<Scratch>();
        world.init_resource::<TickOutcome>();
        world.resource_mut::<Scratch>().frame = Some(MovementFrame {
            next: 1,
            failed: Vec::new(),
            owners: BTreeMap::new(),
            actor_inputs: BTreeMap::new(),
            previous_cars: BTreeMap::new(),
            previous_peds: BTreeMap::new(),
        });
        world
    }
    fn native_schedule() -> Schedule {
        let mut schedule = Schedule::default();
        schedule.set_executor(bevy_ecs::schedule::SingleThreadedExecutor::new());
        schedule.add_systems(
            (
                begin_movement,
                move_pedestrians.run_if(crate::ecs::healthy),
                move_vehicles.run_if(crate::ecs::healthy),
                move_players.run_if(crate::ecs::healthy),
                recover_pedestrians.run_if(crate::ecs::healthy),
            )
                .chain(),
        );
        schedule
    }
    #[test]
    fn native_motors_need_no_legacy_managers_and_preserve_game_id_order() {
        let mut world = fixture();
        assert!(!world.contains_resource::<crate::sessions::Sessions>());
        assert!(!world.contains_resource::<crate::systems::population::PopulationSystem>());
        assert!(!world.contains_resource::<crate::systems::police::PoliceSystem>());
        let expected: Vec<_> = world
            .resource::<crate::ecs::EntityIndex>()
            .0
            .iter()
            .filter(|(_, entity)| world.get::<Vehicle>(**entity).is_some())
            .map(|(id, _)| *id)
            .collect();
        assert!(!expected.is_empty());
        native_schedule().run(&mut world);
        world.resource_mut::<TickOutcome>().finish().unwrap();
        let impacts: Vec<_> = world
            .resource::<Messages<VehicleImpact>>()
            .get_cursor()
            .read(world.resource::<Messages<VehicleImpact>>())
            .map(|impact| impact.vehicle.0)
            .collect();
        assert_eq!(impacts, expected);
        // The exclusion filter must be cleared after the last car as well.
        let last_entity = world.resource::<crate::ecs::EntityIndex>().0[expected.last().unwrap()];
        let car = world.get::<Vehicle>(last_entity).unwrap();
        assert!(
            world
                .non_send::<Collision>()
                .0
                .overlaps_vehicle(
                    car.state.motor.position.0.map(|v| v.0),
                    car.state.motor.yaw.0,
                    car.template.dimensions.map(|v| v as f32)
                )
                .unwrap()
        );
    }
    #[test]
    fn impact_readers_are_independent_and_tick_cleanup_discards_unread_impacts() {
        let mut world = fixture();
        let mut schedule = native_schedule();
        let mut damage = bevy_ecs::message::MessageCursor::<VehicleImpact>::default();
        let mut observer = bevy_ecs::message::MessageCursor::<VehicleImpact>::default();
        schedule.run(&mut world);
        let messages = world.resource::<Messages<VehicleImpact>>();
        let count = messages.len();
        assert!(count > 0);
        for impact in damage.read(messages) {
            assert_eq!(impact.tick, 1);
            assert_eq!(
                world.get::<GameId>(impact.entity).unwrap().0,
                impact.vehicle
            );
        }
        assert_eq!(damage.read(messages).count(), 0);
        assert_eq!(observer.read(messages).count(), count);
        // A skipped damage checkpoint must not replay its batch on another tick.
        schedule.run(&mut world);
        crate::gameplay_messages::clear(&mut world);
        assert_eq!(
            damage
                .read(world.resource::<Messages<VehicleImpact>>())
                .count(),
            0
        );
        world.resource_mut::<Scratch>().frame.as_mut().unwrap().next = 2;
        schedule.run(&mut world);
        let fresh: Vec<_> = damage
            .read(world.resource::<Messages<VehicleImpact>>())
            .collect();
        assert_eq!(fresh.len(), count);
        assert!(fresh.iter().all(|impact| impact.tick == 2));
    }

    #[test]
    fn invalid_pedestrian_stops_later_motors_without_touching_vehicles() {
        let mut world = fixture();
        let entity = world.resource::<crate::ecs::EntityIndex>().0[&20000];
        world
            .get_mut::<Pedestrian>(entity)
            .unwrap()
            .state
            .motor
            .position
            .0[0] = protocol::Scalar(f32::NAN);
        let before: BTreeMap<_, _> = world
            .query::<(&GameId, &Vehicle)>()
            .iter(&world)
            .map(|(id, car)| (id.0.0, car.idle_ticks))
            .collect();
        native_schedule().run(&mut world);
        assert!(world.resource_mut::<TickOutcome>().finish().is_err());
        assert!(world.resource::<Messages<VehicleImpact>>().is_empty());
        let after: BTreeMap<_, _> = world
            .query::<(&GameId, &Vehicle)>()
            .iter(&world)
            .map(|(id, car)| (id.0.0, car.idle_ticks))
            .collect();
        assert_eq!(before, after);
    }
}
