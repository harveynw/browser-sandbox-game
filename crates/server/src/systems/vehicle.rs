//! Shared vehicle motor and possession/collision geometry helpers.
use crate::component::queries::{VehicleQueryItem, VehicleQueryReadOnlyItem};
use crate::component::*;

/// Stops a vehicle and clears its active driving motion.
pub(crate) fn stop(car: &mut impl VehicleMotion) {
    let motor = car.motor_mut();
    motor.speed = protocol::Scalar(0.0);
    motor.velocity = protocol::Vec3([protocol::Scalar(0.0); 3]);
    motor.steering = protocol::Steering(0.0);
}
/// Gives movement helpers access to an owned observation or a live vehicle motor.
pub(crate) trait VehicleMotion {
    fn motor_mut(&mut self) -> &mut protocol::VehicleMotor;
}
impl VehicleMotion for VehicleSnapshot {
    fn motor_mut(&mut self) -> &mut protocol::VehicleMotor {
        &mut self.state.motor
    }
}
impl VehicleMotion for VehicleQueryItem<'_, '_> {
    fn motor_mut(&mut self) -> &mut protocol::VehicleMotor {
        &mut self.state.motor.0
    }
}
impl<T: VehicleMotion> VehicleMotion for &mut T {
    fn motor_mut(&mut self) -> &mut protocol::VehicleMotor {
        (**self).motor_mut()
    }
}

/// Borrows the geometry needed by vehicle placement and collision queries.
pub struct VehicleGeometry<'a> {
    /// Stable gameplay ID used by collision proxies.
    pub id: protocol::EntityId,
    /// Current authoritative vehicle motor.
    pub motor: &'a protocol::VehicleMotor,
    /// Shared authored geometry and mechanics.
    pub template: &'a content::VehicleTemplate,
}
/// Supplies vehicle geometry without cloning movement or gameplay state.
pub trait VehicleGeometryAccess {
    fn geometry(&self) -> VehicleGeometry<'_>;
}
impl VehicleGeometryAccess for VehicleSnapshot {
    fn geometry(&self) -> VehicleGeometry<'_> {
        VehicleGeometry {
            id: self.id,
            motor: &self.state.motor,
            template: &self.template,
        }
    }
}
impl VehicleGeometryAccess for VehicleQueryReadOnlyItem<'_, '_> {
    fn geometry(&self) -> VehicleGeometry<'_> {
        VehicleGeometry {
            id: self.id,
            motor: &self.state.motor.0,
            template: &self.template,
        }
    }
}
impl VehicleGeometryAccess for VehicleQueryItem<'_, '_> {
    fn geometry(&self) -> VehicleGeometry<'_> {
        VehicleGeometry {
            id: self.id,
            motor: &self.state.motor.0,
            template: &self.template,
        }
    }
}
impl<T: VehicleGeometryAccess> VehicleGeometryAccess for &T {
    fn geometry(&self) -> VehicleGeometry<'_> {
        (**self).geometry()
    }
}

/// Returns the character position used for an occupied vehicle seat.
pub(crate) fn seat(car: &impl VehicleGeometryAccess) -> [f32; 3] {
    let car = car.geometry();
    sim_core::anchor(car.motor, car.template.seat_offset)
}
/// Finds a valid nearby vehicle entry position for a player.
pub(crate) fn entry(car: &impl VehicleGeometryAccess) -> [f32; 3] {
    let car = car.geometry();
    sim_core::anchor(car.motor, car.template.exit_offsets[0])
}
/// Builds a collision proxy from a vehicle's current motor and dimensions.
pub(crate) fn proxy(car: &impl VehicleGeometryAccess) -> sim_core::collision::VehicleProxy {
    let car = car.geometry();
    sim_core::collision::VehicleProxy {
        entity_id: car.id.0,
        position: car.motor.position.0.map(|v| v.0),
        yaw: car.motor.yaw.0,
        dimensions: car.template.dimensions.map(|v| v as f32),
    }
}

use sim_core::{CollisionQueries, CollisionWorld, QueryFilter};
/// Checks whether static geometry blocks a movement segment.
pub fn clear_segment(collision: &CollisionWorld, a: [f32; 3], b: [f32; 3]) -> bool {
    let delta: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
    distance < 0.001
        || collision
            .raycast(a, delta, distance, QueryFilter::MOVEMENT)
            .is_ok_and(|hit| hit.is_none_or(|h| h.distance >= distance - 0.01))
}
/// Finds a vehicle exit position that clears collision geometry.
pub fn safe_exit(car: &impl VehicleGeometryAccess, collision: &CollisionWorld) -> Option<[f32; 3]> {
    safe_exit_with(car, collision, |_| true)
}
/// Finds a vehicle exit that clears geometry and the caller's placement checks.
pub fn safe_exit_with(
    car: &impl VehicleGeometryAccess,
    collision: &CollisionWorld,
    eligible: impl Fn([f32; 3]) -> bool,
) -> Option<[f32; 3]> {
    let car = car.geometry();
    for offset in &car.template.exit_offsets {
        let mut p = sim_core::anchor(car.motor, *offset);
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
            car.motor,
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
