//! Construct the authoritative Bevy world, then commit startup actors through
//! the same indexed lifecycle commands and police roster used by live systems.
use crate::{
    content_loader::WorldContent,
    ecs::state::*,
    ecs::{EntityIndex, TickOutcome},
    entities::vehicle::Vehicle,
    systems::{self, police::PoliceSystem, population::PopulationSystem},
    world::Metrics,
};
use bevy_ecs::prelude::*;
use std::result::Result;

pub(crate) fn base(content: WorldContent) -> Result<World, String> {
    let config = sim_core::SimConfig::from_content(&content.config)?;
    let collision = sim_core::CollisionWorld::from_content(&content.collision, &config)?;
    systems::startup::validate_spawns(&content, &collision)?;
    let mut world = World::new();
    world.init_resource::<EntityIndex>();
    crate::ecs::messages::register(&mut world);
    world.insert_resource(Clock(0));
    world.insert_resource(WorldIdentity(crate::admission::random_id("world-", 16)?));
    world.insert_resource(EntityAllocator(1));
    world.insert_resource(Content(content));
    world.insert_resource(SimulationConfig(config));
    world.insert_non_send(Collision(collision));
    world.init_resource::<Metrics>();
    world.init_resource::<crate::performance::WorldTimings>();
    world.init_resource::<TickOutcome>();
    let mut schedule = Schedule::default();
    schedule.add_systems(systems::startup::parked);
    schedule.run(&mut world);
    world.resource_mut::<TickOutcome>().finish()?;
    Ok(world)
}
pub(crate) fn build(content: WorldContent) -> Result<World, String> {
    let mut world = base(content)?;
    let count = world.query::<&Vehicle>().iter(&world).count();
    let content = &world.resource::<Content>().0;
    let population =
        PopulationSystem::from_content(&content.manifest, &content.config, count, 424242)?;
    let police = PoliceSystem::new(&content.manifest);
    world.insert_resource(population);
    world.insert_resource(police);
    world.init_resource::<crate::sessions::Sessions>();
    world.init_resource::<systems::chat::ChatSystem>();
    world.init_resource::<systems::combat::CombatSystem>();
    world.init_resource::<systems::repairs::RepairSystem>();
    world.init_resource::<systems::planning::Planning>();
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            systems::startup::tag_parked,
            systems::police::native::roster,
        )
            .chain(),
    );
    schedule.run(&mut world);
    world.resource_mut::<TickOutcome>().finish()?;
    Ok(world)
}
