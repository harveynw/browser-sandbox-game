//! An ambient pedestrian and its current navigation state.
use crate::systems::population::PedestrianPhase;
use protocol::*;
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
