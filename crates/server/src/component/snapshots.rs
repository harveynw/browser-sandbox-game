//! Protocol state, owned actor observations, and their conversion boundaries.
use super::queries::*;
use super::*;

/// Owns player spawn data or a temporary gameplay observation.
#[derive(Clone)]
#[cfg_attr(test, derive(Component))]
pub struct PlayerSnapshot {
    /// Stable gameplay ID of this player.
    pub id: EntityId,
    /// Protocol-shaped state used by this owned spawn value or observation.
    pub state: CharacterState,
    /// T-shirt colour shown to other players.
    pub tshirt_colour: TshirtColour,
    /// Player name shown in presence and chat messages.
    pub display_name: DisplayName,
    /// Respawn and spawn-protection timers for this player.
    pub lifecycle: Lifecycle,
    /// Weapon inventory and action timers.
    pub weapon: Weapon,
    /// Assigned player spawn slot.
    pub(crate) slot: usize,
}

/// Owns pedestrian spawn data or a temporary gameplay observation.
#[derive(Clone)]
#[cfg_attr(test, derive(Component))]
pub struct PedestrianSnapshot {
    /// Stable gameplay ID of this pedestrian.
    pub id: EntityId,
    /// Protocol-shaped state used by this owned spawn value or observation.
    pub state: CharacterState,
    /// Current stage of this item's lifecycle.
    pub phase: PedestrianPhase,
    /// Navigation node at the start of the current pedestrian edge.
    pub(crate) from: u32,
    /// Navigation node at the end of the current pedestrian edge.
    pub(crate) to: u32,
    /// Movement input selected for the current simulation step.
    pub(crate) input: InputFrame,
    /// Tick when the pedestrian can resume walking.
    pub(crate) wait_until: u32,
    /// First tick when the actor became distant from all observers.
    pub(crate) far_since: Option<u32>,
    /// Tick of the last movement progress check.
    pub(crate) progress_at: u32,
    /// Position recorded at the last movement progress check.
    pub(crate) progress_position: [f32; 3],
    /// Tick when the pedestrian's flight response expires.
    pub(crate) flee_until: u32,
    /// Position from which the pedestrian is fleeing.
    pub(crate) threat: [f32; 3],
}

/// Owns vehicle spawn data or a temporary gameplay observation.
#[derive(Clone)]
#[cfg_attr(test, derive(Component))]
pub struct VehicleSnapshot {
    /// Stable gameplay ID of this vehicle.
    pub id: EntityId,
    /// Immutable authored geometry/mechanics shared with read-only AI observations.
    pub template: std::sync::Arc<content::VehicleTemplate>,
    /// Protocol-shaped state used by this owned spawn value or observation.
    pub state: VehicleState,
    /// Number of consecutive ticks the vehicle has remained idle.
    pub idle_ticks: u32,
}

/// Owns officer spawn data or a temporary roster observation.
#[derive(Clone)]
#[cfg_attr(test, derive(Component))]
pub(crate) struct OfficerSnapshot {
    /// Gameplay ID of the officer's patrol vehicle.
    pub(crate) unit: u32,
    /// Protocol-shaped state used by this owned spawn value or observation.
    pub(crate) state: CharacterState,
    /// Whether the officer remains inside its patrol vehicle.
    pub(crate) mounted: bool,
    /// Weapon inventory and action timers.
    pub(crate) weapon: Weapon,
    /// Gameplay ID of the officer's assigned suspect.
    pub(crate) assigned: Option<u32>,
    /// Tick when the officer began warning the suspect.
    pub(crate) warned_at: Option<u32>,
    /// Tick when the officer began its current aim.
    pub(crate) aiming_since: Option<u32>,
    /// Navigation nodes remaining on the actor's route.
    pub(crate) route: VecDeque<u32>,
    /// Destination node used to compute the current route.
    pub(crate) route_goal: Option<u32>,
    /// Earliest tick when the actor can request another route.
    pub(crate) next_plan: u32,
}

impl CharacterQueryItem<'_, '_> {
    /// Assembles protocol state from the current authoritative components.
    pub fn to_protocol(&self) -> CharacterState {
        CharacterState {
            motor: self.motor.0.clone(),
            health: CharacterHealth(self.health.0),
            mode: self.mode,
            vehicle_id: self.vehicle_id,
            equipment: self.equipment.clone(),
        }
    }
}

impl CharacterQueryReadOnlyItem<'_, '_> {
    /// Assembles protocol state from the current authoritative components.
    pub fn to_protocol(&self) -> CharacterState {
        CharacterState {
            motor: self.motor.0.clone(),
            health: CharacterHealth(self.health.0),
            mode: self.mode,
            vehicle_id: self.vehicle_id,
            equipment: self.equipment.clone(),
        }
    }
}

