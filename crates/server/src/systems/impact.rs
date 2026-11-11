//! Native impact calculation and health systems; control/replication consequences follow.
use crate::{
    components::{Pedestrian, Vehicle},
    ecs::runtime::Scratch,
    ecs::state::Clock,
    ecs::{GameId, TickOutcome},
    gameplay_messages::{PedestrianImpactDamage, VehicleContactDamage, VehicleImpact},
};
use bevy_ecs::prelude::*;
use std::collections::BTreeMap;

/// Persistent collision-pair suppression, not a queue of work for another system.
#[derive(Resource, Default)]
pub(crate) struct ImpactCooldowns(BTreeMap<(u32, u32, bool), u32>);

pub(crate) fn calculate_vehicle_damage(
    mut impacts: MessageReader<VehicleImpact>,
    mut damage: MessageWriter<VehicleContactDamage>,
    vehicles: Query<&GameId, With<Vehicle>>,
    scratch: Res<Scratch>,
    mut clock: ResMut<Clock>,
    mut cooldowns: ResMut<ImpactCooldowns>,
    mut outcome: ResMut<TickOutcome>,
) {
    let Some(frame) = scratch.frame.as_ref() else {
        outcome.fail("missing movement frame".into());
        return;
    };
    // Preserve the clock commit after all motors and before damage consequences.
    clock.0 = frame.next;
    cooldowns.0.retain(|_, until| *until > clock.0);
    for impact in impacts.read() {
        if impact.tick != clock.0
            || !vehicles
                .get(impact.entity)
                .is_ok_and(|id| id.0 == impact.vehicle)
        {
            continue;
        }
        for contact in &impact.contacts {
            let key = (impact.vehicle.0, contact.collider_id, contact.vehicle);
            let amount = ((impact.delta_speed - 6.0) * 5.0).clamp(0.0, 50.0) as u16;
            if amount == 0 || cooldowns.0.contains_key(&key) {
                continue;
            }
            cooldowns.0.insert(key, clock.0.saturating_add(15));
            if contact.vehicle {
                cooldowns.0.insert(
                    (contact.collider_id, impact.vehicle.0, true),
                    clock.0.saturating_add(15),
                );
            }
            damage.write(VehicleContactDamage {
                vehicle: impact.vehicle,
                entity: impact.entity,
                tick: clock.0,
                contact: contact.clone(),
                damage: amount,
            });
        }
    }
}

pub(crate) fn calculate_pedestrian_damage(
    vehicles: Query<(Entity, &GameId, &Vehicle)>,
    pedestrians: Query<(Entity, &GameId, &Pedestrian)>,
    scratch: Res<Scratch>,
    clock: Res<Clock>,
    mut cooldowns: ResMut<ImpactCooldowns>,
    mut damage: MessageWriter<PedestrianImpactDamage>,
    mut outcome: ResMut<TickOutcome>,
) {
    let Some(frame) = scratch.frame.as_ref() else {
        outcome.fail("missing movement frame".into());
        return;
    };
    let cars: BTreeMap<_, _> = vehicles
        .iter()
        .map(|(entity, id, car)| (id.0.0, (entity, car)))
        .collect();
    let peds: BTreeMap<_, _> = pedestrians
        .iter()
        .map(|(entity, id, ped)| (id.0.0, (entity, ped)))
        .collect();
    // This pass MUST follow vehicle damage: disabled cars no longer sweep pedestrians.
    // It runs even when the motor emitted no contact reports.
    for (&id, before) in &frame.previous_cars {
        let Some(&(car_entity, car)) = cars.get(&id) else {
            continue;
        };
        let speed = before.speed.0.abs().max(car.state.motor.speed.0.abs());
        if speed <= 4.0 || car.state.disabled {
            continue;
        }
        for (&ped, &(pedestrian_entity, p)) in &peds {
            if p.state.health.0 == 0 || cooldowns.0.contains_key(&(id, ped, true)) {
                continue;
            }
            let from = frame
                .previous_peds
                .get(&ped)
                .copied()
                .unwrap_or_else(|| p.state.motor.position.0.map(|v| v.0));
            if sim_core::collision::vehicle_hits_character(
                before.position.0.map(|v| v.0),
                car.state.motor.position.0.map(|v| v.0),
                [before.yaw.0, car.state.motor.yaw.0],
                car.template.dimensions.map(|v| v as f32),
                from,
                p.state.motor.position.0.map(|v| v.0),
            ) {
                cooldowns
                    .0
                    .insert((id, ped, true), clock.0.saturating_add(15));
                damage.write(PedestrianImpactDamage {
                    car: protocol::EntityId(id),
                    car_entity,
                    pedestrian: protocol::EntityId(ped),
                    pedestrian_entity,
                    tick: clock.0,
                    damage: (speed * 8.0).min(100.0) as u8,
                });
            }
        }
    }
}

