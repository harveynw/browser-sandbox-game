//! A connected player character and its equipment.
use protocol::*;
pub struct Player {
    pub id: EntityId,
    pub state: CharacterState,
    pub tshirt_colour: TshirtColour,
    pub display_name: DisplayName,
    pub lifecycle: crate::entities::weapon::Lifecycle,
    pub weapon: crate::entities::weapon::Weapon,
    pub(crate) slot: usize,
}

impl Player {
    pub fn public_state(&self) -> CharacterState {
        let mut state = self.state.clone();
        state.equipment = Some(EquipmentView {
            weapon_id: self.weapon.equipped,
            aiming: self.weapon.aiming
                && self.state.mode == CharacterMode::OnFoot
                && self.state.health.0 > 0,
            aim_pitch: self.weapon.aim_pitch,
            reload_end_tick: self.weapon.reload_end,
            equip_end_tick: self.weapon.equip_end,
        });
        state
    }
}
