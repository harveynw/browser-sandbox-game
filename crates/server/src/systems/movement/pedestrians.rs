//! Pedestrian motor stepping invoked by movement systems and fixtures.
use crate::component::*;

/// Advance this actor only; navigation decisions are made before movement.
pub(crate) fn step_motor(
    movement: &mut CharacterMovement,
    health: &Health,
    input: &protocol::InputFrame,
    tick: u32,
    collision: &sim_core::CollisionWorld,
    config: &sim_core::SimConfig,
) -> std::result::Result<(), String> {
    if health.0 == 0 {
        return Ok(());
    }
    let mut input = input.clone();
    input.tick = tick;
    let result = sim_core::step_character(
        &movement.0,
        &input,
        &sim_core::StepContext {
            tick,
            collision,
            config,
        },
    )?;
    movement.0 = result.motor;
    Ok(())
}