#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct DamageActors<'w, 's> {
    cars: Query<
        'w,
        's,
        (
            &'static GameId,
            &'static mut Vehicle,
            Option<&'static crate::systems::police::Unit>,
        ),
    >,
    pedestrians: Query<'w, 's, (&'static GameId, &'static mut Pedestrian)>,
    players: Query<'w, 's, &'static crate::components::Player>,
    index: Res<'w, crate::ecs::EntityIndex>,
    collision: NonSend<'w, crate::ecs::state::Collision>,
    clock: Res<'w, Clock>,
    commands: Commands<'w, 's>,
}
impl DamageActors<'_, '_> {
    fn crime(
        &self,
        actor: protocol::EntityId,
        cause: crate::systems::police::CrimeId,
        kind: crate::systems::police::CrimeKind,
        at: [f32; 3],
    ) -> Option<crate::gameplay_messages::CrimeReported> {
        let entity = *self.index.0.get(&actor.0)?;
        let player = self.players.get(entity).ok()?;
        let position = player
            .state
            .vehicle_id
            .and_then(|id| self.index.0.get(&id.0))
            .and_then(|e| self.cars.get(*e).ok())
            .map_or(player.state.motor.position.0, |(_, car, _)| {
                car.state.motor.position.0
            })
            .map(|v| v.0);
        let police: Vec<_> = self
            .cars
            .iter()
            .filter(|(_, _, unit)| unit.is_some())
            .map(|(id, car, _)| {
                (
                    id.0.0,
                    car.state.health.0,
                    car.state.motor.position.0.map(|v| v.0),
                )
            })
            .collect();
        let report = crate::systems::police::observe_crime_snapshot(
            kind,
            at,
            position,
            &police,
            &self.collision.0,
        )?;
        Some(crate::gameplay_messages::CrimeReported {
            actor,
            actor_entity: entity,
            cause,
            report,
        })
    }
    fn damage_car(
        &mut self,
        entity: Entity,
        amount: u16,
        attacker: Option<protocol::EntityId>,
    ) -> Option<([f32; 3], bool)> {
        let (_, mut car, police) = self.cars.get_mut(entity).ok()?;
        if !crate::systems::damage::vehicle_damage(&mut car, amount, attacker) {
            return None;
        }
        if car.state.health.0 == 0 {
            self.commands
                .entity(entity)
                .insert_if_new(crate::components::DisabledVehicle((
                    self.clock.0.saturating_add(900),
                    0,
                )));
        }
        Some((car.state.motor.position.0.map(|v| v.0), police.is_some()))
    }
}

