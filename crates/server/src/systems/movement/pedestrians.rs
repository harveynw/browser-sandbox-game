//! Pedestrian motor stepping invoked by movement systems and fixtures.
use crate::components::*;

/// Advance this actor only; navigation decisions are made before movement.
pub(crate) fn step_motor(
    ped: &mut Pedestrian,
    tick: u32,
    collision: &sim_core::CollisionWorld,
    config: &sim_core::SimConfig,
) -> std::result::Result<(), String> {
    if ped.state.health.0 == 0 {
        return Ok(());
    }
    let mut input = ped.input.clone();
    input.tick = tick;
    let result = sim_core::step_character(
        &ped.state.motor,
        &input,
        &sim_core::StepContext {
            tick,
            collision,
            config,
        },
    )?;
    ped.state.motor = result.motor;
    Ok(())
}
