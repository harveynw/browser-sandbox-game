//! Typed world resources. Owned install/unpack helpers exist only for fixtures.
use crate::content_loader::WorldContent;
#[cfg(test)]
use crate::{ecs::EntityStore, sessions::Sessions, systems::Systems, world::World as Core};
use bevy_ecs::prelude::*;

#[derive(Resource)]
pub struct Clock(pub u32);
#[derive(Resource)]
pub struct WorldIdentity(pub String);
#[derive(Resource)]
pub struct EntityAllocator(pub u32);
#[derive(Resource)]
pub struct Content(pub WorldContent);
#[derive(Resource)]
pub struct SimulationConfig(pub sim_core::SimConfig);
pub struct Collision(pub sim_core::CollisionWorld);

#[cfg(test)]
pub(crate) fn install(core: Core) -> World {
    let Core {
        entities,
        tick,
        id,
        next_entity,
        content,
        config,
        collision,
        metrics,
        timings,
    } = core;
    let mut world = entities.into_world();
    world.insert_resource(Clock(tick));
    world.insert_resource(WorldIdentity(id));
    world.insert_resource(EntityAllocator(next_entity));
    world.insert_resource(Content(content));
    world.insert_resource(SimulationConfig(config));
    world.insert_non_send(Collision(collision));
    world.insert_resource(metrics);
    world.insert_resource(timings);
    world
}

#[cfg(test)]
pub(crate) fn install_systems(world: &mut World, systems: Systems) {
    let Systems {
        chat,
        movement,
        combat,
        missions,
        police,
        population,
        repairs,
    } = systems;
    world.insert_resource(chat);
    world.insert_resource(movement);
    world.insert_resource(combat);
    world.insert_resource(missions);
    world.insert_resource(police);
    world.insert_resource(population);
    world.insert_resource(repairs);
}
#[cfg(test)]
fn take_systems(world: &mut World) -> Systems {
    Systems {
        chat: world.remove_resource().expect("chat state"),
        movement: world.remove_resource().expect("movement state"),
        combat: world.remove_resource().expect("combat state"),
        missions: world.remove_resource().expect("mission state"),
        police: world.remove_resource().expect("police state"),
        population: world.remove_resource().expect("population state"),
        repairs: world.remove_resource().expect("repair state"),
    }
}

// Existing scenario tests edit an owned fixture between ticks. No production
// tick takes this path; retain the same Bevy world and Schedule across test steps.
#[cfg(test)]
pub(crate) fn unpack(mut world: World) -> (Core, Sessions, Systems) {
    let sessions = world.remove_resource().unwrap();
    let systems = take_systems(&mut world);
    (unpack_core(world), sessions, systems)
}
#[cfg(test)]
pub(crate) fn unpack_core(mut world: World) -> Core {
    let Clock(tick) = world.remove_resource().unwrap();
    let WorldIdentity(id) = world.remove_resource().unwrap();
    let EntityAllocator(next_entity) = world.remove_resource().unwrap();
    let Content(content) = world.remove_resource().unwrap();
    let SimulationConfig(config) = world.remove_resource().unwrap();
    let Collision(collision) = world.remove_non_send().unwrap();
    let metrics = world.remove_resource().unwrap();
    let timings = world.remove_resource().unwrap();
    Core {
        entities: EntityStore::from_world(world),
        tick,
        id,
        next_entity,
        content,
        config,
        collision,
        metrics,
        timings,
    }
}
