//! Vehicle entities and local geometry helpers.
//! Vehicle state and safe possession geometry. Transactions are owned by World.
use protocol::{EntityId, VehicleState};
use sim_core::{CollisionQueries, CollisionWorld, QueryFilter};
#[derive(bevy_ecs::prelude::Component, Clone)]
pub struct Vehicle {
    pub id: EntityId,
    /// Immutable authored geometry/mechanics shared with read-only AI observations.
    pub template: std::sync::Arc<content::VehicleTemplate>,
    pub state: VehicleState,
    pub idle_ticks: u32,
}
impl Vehicle {
    pub(crate) fn apply_damage(
        &mut self,
        amount: u16,
        attacker: Option<protocol::EntityId>,
    ) -> bool {
        if self.state.disabled
            || self.state.health.0 == 0
            || attacker.is_some_and(|a| self.state.driver_id.is_some_and(|driver| driver != a))
        {
            return false;
        }
        self.state.health.0 = self.state.health.0.saturating_sub(amount);
        self.idle_ticks = 0;
        if self.state.health.0 == 0 {
            self.state.disabled = self.state.driver_id.is_none();
            self.stop();
        }
        true
    }

    pub fn stop(&mut self) {
        self.state.motor.speed = protocol::Scalar(0.0);
        self.state.motor.velocity = protocol::Vec3([protocol::Scalar(0.0); 3]);
        self.state.motor.steering = protocol::Steering(0.0);
    }
    pub fn seat(&self) -> [f32; 3] {
        sim_core::anchor(&self.state.motor, self.template.seat_offset)
    }
    pub fn entry(&self) -> [f32; 3] {
        sim_core::anchor(&self.state.motor, self.template.exit_offsets[0])
    }
    pub fn proxy(&self) -> sim_core::collision::VehicleProxy {
        sim_core::collision::VehicleProxy {
            entity_id: self.id.0,
            position: self.state.motor.position.0.map(|v| v.0),
            yaw: self.state.motor.yaw.0,
            dimensions: self.template.dimensions.map(|v| v as f32),
        }
    }
}
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
