//! Read-only public character and inventory projections.
use crate::components::*;
use protocol::*;
pub(crate) fn public_state(player: &Player) -> CharacterState {
    let mut state = player.state.clone();
    state.equipment = Some(EquipmentView {
        weapon_id: player.weapon.equipped,
        aiming: player.weapon.aiming
            && player.state.mode == CharacterMode::OnFoot
            && player.state.health.0 > 0,
        aim_pitch: player.weapon.aim_pitch,
        reload_end_tick: player.weapon.reload_end,
        equip_end_tick: player.weapon.equip_end,
    });
    state
}

pub(crate) fn views(weapon: &Weapon) -> [WeaponSlotView; 3] {
    std::array::from_fn(|i| WeaponSlotView {
        weapon_id: weapon_catalogue()[i].id,
        owned: weapon.slots[i].owned,
        magazine: Magazine(weapon.slots[i].magazine),
        reserve: weapon.slots[i].reserve,
    })
}
