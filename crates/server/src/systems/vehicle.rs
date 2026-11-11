//! Shared vehicle motor and possession/collision geometry helpers.
use crate::components::*;

pub(crate) fn stop(car: &mut Vehicle) {
    car.state.motor.speed = protocol::Scalar(0.0);
    car.state.motor.velocity = protocol::Vec3([protocol::Scalar(0.0); 3]);
    car.state.motor.steering = protocol::Steering(0.0);
}
pub(crate) fn seat(car: &Vehicle) -> [f32; 3] {
    sim_core::anchor(&car.state.motor, car.template.seat_offset)
}
pub(crate) fn entry(car: &Vehicle) -> [f32; 3] {
    sim_core::anchor(&car.state.motor, car.template.exit_offsets[0])
}
pub(crate) fn proxy(car: &Vehicle) -> sim_core::collision::VehicleProxy {
    sim_core::collision::VehicleProxy {
        entity_id: car.id.0,
        position: car.state.motor.position.0.map(|v| v.0),
        yaw: car.state.motor.yaw.0,
        dimensions: car.template.dimensions.map(|v| v as f32),
    }
}

use sim_core::{CollisionQueries, CollisionWorld, QueryFilter};
pub fn clear_segment(collision: &CollisionWorld, a: [f32; 3], b: [f32; 3]) -> bool {
    let delta: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
    distance < 0.001
        || collision
            .raycast(a, delta, distance, QueryFilter::MOVEMENT)
            .is_ok_and(|hit| hit.is_none_or(|h| h.distance >= distance - 0.01))
}
pub fn safe_exit(car: &Vehicle, collision: &CollisionWorld) -> Option<[f32; 3]> {
    safe_exit_with(car, collision, |_| true)
}
pub fn safe_exit_with(
    car: &Vehicle,
    collision: &CollisionWorld,
    eligible: impl Fn([f32; 3]) -> bool,
) -> Option<[f32; 3]> {
    for offset in &car.template.exit_offsets {
        let mut p = sim_core::anchor(&car.state.motor, *offset);
        let floor = collision
            .raycast(
                [p[0], p[1] + 0.5, p[2]],
                [0.0, -1.0, 0.0],
                1.0,
                QueryFilter::MOVEMENT,
            )
            .ok()
            .flatten();
        let Some(floor) = floor else {
            continue;
        };
        if floor.normal[1] < 0.707 {
            continue;
        }
        p[1] = floor.point[1] + 0.02;
        if collision
            .overlaps_character(p, QueryFilter::MOVEMENT)
            .ok()?
        {
            continue;
        }
        let door = sim_core::anchor(
            &car.state.motor,
            [
                offset[0].clamp(
                    -car.template.dimensions[0] / 2.0,
                    car.template.dimensions[0] / 2.0,
                ),
                0.9,
                offset[2].clamp(
                    -car.template.dimensions[2] / 2.0,
                    car.template.dimensions[2] / 2.0,
                ),
            ],
        );
        if eligible(p) && clear_segment(collision, door, [p[0], p[1] + 0.9, p[2]]) {
            return Some(p);
        }
    }
    None
}
