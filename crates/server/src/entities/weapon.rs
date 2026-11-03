//! Authoritative fixed-size inventory; lifecycle timers live on Player.
use protocol::*;
#[derive(Clone, Default)]
pub struct Slot {
    pub owned: bool,
    pub magazine: u8,
    pub reserve: u16,
    pub bloom: f32,
    pub next_fire: u32,
}
#[derive(Clone)]
pub struct Weapon {
    pub trigger: Option<(Trigger, u32)>,
    pub trigger_sequence: u32,
    pub aiming: bool,
    pub aim_pitch: Pitch,
    pub slots: [Slot; 3],
    pub equipped: WeaponId,
    pub reload_end: Option<u32>,
    pub equip_end: Option<u32>,
    pub shot_sequence: u32,
}
#[derive(Default)]
pub struct Lifecycle {
    pub respawn: Option<u32>,
    pub protected_until: Option<u32>,
}
impl Default for Weapon {
    fn default() -> Self {
        Self {
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
}
impl std::ops::Deref for Weapon {
    type Target = Slot;
    fn deref(&self) -> &Slot {
        &self.slots[self.equipped.index()]
    }
}
impl std::ops::DerefMut for Weapon {
    fn deref_mut(&mut self) -> &mut Slot {
        &mut self.slots[self.equipped.index()]
    }
}
impl Weapon {
    pub fn trigger_intent(&mut self, intent: &Trigger, tick: u32) {
        if intent.sequence <= self.trigger_sequence {
            return;
        }
        self.trigger_sequence = intent.sequence;
        if !intent.held {
            self.trigger = None;
            return;
        }
        if intent.weapon_id != self.equipped
            || !self.definition().automatic
            || self.equip_end.is_some()
            || self.reload_end.is_some()
        {
            self.trigger = None;
            return;
        }
        if !intent.pressed
            && !self
                .trigger
                .as_ref()
                .is_some_and(|(_, until)| tick < *until)
        {
            return;
        }
        if let Some((_, until)) = &self.trigger {
            if tick + 12 < *until {
                return;
            }
        }
        self.trigger = Some((intent.clone(), tick.saturating_add(15)));
    }
    pub fn automatic_aim(&mut self, tick: u32, epoch: u32) -> Option<Fire> {
        let (intent, until) = self.trigger.as_ref()?;
        if tick >= *until || intent.control_epoch.0 != epoch {
            self.trigger = None;
            return None;
        }
        Some(Fire {
            weapon_id: intent.weapon_id,
            aim_yaw: intent.aim_yaw,
            aim_pitch: intent.aim_pitch,
        })
    }
    pub fn definition(&self) -> WeaponDefinition {
        weapon_catalogue()[self.equipped.index()].clone()
    }
    pub fn cancel(&mut self) {
        self.aiming = false;
        self.trigger = None;
        self.reload_end = None;
        self.equip_end = None;
    }
    pub fn equip(&mut self, id: WeaponId, tick: u32) -> ActionCode {
        if !self.slots[id.index()].owned {
            return ActionCode::Unavailable;
        }
        if id == self.equipped {
            return ActionCode::Ok;
        }
        self.cancel();
        self.equipped = id;
        self.equip_end = Some(tick.saturating_add(self.definition().equip_ticks));
        ActionCode::Ok
    }
    pub fn reload(&mut self, tick: u32) -> ActionCode {
        if self.reload_end.is_some()
            || self.equip_end.is_some()
            || self.magazine == self.definition().capacity
        {
            return ActionCode::Cooldown;
        }
        if self.reserve == 0 {
            return ActionCode::NoAmmo;
        }
        self.trigger = None;
        self.reload_end = Some(tick.saturating_add(self.definition().reload_ticks));
        ActionCode::Ok
    }
    pub fn advance(&mut self, tick: u32) -> bool {
        for (slot, definition) in self.slots.iter_mut().zip(weapon_catalogue()) {
            slot.bloom = (slot.bloom - definition.bloom_recovery as f32 / 30.0).max(0.0);
        }
        let mut changed = false;
        if self.equip_end.is_some_and(|t| tick >= t) {
            self.equip_end = None;
            changed = true;
        }
        if self.reload_end.is_some_and(|t| tick >= t) {
            let n = u16::from(self.definition().capacity - self.magazine).min(self.reserve);
            self.magazine += n as u8;
            self.reserve -= n;
            self.reload_end = None;
            changed = true;
        }
        changed
    }
    pub fn views(&self) -> [WeaponSlotView; 3] {
        std::array::from_fn(|i| WeaponSlotView {
            weapon_id: weapon_catalogue()[i].id,
            owned: self.slots[i].owned,
            magazine: Magazine(self.slots[i].magazine),
            reserve: self.slots[i].reserve,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_loadout_owns_every_weapon_with_full_ammunition() {
        let weapon = Weapon::default();
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
            let mut w = Weapon::default();
            w.slots[d.id.index()] = Slot {
                owned: true,
                magazine: 2,
                reserve: 3,
                next_fire: 1000,
                ..Default::default()
            };
            w.equipped = d.id;
            assert_eq!(w.reload(10), ActionCode::Ok);
            assert!(!w.advance(10 + d.reload_ticks - 1));
            w.cancel();
            w.advance(10 + d.reload_ticks);
            assert_eq!((w.magazine, w.reserve), (2, 3));
            assert_eq!(w.reload(100), ActionCode::Ok);
            w.advance(100 + d.reload_ticks);
            assert_eq!((w.magazine, w.reserve), (5, 0));
            assert_eq!(w.reload(500), ActionCode::NoAmmo);
            assert_eq!(w.next_fire, 1000);
        }
    }
    #[test]
    fn switching_requires_ownership_and_preserves_each_cooldown() {
        let mut w = Weapon::default();
        w.slots[1].owned = false;
        assert_eq!(w.equip(WeaponId::Rifle, 1), ActionCode::Unavailable);
        w.slots[1].owned = true;
        w.next_fire = 100;
        w.magazine = 3;
        w.reload(1);
        assert_eq!(w.equip(WeaponId::Rifle, 2), ActionCode::Ok);
        assert!(w.reload_end.is_none());
        assert_eq!(w.reload(3), ActionCode::Cooldown);
        w.equip(WeaponId::Pistol, 3);
        assert_eq!(w.next_fire, 100);
        assert_eq!(w.magazine, 3);
        assert_eq!(w.reserve, 72);
        let end = w.equip_end;
        w.equip(WeaponId::Pistol, 4);
        assert_eq!(w.equip_end, end);
        w.cancel();
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
        let mut w = Weapon::default();
        w.slots[2].owned = true;
        w.equipped = WeaponId::MachineGun;
        w.trigger_intent(&intent(1, true, true), 100);
        assert!(w.automatic_aim(114, 1).is_some());
        w.trigger_intent(&intent(1, true, true), 114);
        assert!(w.automatic_aim(115, 1).is_none());
        w.trigger_intent(&intent(2, true, false), 116);
        assert!(w.trigger.is_none());
        w.trigger_intent(&intent(3, true, true), 117);
        w.trigger_intent(&intent(4, false, false), 118);
        w.trigger_intent(&intent(3, true, true), 119);
        assert!(w.trigger.is_none());
        w.trigger_intent(&intent(5, true, true), 120);
        assert!(w.automatic_aim(121, 2).is_none());
    }
    #[test]
    fn reload_equip_and_epoch_changes_require_fresh_press() {
        let mut w = Weapon::default();
        w.slots[2] = Slot {
            owned: true,
            magazine: 20,
            reserve: 20,
            ..Default::default()
        };
        w.equipped = WeaponId::MachineGun;
        w.trigger_intent(&intent(1, true, true), 10);
        assert!(w.trigger.is_some());
        w.reload(11);
        assert!(w.trigger.is_none());
        w.advance(101);
        w.trigger_intent(&intent(2, true, false), 102);
        assert!(w.trigger.is_none());
        w.trigger_intent(&intent(3, true, true), 103);
        assert!(w.trigger.is_some());
        w.equip(WeaponId::Pistol, 104);
        assert!(w.trigger.is_none());
    }
}
