//! An ambient pedestrian and its current navigation state.
use crate::systems::population::PedestrianPhase;
use protocol::*;
#[derive(bevy_ecs::prelude::Component)]
pub struct Pedestrian {
    pub id: EntityId,
    pub state: CharacterState,
    pub phase: PedestrianPhase,
    pub(crate) from: u32,
    pub(crate) to: u32,
    pub(crate) input: InputFrame,
    pub(crate) wait_until: u32,
    pub(crate) far_since: Option<u32>,
    pub(crate) progress_at: u32,
    pub(crate) progress_position: [f32; 3],
    pub(crate) flee_until: u32,
    pub(crate) threat: [f32; 3],
}

impl Pedestrian {
    pub(crate) fn apply_damage(&mut self, amount: u8) {
        self.state.health.0 = self.state.health.0.saturating_sub(u16::from(amount));
        if self.state.health.0 == 0 {
            self.phase = PedestrianPhase::Incapacitated;
            self.state.mode = CharacterMode::Downed;
            self.state.motor.velocity = Vec3([Scalar(0.0); 3]);
        }
    }

    /// Advance this actor only; navigation decisions are made before movement.
    pub(crate) fn step_motor(
        &mut self,
        tick: u32,
        collision: &sim_core::CollisionWorld,
        config: &sim_core::SimConfig,
    ) -> std::result::Result<(), String> {
        if self.state.health.0 == 0 {
            return Ok(());
        }
        let mut input = self.input.clone();
        input.tick = tick;
        let result = sim_core::step_character(
            &self.state.motor,
            &input,
            &sim_core::StepContext {
                tick,
                collision,
                config,
            },
        )?;
        self.state.motor = result.motor;
        Ok(())
    }
}