pub(crate) fn apply_vehicle_damage(
    mut messages: MessageReader<VehicleContactDamage>,
    mut actors: DamageActors,
    mut applied: MessageWriter<crate::gameplay_messages::ImpactApplied>,
    mut crimes: MessageWriter<crate::gameplay_messages::CrimeReported>,
    mut noises: MessageWriter<crate::gameplay_messages::NoiseEmitted>,
) {
    use crate::systems::police::{CrimeId, CrimeKind};
    for impact in messages.read() {
        if impact.tick != actors.clock.0 {
            continue;
        }
        let Ok((id, car, _)) = actors.cars.get(impact.entity) else {
            continue;
        };
        if id.0 != impact.vehicle {
            continue;
        }
        let own_driver = car.state.driver_id;
        let other = impact
            .contact
            .vehicle
            .then(|| actors.index.0.get(&impact.contact.collider_id).copied())
            .flatten();
        let attacker = other
            .and_then(|e| actors.cars.get(e).ok())
            .and_then(|(_, car, _)| car.state.driver_id);
        if let Some(other) = other
            && let Some((position, true)) = actors.damage_car(other, impact.damage, own_driver)
            && let Some(driver) = own_driver
            && let Some(crime) = actors.crime(
                driver,
                CrimeId::Impact(impact.tick, impact.vehicle.0),
                CrimeKind::PoliceCar(impact.contact.collider_id),
                position,
            )
        {
            crimes.write(crime);
        }
        if let Some((position, police)) = actors.damage_car(impact.entity, impact.damage, attacker)
        {
            if police
                && let Some(driver) = attacker
                && let Some(crime) = actors.crime(
                    driver,
                    CrimeId::Impact(impact.tick, impact.contact.collider_id),
                    CrimeKind::PoliceCar(impact.vehicle.0),
                    position,
                )
            {
                crimes.write(crime);
            }
            applied.write(crate::gameplay_messages::ImpactApplied {
                target: impact.vehicle,
                position,
                severity: f32::from(impact.damage) / 50.0,
                tick: impact.tick,
            });
            noises.write(crate::gameplay_messages::NoiseEmitted {
                position,
                tick: impact.tick,
            });
        }
    }
}

pub(crate) fn apply_pedestrian_damage(
    mut messages: MessageReader<PedestrianImpactDamage>,
    mut actors: DamageActors,
    mut applied: MessageWriter<crate::gameplay_messages::ImpactApplied>,
    mut crimes: MessageWriter<crate::gameplay_messages::CrimeReported>,
    mut noises: MessageWriter<crate::gameplay_messages::NoiseEmitted>,
) {
    use crate::systems::police::{CrimeId, CrimeKind};
    for impact in messages.read() {
        if impact.tick != actors.clock.0 {
            continue;
        }
        let Ok((id, car, _)) = actors.cars.get(impact.car_entity) else {
            continue;
        };
        if id.0 != impact.car {
            continue;
        }
        let driver = car.state.driver_id;
        let Ok((id, mut ped)) = actors.pedestrians.get_mut(impact.pedestrian_entity) else {
            continue;
        };
        if id.0 != impact.pedestrian {
            continue;
        }
        crate::systems::damage::pedestrian_damage(&mut ped, impact.damage);
        let position = ped.state.motor.position.0.map(|v| v.0);
        if ped.state.health.0 == 0 {
            actors
                .commands
                .entity(impact.pedestrian_entity)
                .insert_if_new(crate::components::Incapacitated(
                    impact.tick.saturating_add(300),
                ));
        }
        if let Some(driver) = driver
            && let Some(crime) = actors.crime(
                driver,
                CrimeId::Impact(impact.tick, impact.car.0),
                CrimeKind::Harm,
                position,
            )
        {
            crimes.write(crime);
        }
        applied.write(crate::gameplay_messages::ImpactApplied {
            target: impact.pedestrian,
            position,
            severity: f32::from(impact.damage) / 100.0,
            tick: impact.tick,
        });
        noises.write(crate::gameplay_messages::NoiseEmitted {
            position,
            tick: impact.tick,
        });
    }
}

#[cfg(test)]
mod health_tests {
    use super::*;
    use crate::{
        components::{DisabledVehicle, Incapacitated},
        gameplay_messages::ImpactApplied,
    };
    use protocol::*;

