//! A connected player character and its equipment.
use protocol::*;
pub struct Player {
    pub id: EntityId,
    pub state: CharacterState,
    pub tshirt_colour: TshirtColour,
    pub display_name: DisplayName,
    pub weapon: crate::entities::weapon::Weapon,
    pub(crate) slot: usize,
}
