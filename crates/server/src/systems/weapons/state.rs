//! Weapon initialization, validation and inventory/trigger operations.
use crate::components::*;
use protocol::*;

/// Shared eligibility rules for native intent selection and synchronous actions.
pub(crate) fn can_fire(
    weapon: &Weapon,
    aim: &Fire,
    automatic_tick: bool,
    tick: u32,
) -> std::result::Result<WeaponDefinition, ActionCode> {
    let definition = definition(weapon);
    if (definition.automatic && !automatic_tick) || aim.weapon_id != weapon.equipped {
        return Err(ActionCode::Unavailable);
    }
    if weapon.equip_end.is_some() {
        return Err(ActionCode::Cooldown);
    }
    if weapon.shot_sequence == u32::MAX {
        return Err(ActionCode::Unavailable);
    }
    if weapon.reload_end.is_some() || tick < weapon.slots[weapon.equipped.index()].next_fire {
        return Err(ActionCode::Cooldown);
    }
    if weapon.slots[weapon.equipped.index()].magazine == 0 {
        return Err(ActionCode::NoAmmo);
    }
    Ok(definition)
}
pub(crate) fn begin_fire(
    weapon: &mut Weapon,
    aim: &Fire,
    automatic_tick: bool,
    tick: u32,
) -> std::result::Result<WeaponDefinition, ActionCode> {
    let definition = can_fire(weapon, aim, automatic_tick, tick)?;
    weapon.slots[weapon.equipped.index()].magazine -= 1;
    weapon.slots[weapon.equipped.index()].next_fire = tick.saturating_add(definition.fire_ticks);
    Ok(definition)
}