    fn fixture() -> (World, Entity, EntityId) {
        let content = crate::content_loader::WorldContent::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../apps/client/public/content"),
        )
        .unwrap();
        let core = crate::world::World::new(content).unwrap();
        let mut world = crate::ecs::state::install(core);
        let (entity, id) = world
            .query::<(Entity, &GameId)>()
            .iter(&world)
            .find(|(e, _)| world.get::<Vehicle>(*e).is_some())
            .map(|(e, id)| (e, id.0))
            .unwrap();
        (world, entity, id)
    }

    #[test]
    fn native_vehicle_health_preserves_occupant_protection_and_first_cleanup_deadline() {
        let (mut world, entity, id) = fixture();
        let other = world
            .query::<(Entity, &GameId, &Vehicle)>()
            .iter(&world)
            .find(|(e, _, _)| *e != entity)
            .map(|(e, id, _)| (e, id.0))
            .unwrap();
        world.get_mut::<Vehicle>(entity).unwrap().state.driver_id = Some(EntityId(100));
        world.get_mut::<Vehicle>(other.0).unwrap().state.driver_id = Some(EntityId(101));
        let health = world.get::<Vehicle>(entity).unwrap().state.health.0;
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_vehicle_damage);
        let mut hit = VehicleContactDamage {
            vehicle: id,
            entity,
            tick: 0,
            contact: sim_core::Contact {
                vehicle: true,
                collider_id: other.1.0,
                normal: [1.0, 0.0, 0.0],
            },
            damage: 50,
        };
        world.write_message(hit.clone());
        schedule.run(&mut world);
        assert_eq!(world.get::<Vehicle>(entity).unwrap().state.health.0, health);
        assert!(world.resource::<Messages<ImpactApplied>>().is_empty());
        // A static fatal crash still stops an occupied car; control cleanup is later.
        hit.contact.vehicle = false;
        hit.damage = u16::MAX;
        world.write_message(hit.clone());
        schedule.run(&mut world);
        let car = world.get::<Vehicle>(entity).unwrap();
        assert_eq!(car.state.health.0, 0);
        assert!(!car.state.disabled);
        assert_eq!(car.state.motor.speed.0, 0.0);
        assert_eq!(world.get::<DisabledVehicle>(entity).unwrap().0, (900, 0));
        assert_eq!(world.resource::<Messages<ImpactApplied>>().len(), 1);
        schedule.run(&mut world);
        assert_eq!(world.resource::<Messages<ImpactApplied>>().len(), 1);
        world.resource_mut::<Clock>().0 = 20;
        hit.tick = 20;
        world.write_message(hit);
        schedule.run(&mut world);
        assert_eq!(world.get::<DisabledVehicle>(entity).unwrap().0, (900, 0));
        // Unoccupied destroyed cars are immediately disabled, before pedestrian sweeps.
        world.get_mut::<Vehicle>(other.0).unwrap().state.driver_id = None;
        world.write_message(VehicleContactDamage {
            vehicle: other.1,
            entity: other.0,
            tick: 20,
            contact: sim_core::Contact {
                vehicle: false,
                collider_id: 1,
                normal: [0.0; 3],
            },
            damage: u16::MAX,
        });
        schedule.run(&mut world);
        assert!(world.get::<Vehicle>(other.0).unwrap().state.disabled);
    }

    #[test]
    fn native_pedestrian_health_sets_lifecycle_without_extending_deadline() {
        let (mut world, car_entity, car) = fixture();
        let id = EntityId(20000);
        let pedestrian = Pedestrian {
            id,
            state: CharacterState {
                equipment: None,
                motor: sim_core::spawn_character([0.0; 3], 0.0).unwrap(),
                health: CharacterHealth(100),
                mode: CharacterMode::OnFoot,
                vehicle_id: None,
            },
            phase: crate::systems::population::PedestrianPhase::Walking,
            from: 0,
            to: 0,
            input: sim_core::neutral_input(0, Yaw(0.0)),
            wait_until: 0,
            far_since: None,
            progress_at: 0,
            progress_position: [0.0; 3],
            flee_until: 0,
            threat: [0.0; 3],
        };
        let entity = world.spawn((GameId(id), pedestrian)).id();
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_pedestrian_damage);
        let mut hit = PedestrianImpactDamage {
            car,
            car_entity,
            pedestrian: id,
            pedestrian_entity: entity,
            tick: 0,
            damage: 100,
        };
        world.write_message(hit.clone());
        schedule.run(&mut world);
        let ped = world.get::<Pedestrian>(entity).unwrap();
        assert_eq!(ped.state.health.0, 0);
        assert_eq!(ped.state.mode, CharacterMode::Downed);
        assert_eq!(ped.state.motor.velocity, Vec3([Scalar(0.0); 3]));
        assert_eq!(world.get::<Incapacitated>(entity).unwrap().0, 300);
        world.resource_mut::<Clock>().0 = 10;
        hit.tick = 10;
        world.write_message(hit.clone());
        schedule.run(&mut world);
        assert_eq!(world.get::<Incapacitated>(entity).unwrap().0, 300);
        // An old generational handle must not affect a replacement actor.
        world.despawn(entity);
        world.write_message(hit);
        schedule.run(&mut world);
        assert_eq!(world.resource::<Messages<ImpactApplied>>().len(), 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{content_loader::WorldContent, systems::movement::MovementFrame};
    use std::path::Path;

    #[test]
    fn calculation_needs_no_managers_preserves_health_and_enforces_pair_expiry() {
        let content = WorldContent::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
        )
        .unwrap();
        let core = crate::world::World::new(content).unwrap();
        let mut world = crate::ecs::state::install(core);
        world.init_resource::<Scratch>();
        world.init_resource::<TickOutcome>();
        world.resource_mut::<Scratch>().frame = Some(MovementFrame {
            next: 1,
            failed: vec![],
            owners: BTreeMap::new(),
            actor_inputs: BTreeMap::new(),
            previous_cars: BTreeMap::new(),
            previous_peds: BTreeMap::new(),
        });
        let cars: Vec<_> = world
            .query::<(Entity, &GameId, &Vehicle)>()
            .iter(&world)
            .map(|(e, id, v)| (e, id.0, v.state.health.0))
            .take(2)
            .collect();
        assert_eq!(cars.len(), 2);
        let report = |index: usize, tick, speed| VehicleImpact {
            vehicle: cars[index].1,
            entity: cars[index].0,
            tick,
            delta_speed: speed,
            contacts: vec![sim_core::Contact {
                vehicle: true,
                collider_id: cars[1 - index].1.0,
                normal: [1.0, 0.0, 0.0],
            }],
        };
        let mut schedule = Schedule::default();
        schedule.add_systems(calculate_vehicle_damage);
        // A threshold hit and a stale timestamp must not consume the pair cooldown.
        world.write_message(report(0, 1, 6.0));
        world.write_message(report(0, 0, 30.0));
        world.write_message(report(0, 1, 10.0));
        world.write_message(report(1, 1, 10.0));
        schedule.run(&mut world);
        let mut cursor = bevy_ecs::message::MessageCursor::<VehicleContactDamage>::default();
        let output: Vec<_> = cursor
            .read(world.resource::<Messages<VehicleContactDamage>>())
            .collect();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].damage, 20);
        for (entity, _, health) in &cars {
            assert_eq!(
                world.get::<Vehicle>(*entity).unwrap().state.health.0,
                *health
            );
        }
        for (tick, expected) in [(15, 0), (16, 1)] {
            crate::gameplay_messages::clear(&mut world);
            world.resource_mut::<Scratch>().frame.as_mut().unwrap().next = tick;
            world.write_message(report(1, tick, 30.0));
            schedule.run(&mut world);
            let output: Vec<_> = cursor
                .read(world.resource::<Messages<VehicleContactDamage>>())
                .collect();
            assert_eq!(output.len(), expected);
            if expected == 1 {
                assert_eq!(output[0].damage, 50);
            }
        }
        // Same game ID on a new generational entity must not accept an old report.
        crate::gameplay_messages::clear(&mut world);
        world.resource_mut::<Scratch>().frame.as_mut().unwrap().next = 31;
        let car = world.get::<Vehicle>(cars[0].0).unwrap().clone();
        world.despawn(cars[0].0);
        world.spawn((GameId(cars[0].1), car));
        world.write_message(report(0, 31, 30.0));
        schedule.run(&mut world);
        assert!(
            world
                .resource::<Messages<VehicleContactDamage>>()
                .is_empty()
        );
    }
}
