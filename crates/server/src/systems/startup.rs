//! Startup entity construction and authored metadata assignment.
use crate::{
    components::Vehicle,
    ecs::{TickOutcome, state::*},
    systems::{self, population::VehiclePool},
};
use bevy_ecs::prelude::*;
use protocol::*;
use sim_core::CollisionQueries;
use std::result::Result;

pub(crate) fn validate_spawns(
    content: &crate::content_loader::WorldContent,
    collision: &sim_core::CollisionWorld,
) -> Result<(), String> {
    for slot in 0..crate::world::MAX_PLAYERS {
        if collision.overlaps_character(
            crate::world::spawn_position(content, slot)?,
            sim_core::QueryFilter::MOVEMENT,
        )? {
            return Err("player spawn obstructed".into());
        }
    }
    Ok(())
}

pub(crate) fn parked(
    content: Res<Content>,
    mut collision: NonSendMut<Collision>,
    mut next: ResMut<EntityAllocator>,
    mut commands: Commands,
    mut outcome: ResMut<TickOutcome>,
) {
    let mut proxies = Vec::new();
    let result = (|| -> Result<(), String> {
        for slot in content
            .0
            .manifest
            .parking
            .iter()
            .filter(|p| !p.mission_reserved)
        {
            let template = content
                .0
                .manifest
                .vehicle_templates
                .iter()
                .find(|t| t.id == slot.template_id)
                .ok_or("parking template missing")?
                .clone();
            sim_core::vehicle::validate_template(&template)?;
            let motor = sim_core::spawn_vehicle(slot.position.map(|v| v as f32), slot.yaw as f32)?;
            if collision.0.overlaps_vehicle(
                motor.position.0.map(|v| v.0),
                motor.yaw.0,
                template.dimensions.map(|v| v as f32),
            )? {
                return Err("car spawn obstructed".into());
            }
            let id = EntityId(next.0);
            next.0 = next.0.checked_add(1).ok_or("entity IDs exhausted")?;
            let car = Vehicle {
                id,
                state: VehicleState {
                    motor,
                    health: VehicleHealth(template.health),
                    driver_id: None,
                    disabled: false,
                },
                template: template.into(),
                idle_ticks: 30,
            };
            proxies.push(crate::systems::vehicle::proxy(&car));
            systems::planning::spawn(&mut commands, id.0, car);
            collision.0.set_vehicle_proxies(&proxies)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        outcome.fail(error);
    }
}
pub(crate) fn tag_parked(
    cars: Query<(Entity, &Vehicle)>,
    content: Res<Content>,
    mut commands: Commands,
) {
    for (entity, car) in &cars {
        commands
            .entity(entity)
            .insert(crate::components::PoolMembership(VehiclePool::Parked));
        if content.0.manifest.parking.iter().any(|slot| {
            slot.secured
                && systems::police::distance(
                    slot.position.map(|v| v as f32),
                    car.state.motor.position.0.map(|v| v.0),
                ) < 0.1
        }) {
            commands
                .entity(entity)
                .insert(crate::components::SecuredVehicle);
        }
    }
}
