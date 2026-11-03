//! One fixed simulation tick, in gameplay order. Networking is polled/flushed by main.
use crate::{
    admission::Profiles, replication::Replication, sessions::Sessions, systems::Systems,
    transport::Network, world::World,
};
use std::time::Instant;
/// Preserve pre-movement actions, post-movement consequences and replication ordering.
pub fn step(
    world: &mut World,
    sessions: &mut Sessions,
    systems: &mut Systems,
    network: &mut Network,
    profiles: &mut Profiles,
    now: u64,
    allow_joins: bool,
) -> Result<(), String> {
    if world.tick >= u32::MAX - 1 {
        return Err("world tick exhausted; restart required".into());
    }

    let mut timing = Instant::now();
    sessions.receive(network, profiles, now, allow_joins, world, systems)?;
    world.timings.admission.observe(timing.elapsed());
    timing = Instant::now();
    let mut frame = systems.movement.prepare_tick(
        world,
        sessions,
        &mut systems.population,
        &mut systems.police,
        &mut systems.combat,
        &mut systems.missions,
    )?;
    world.timings.planning.observe(timing.elapsed());
    timing = Instant::now();
    systems.movement.update(
        &mut frame,
        world,
        sessions,
        &mut systems.population,
        &mut systems.police,
        &mut systems.combat,
        &mut systems.missions,
    )?;
    world.timings.motors.observe(timing.elapsed());
    timing = Instant::now();
    systems.combat.update(
        world,
        sessions,
        &mut systems.population,
        &mut systems.police,
        &mut systems.missions,
        &mut systems.chat,
    );
    systems.police.finish_tick(
        world,
        sessions,
        &mut systems.population,
        &mut systems.missions,
    )?;
    for peer in frame.failed {
        sessions.disconnect(peer, profiles, world, systems);
        sessions.closing.insert(peer, "unsafe motor input/state");
    }
    systems
        .repairs
        .update(profiles, world, sessions, &mut systems.police)?;
    systems.missions.update(
        profiles,
        &frame.previous_cars,
        world,
        sessions,
        &mut systems.population,
        &mut systems.police,
        &mut systems.combat,
        &mut systems.repairs,
    )?;
    world.timings.gameplay.observe(timing.elapsed());
    timing = Instant::now();
    Replication::publish(world, sessions, systems)?;
    world.timings.replication.observe(timing.elapsed());
    timing = Instant::now();
    Replication::flush(
        network,
        profiles,
        now,
        &frame.previous_cars,
        world,
        sessions,
        systems,
    )?;
    world.timings.flush.observe(timing.elapsed());
    Ok(())
}
/// Resolve interrupted economic operations after disconnecting all players.
pub fn shutdown(
    world: &mut World,
    sessions: &mut Sessions,
    systems: &mut Systems,
    profiles: &mut Profiles,
) -> bool {
    sessions.shutdown(profiles, world, systems)
}
