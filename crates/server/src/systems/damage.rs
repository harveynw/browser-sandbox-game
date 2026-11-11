//! Shared damage rules invoked by ordered weapon and impact systems.
use crate::components::*;
use crate::systems::population::PedestrianPhase;
use protocol::*;
pub(crate) fn pedestrian_damage(ped: &mut Pedestrian, amount: u8) {
    ped.state.health.0 = ped.state.health.0.saturating_sub(u16::from(amount));
    if ped.state.health.0 == 0 {
        ped.phase = PedestrianPhase::Incapacitated;
        ped.state.mode = CharacterMode::Downed;
        ped.state.motor.velocity = Vec3([Scalar(0.0); 3]);
    }
}
pub(crate) fn vehicle_damage(
    car: &mut Vehicle,
    amount: u16,
    attacker: Option<protocol::EntityId>,
) -> bool {
    if car.state.disabled
        || car.state.health.0 == 0
        || attacker.is_some_and(|a| car.state.driver_id.is_some_and(|driver| driver != a))
    {
        return false;
    }
    car.state.health.0 = car.state.health.0.saturating_sub(amount);
    car.idle_ticks = 0;
    if car.state.health.0 == 0 {
        car.state.disabled = car.state.driver_id.is_none();
        crate::systems::vehicle::stop(&mut *car);
    }
    true
}