impl VehicleStateQueryReadOnlyItem<'_, '_> {
    /// Assembles protocol state from the current authoritative components.
    pub fn to_protocol(&self) -> VehicleState {
        VehicleState {
            motor: self.motor.0.clone(),
            health: VehicleHealth(self.health.0),
            driver_id: self.driver_id,
            disabled: self.disabled,
        }
    }
}

impl PlayerQueryItem<'_, '_> {
    /// Builds an owned observation from the borrowed actor components.
    pub fn to_snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            state: self.state.to_protocol(),
            id: self.data.id.clone(),
            tshirt_colour: self.data.tshirt_colour.clone(),
            display_name: self.data.display_name.clone(),
            lifecycle: self.data.lifecycle.clone(),
            weapon: self.data.weapon.clone(),
            slot: self.data.slot.clone(),
        }
    }
}

impl PlayerQueryReadOnlyItem<'_, '_> {
    /// Builds an owned observation from the borrowed actor components.
    pub fn to_snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            state: self.state.to_protocol(),
            id: self.data.id.clone(),
            tshirt_colour: self.data.tshirt_colour.clone(),
            display_name: self.data.display_name.clone(),
            lifecycle: self.data.lifecycle.clone(),
            weapon: self.data.weapon.clone(),
            slot: self.data.slot.clone(),
        }
    }
}

/// Supplies an owned player observation for projection and shot geometry.
pub trait ToPlayerSnapshot {
    /// Builds an owned observation from the borrowed actor components.
    fn to_snapshot(&self) -> PlayerSnapshot;
}

impl ToPlayerSnapshot for PlayerSnapshot {
    /// Builds an owned observation from the borrowed actor components.
    fn to_snapshot(&self) -> PlayerSnapshot {
        self.clone()
    }
}

impl ToPlayerSnapshot for PlayerQueryItem<'_, '_> {
    /// Builds an owned observation from the borrowed actor components.
    fn to_snapshot(&self) -> PlayerSnapshot {
        self.to_snapshot()
    }
}

impl ToPlayerSnapshot for PlayerQueryReadOnlyItem<'_, '_> {
    /// Builds an owned observation from the borrowed actor components.
    fn to_snapshot(&self) -> PlayerSnapshot {
        self.to_snapshot()
    }
}

impl VehicleQueryReadOnlyItem<'_, '_> {
    /// Builds an owned observation from the borrowed actor components.
    pub fn to_snapshot(&self) -> VehicleSnapshot {
        VehicleSnapshot {
            state: self.state.to_protocol(),
            id: self.data.id.clone(),
            template: self.data.template.clone(),
            idle_ticks: self.data.idle_ticks.clone(),
        }
    }
}

impl OfficerQueryItem<'_, '_> {
    /// Builds an owned observation from the borrowed actor components.
    pub fn to_snapshot(&self) -> OfficerSnapshot {
        OfficerSnapshot {
            state: self.state.to_protocol(),
            unit: self.data.unit.clone(),
            mounted: self.data.mounted.clone(),
            weapon: self.data.weapon.clone(),
            assigned: self.data.assigned.clone(),
            warned_at: self.data.warned_at.clone(),
            aiming_since: self.data.aiming_since.clone(),
            route: self.data.route.clone(),
            route_goal: self.data.route_goal.clone(),
            next_plan: self.data.next_plan.clone(),
        }
    }
}

impl<T: ToPlayerSnapshot + ?Sized> ToPlayerSnapshot for &T {
    /// Builds an owned observation from the borrowed actor components.
    fn to_snapshot(&self) -> PlayerSnapshot {
        (*self).to_snapshot()
    }
}

impl OfficerQueryItem<'_, '_> {
    /// Commits an officer roster decision to its authoritative components.
    pub fn apply(&mut self, value: OfficerSnapshot) {
        *self.data = OfficerData {
            unit: value.unit,
            mounted: value.mounted,
            weapon: value.weapon,
            assigned: value.assigned,
            warned_at: value.warned_at,
            aiming_since: value.aiming_since,
            route: value.route,
            route_goal: value.route_goal,
            next_plan: value.next_plan,
        };
        self.state.motor.0 = value.state.motor;
        self.state.health.0 = value.state.health.0;
        *self.state.body = CharacterBody {
            mode: value.state.mode,
            vehicle_id: value.state.vehicle_id,
            equipment: value.state.equipment,
        };
    }
}

/// Builds public character state with the current weapon presentation.
pub(crate) fn public_state(
    player: &impl crate::component::snapshots::ToPlayerSnapshot,
) -> CharacterState {
    let player = player.to_snapshot();
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

/// Builds private ammunition and ownership views for each weapon slot.
pub(crate) fn views(weapon: &Weapon) -> [WeaponSlotView; 3] {
    std::array::from_fn(|i| WeaponSlotView {
        weapon_id: weapon_catalogue()[i].id,
        owned: weapon.slots[i].owned,
        magazine: Magazine(weapon.slots[i].magazine),
        reserve: weapon.slots[i].reserve,
    })
}