pub(crate) fn trigger_intent(weapon: &mut Weapon, intent: &Trigger, tick: u32) {
    if intent.sequence <= weapon.trigger_sequence {
        return;
    }
    weapon.trigger_sequence = intent.sequence;
    if !intent.held {
        weapon.trigger = None;
        return;
    }
    if intent.weapon_id != weapon.equipped
        || !definition(weapon).automatic
        || weapon.equip_end.is_some()
        || weapon.reload_end.is_some()
    {
        weapon.trigger = None;
        return;
    }
    if !intent.pressed
        && !weapon
            .trigger
            .as_ref()
            .is_some_and(|(_, until)| tick < *until)
    {
        return;
    }
    if let Some((_, until)) = &weapon.trigger {
        if tick + 12 < *until {
            return;
        }
    }
    weapon.trigger = Some((intent.clone(), tick.saturating_add(15)));
}
pub(crate) fn automatic_aim(weapon: &mut Weapon, tick: u32, epoch: u32) -> Option<Fire> {
    let (intent, until) = weapon.trigger.as_ref()?;
    if tick >= *until || intent.control_epoch.0 != epoch {
        weapon.trigger = None;
        return None;
    }
    Some(Fire {
        weapon_id: intent.weapon_id,
        aim_yaw: intent.aim_yaw,
        aim_pitch: intent.aim_pitch,
    })
}
pub(crate) fn definition(weapon: &Weapon) -> WeaponDefinition {
    weapon_catalogue()[weapon.equipped.index()].clone()
}
pub(crate) fn cancel(weapon: &mut Weapon) {
    weapon.aiming = false;
    weapon.trigger = None;
    weapon.reload_end = None;
    weapon.equip_end = None;
}
pub(crate) fn equip(weapon: &mut Weapon, id: WeaponId, tick: u32) -> ActionCode {
    if !weapon.slots[id.index()].owned {
        return ActionCode::Unavailable;
    }
    if id == weapon.equipped {
        return ActionCode::Ok;
    }
    cancel(weapon);
    weapon.equipped = id;
    weapon.equip_end = Some(tick.saturating_add(definition(weapon).equip_ticks));
    ActionCode::Ok
}
pub(crate) fn reload(weapon: &mut Weapon, tick: u32) -> ActionCode {
    if weapon.reload_end.is_some()
        || weapon.equip_end.is_some()
        || weapon.slots[weapon.equipped.index()].magazine == definition(weapon).capacity
    {
        return ActionCode::Cooldown;
    }
    if weapon.slots[weapon.equipped.index()].reserve == 0 {
        return ActionCode::NoAmmo;
    }
    weapon.trigger = None;
    weapon.reload_end = Some(tick.saturating_add(definition(weapon).reload_ticks));
    ActionCode::Ok
}
pub(crate) fn advance(weapon: &mut Weapon, tick: u32) -> bool {
    for (slot, definition) in weapon.slots.iter_mut().zip(weapon_catalogue()) {
        slot.bloom = (slot.bloom - definition.bloom_recovery as f32 / 30.0).max(0.0);
    }
    let mut changed = false;
    if weapon.equip_end.is_some_and(|t| tick >= t) {
        weapon.equip_end = None;
        changed = true;
    }
    if weapon.reload_end.is_some_and(|t| tick >= t) {
        let n =
            u16::from(definition(weapon).capacity - weapon.slots[weapon.equipped.index()].magazine)
                .min(weapon.slots[weapon.equipped.index()].reserve);
        weapon.slots[weapon.equipped.index()].magazine += n as u8;
        weapon.slots[weapon.equipped.index()].reserve -= n;
        weapon.reload_end = None;
        changed = true;
    }
    changed
}
pub(crate) fn loadout() -> Weapon {
    Weapon {
        trigger: None,
        trigger_sequence: 0,
        aiming: false,
        aim_pitch: Pitch(0.0),
        slots: weapon_catalogue().map(|definition| Slot {
            owned: true,
            magazine: definition.capacity,
            reserve: definition.max_reserve,
            ..Slot::default()
        }),
        equipped: WeaponId::Pistol,
        reload_end: None,
        equip_end: None,
        shot_sequence: 0,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_loadout_owns_every_weapon_with_full_ammunition() {
        let weapon = crate::systems::weapons::state::loadout();
        for definition in weapon_catalogue() {
            let slot = &weapon.slots[definition.id.index()];
            assert!(slot.owned);
            assert_eq!(slot.magazine, definition.capacity);
            assert_eq!(slot.reserve, definition.max_reserve);
        }
        assert_eq!(weapon.equipped, WeaponId::Pistol);
    }
    #[test]
    fn partial_reload_and_cancellation_preserve_rounds_for_every_weapon() {
        for d in weapon_catalogue() {
            let mut w = crate::systems::weapons::state::loadout();
            w.slots[d.id.index()] = Slot {
                owned: true,
                magazine: 2,
                reserve: 3,
                next_fire: 1000,
                ..Default::default()
            };
            w.equipped = d.id;
            assert_eq!(
                crate::systems::weapons::state::reload(&mut w, 10),
                ActionCode::Ok
            );
            assert!(!crate::systems::weapons::state::advance(
                &mut w,
                10 + d.reload_ticks - 1
            ));
            crate::systems::weapons::state::cancel(&mut w);
            crate::systems::weapons::state::advance(&mut w, 10 + d.reload_ticks);
            assert_eq!(
                (
                    w.slots[w.equipped.index()].magazine,
                    w.slots[w.equipped.index()].reserve
                ),
                (2, 3)
            );
            assert_eq!(
                crate::systems::weapons::state::reload(&mut w, 100),
                ActionCode::Ok
            );
            crate::systems::weapons::state::advance(&mut w, 100 + d.reload_ticks);
            assert_eq!(
                (
                    w.slots[w.equipped.index()].magazine,
                    w.slots[w.equipped.index()].reserve
                ),
                (5, 0)
            );
            assert_eq!(
                crate::systems::weapons::state::reload(&mut w, 500),
                ActionCode::NoAmmo
            );
            assert_eq!(w.slots[w.equipped.index()].next_fire, 1000);
        }
    }
    #[test]
    fn switching_requires_ownership_and_preserves_each_cooldown() {
        let mut w = crate::systems::weapons::state::loadout();
        w.slots[1].owned = false;
        assert_eq!(
            crate::systems::weapons::state::equip(&mut w, WeaponId::Rifle, 1),
            ActionCode::Unavailable
        );
        w.slots[1].owned = true;
        w.slots[w.equipped.index()].next_fire = 100;
        w.slots[w.equipped.index()].magazine = 3;
        crate::systems::weapons::state::reload(&mut w, 1);
        assert_eq!(
            crate::systems::weapons::state::equip(&mut w, WeaponId::Rifle, 2),
            ActionCode::Ok
        );
        assert!(w.reload_end.is_none());
        assert_eq!(
            crate::systems::weapons::state::reload(&mut w, 3),
            ActionCode::Cooldown
        );
        crate::systems::weapons::state::equip(&mut w, WeaponId::Pistol, 3);
        assert_eq!(w.slots[w.equipped.index()].next_fire, 100);
        assert_eq!(w.slots[w.equipped.index()].magazine, 3);
        assert_eq!(w.slots[w.equipped.index()].reserve, 72);
        let end = w.equip_end;
        crate::systems::weapons::state::equip(&mut w, WeaponId::Pistol, 4);
        assert_eq!(w.equip_end, end);
        crate::systems::weapons::state::cancel(&mut w);
        assert!(w.equip_end.is_none());
    }
}

#[cfg(test)]
mod trigger_tests {
    use super::*;
    fn intent(sequence: u32, held: bool, pressed: bool) -> Trigger {
        Trigger {
            control_epoch: ControlEpoch(1),
            sequence,
            held,
            pressed,
            weapon_id: WeaponId::MachineGun,
            aim_yaw: Yaw(0.0),
            aim_pitch: Pitch(0.0),
        }
    }
    #[test]
    fn leases_expire_and_duplicates_old_starts_and_refreshes_cannot_resurrect_them() {
        let mut w = crate::systems::weapons::state::loadout();
        w.slots[2].owned = true;
        w.equipped = WeaponId::MachineGun;
        crate::systems::weapons::state::trigger_intent(&mut w, &intent(1, true, true), 100);
        assert!(crate::systems::weapons::state::automatic_aim(&mut w, 114, 1).is_some());
        crate::systems::weapons::state::trigger_intent(&mut w, &intent(1, true, true), 114);
        assert!(crate::systems::weapons::state::automatic_aim(&mut w, 115, 1).is_none());
        crate::systems::weapons::state::trigger_intent(&mut w, &intent(2, true, false), 116);
        assert!(w.trigger.is_none());
        crate::systems::weapons::state::trigger_intent(&mut w, &intent(3, true, true), 117);
        crate::systems::weapons::state::trigger_intent(&mut w, &intent(4, false, false), 118);
        crate::systems::weapons::state::trigger_intent(&mut w, &intent(3, true, true), 119);
        assert!(w.trigger.is_none());
        crate::systems::weapons::state::trigger_intent(&mut w, &intent(5, true, true), 120);
        assert!(crate::systems::weapons::state::automatic_aim(&mut w, 121, 2).is_none());
    }
    #[test]
    fn reload_equip_and_epoch_changes_require_fresh_press() {
        let mut w = crate::systems::weapons::state::loadout();
        w.slots[2] = Slot {
            owned: true,
            magazine: 20,
            reserve: 20,
            ..Default::default()
        };
        w.equipped = WeaponId::MachineGun;
        crate::systems::weapons::state::trigger_intent(&mut w, &intent(1, true, true), 10);
        assert!(w.trigger.is_some());
        crate::systems::weapons::state::reload(&mut w, 11);
        assert!(w.trigger.is_none());
        crate::systems::weapons::state::advance(&mut w, 101);
        crate::systems::weapons::state::trigger_intent(&mut w, &intent(2, true, false), 102);
        assert!(w.trigger.is_none());
        crate::systems::weapons::state::trigger_intent(&mut w, &intent(3, true, true), 103);
        assert!(w.trigger.is_some());
        crate::systems::weapons::state::equip(&mut w, WeaponId::Pistol, 104);
        assert!(w.trigger.is_none());
    }
}

#[cfg(test)]
mod firing_tests {
    use super::*;
    #[test]
    fn firing_validation_is_read_only_and_commit_rechecks_before_spending_ammo() {
        let mut weapon = crate::systems::weapons::state::loadout();
        let aim = Fire {
            weapon_id: weapon.equipped,
            aim_yaw: Yaw(0.0),
            aim_pitch: Pitch(0.0),
        };
        let ammo = weapon.slots[weapon.equipped.index()].magazine;
        let definition =
            crate::systems::weapons::state::can_fire(&weapon, &aim, false, 10).unwrap();
        assert_eq!(weapon.slots[weapon.equipped.index()].magazine, ammo);
        assert_eq!(weapon.slots[weapon.equipped.index()].next_fire, 0);
        weapon.reload_end = Some(20);
        assert!(matches!(
            crate::systems::weapons::state::begin_fire(&mut weapon, &aim, false, 10),
            Err(ActionCode::Cooldown)
        ));
        assert_eq!(weapon.slots[weapon.equipped.index()].magazine, ammo);
        weapon.reload_end = None;
        crate::systems::weapons::state::begin_fire(&mut weapon, &aim, false, 10).unwrap();
        assert_eq!(weapon.slots[weapon.equipped.index()].magazine, ammo - 1);
        assert_eq!(
            weapon.slots[weapon.equipped.index()].next_fire,
            10 + definition.fire_ticks
        );
        assert!(matches!(
            crate::systems::weapons::state::begin_fire(&mut weapon, &aim, false, 10),
            Err(ActionCode::Cooldown)
        ));
        assert_eq!(weapon.slots[weapon.equipped.index()].magazine, ammo - 1);
        weapon.slots[weapon.equipped.index()].magazine = 0;
        assert!(matches!(
            crate::systems::weapons::state::begin_fire(
                &mut weapon,
                &aim,
                false,
                10 + definition.fire_ticks
            ),
            Err(ActionCode::NoAmmo)
        ));
    }
}
