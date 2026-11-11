//! Traffic input generation from component data.
use super::*;

pub(crate) fn motor_input(traffic: &Traffic, tick: u32, speed: f32) -> Option<InputFrame> {
    if traffic.phase == TrafficPhase::Available {
        return None;
    }
    let mut input = traffic.input.clone();
    input.tick = tick;
    input.move_z = InputAxis(
        ((traffic.target_speed - speed) * 2.0)
            .clamp(-1.0, 1.0)
            .mul_add(32767.0, 0.0) as i16,
    );
    Some(input)
}
