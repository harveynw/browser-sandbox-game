//! Test adapters between owned scenario values and live actor components.
use super::*;
use super::{queries::*, spawn::*};
use bevy_ecs::prelude::*;

impl PlayerBundle {
    /// Extracts an owned actor value for scenario editing between ticks.
    fn into_snapshot(self) -> PlayerSnapshot {
        PlayerSnapshot {
            id: self.data.id,
            tshirt_colour: self.data.tshirt_colour,
            display_name: self.data.display_name,
            lifecycle: self.data.lifecycle,
            weapon: self.data.weapon,
            slot: self.data.slot,
            state: CharacterState {
                motor: self.movement.0,
                health: CharacterHealth(self.health.0),
                mode: self.body.mode,
                vehicle_id: self.body.vehicle_id,
                equipment: self.body.equipment,
            },
        }
    }
}

impl VehicleBundle {
    /// Extracts an owned actor value for scenario editing between ticks.
    fn into_snapshot(self) -> VehicleSnapshot {
        VehicleSnapshot {
            id: self.data.id,
            template: self.data.template,
            idle_ticks: self.data.idle_ticks,
            state: VehicleState {
                motor: self.movement.0,
                health: VehicleHealth(self.health.0),
                driver_id: self.body.driver_id,
                disabled: self.body.disabled,
            },
        }
    }
}

impl PedestrianBundle {
    /// Extracts an owned actor value for scenario editing between ticks.
    fn into_snapshot(self) -> PedestrianSnapshot {
        PedestrianSnapshot {
            id: self.data.id,
            phase: self.data.phase,
            from: self.data.from,
            to: self.data.to,
            input: self.data.input,
            wait_until: self.data.wait_until,
            far_since: self.data.far_since,
            progress_at: self.data.progress_at,
            progress_position: self.data.progress_position,
            flee_until: self.data.flee_until,
            threat: self.data.threat,
            state: CharacterState {
                motor: self.movement.0,
                health: CharacterHealth(self.health.0),
                mode: self.body.mode,
                vehicle_id: self.body.vehicle_id,
                equipment: self.body.equipment,
            },
        }
    }
}

impl OfficerBundle {
    /// Extracts an owned actor value for scenario editing between ticks.
    fn into_snapshot(self) -> OfficerSnapshot {
        OfficerSnapshot {
            unit: self.data.unit,
            mounted: self.data.mounted,
            weapon: self.data.weapon,
            assigned: self.data.assigned,
            warned_at: self.data.warned_at,
            aiming_since: self.data.aiming_since,
            route: self.data.route,
            route_goal: self.data.route_goal,
            next_plan: self.data.next_plan,
            state: CharacterState {
                motor: self.movement.0,
                health: CharacterHealth(self.health.0),
                mode: self.body.mode,
                vehicle_id: self.body.vehicle_id,
                equipment: self.body.equipment,
            },
        }
    }
}

/// Converts owned scenario actors into the same components used by the live server.
pub(crate) fn install_fixture_actors(world: &mut World) {
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<PlayerSnapshot>>()
        .iter(world)
        .collect();
    for entity in entities {
        let actor = world.entity_mut(entity).take::<PlayerSnapshot>().unwrap();
        world.entity_mut(entity).insert(actor.into_bundle());
    }
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<VehicleSnapshot>>()
        .iter(world)
        .collect();
    for entity in entities {
        let actor = world.entity_mut(entity).take::<VehicleSnapshot>().unwrap();
        world.entity_mut(entity).insert(actor.into_bundle());
    }
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<PedestrianSnapshot>>()
        .iter(world)
        .collect();
    for entity in entities {
        let actor = world
            .entity_mut(entity)
            .take::<PedestrianSnapshot>()
            .unwrap();
        world.entity_mut(entity).insert(actor.into_bundle());
    }
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<OfficerSnapshot>>()
        .iter(world)
        .collect();
    for entity in entities {
        let actor = world.entity_mut(entity).take::<OfficerSnapshot>().unwrap();
        world.entity_mut(entity).insert(actor.into_bundle());
    }
}

/// Extracts owned scenario actors without retaining a second copy in the live world.
pub(crate) fn unpack_fixture_actors(world: &mut World) {
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<PlayerData>>()
        .iter(world)
        .collect();
    for entity in entities {
        let actor = world
            .entity_mut(entity)
            .take::<PlayerBundle>()
            .expect("complete actor components");
        world.entity_mut(entity).insert(actor.into_snapshot());
    }
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<VehicleData>>()
        .iter(world)
        .collect();
    for entity in entities {
        let actor = world
            .entity_mut(entity)
            .take::<VehicleBundle>()
            .expect("complete actor components");
        world.entity_mut(entity).insert(actor.into_snapshot());
    }
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<PedestrianData>>()
        .iter(world)
        .collect();
    for entity in entities {
        let actor = world
            .entity_mut(entity)
            .take::<PedestrianBundle>()
            .expect("complete actor components");
        world.entity_mut(entity).insert(actor.into_snapshot());
    }
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<OfficerData>>()
        .iter(world)
        .collect();
    for entity in entities {
        let actor = world
            .entity_mut(entity)
            .take::<OfficerBundle>()
            .expect("complete actor components");
        world.entity_mut(entity).insert(actor.into_snapshot());
    }
}

pub(crate) trait ActorWorldExt {
    fn actor<
        Q: bevy_ecs::query::ReadOnlyQueryData
            + bevy_ecs::query::ReleaseStateQueryData
            + bevy_ecs::query::SingleEntityQueryData,
    >(
        &self,
        entity: Entity,
    ) -> Option<Q::Item<'_, 'static>>;
    fn actor_mut<
        Q: bevy_ecs::query::ReleaseStateQueryData + bevy_ecs::query::SingleEntityQueryData,
    >(
        &mut self,
        entity: Entity,
    ) -> Option<Q::Item<'_, 'static>>;
}

impl ActorWorldExt for World {
    fn actor<
        Q: bevy_ecs::query::ReadOnlyQueryData
            + bevy_ecs::query::ReleaseStateQueryData
            + bevy_ecs::query::SingleEntityQueryData,
    >(
        &self,
        entity: Entity,
    ) -> Option<Q::Item<'_, 'static>> {
        self.get_entity(entity).ok()?.get_components::<Q>().ok()
    }
    fn actor_mut<
        Q: bevy_ecs::query::ReleaseStateQueryData + bevy_ecs::query::SingleEntityQueryData,
    >(
        &mut self,
        entity: Entity,
    ) -> Option<Q::Item<'_, 'static>> {
        self.get_entity_mut(entity)
            .ok()?
            .into_components_mut::<Q>()
            .ok()
    }
}

mod tests {
    use super::*;

    /// Changes health without borrowing movement and projects the same authoritative values to the wire.
    #[test]
    fn independent_health_and_movement_are_the_only_live_state() {
        let mut world = World::new();
        let original = sim_core::spawn_character([1.0, 0.02, 3.0], 0.0).unwrap();
        let entity = world
            .spawn(
                PlayerSnapshot {
                    id: EntityId(1),
                    state: CharacterState {
                        motor: original.clone(),
                        health: CharacterHealth(100),
                        mode: CharacterMode::OnFoot,
                        vehicle_id: None,
                        equipment: None,
                    },
                    tshirt_colour: TshirtColour::default(),
                    display_name: DisplayName("Guest".into()),
                    lifecycle: Lifecycle::default(),
                    weapon: crate::systems::weapons::state::loadout(),
                    slot: 0,
                }
                .into_bundle(),
            )
            .id();
        // The owned spawn value is not retained alongside its split components.
        assert!(world.get::<PlayerSnapshot>(entity).is_none());
        let mut schedule = Schedule::default();
        schedule.add_systems(
            |players: Query<&PlayerData>, mut health: Query<&mut Health>| {
                assert_eq!(players.iter().count(), 1);
                for mut health in &mut health {
                    health.0 = health.0.saturating_sub(25);
                }
            },
        );
        schedule.run(&mut world);
        assert_eq!(world.get::<CharacterMovement>(entity).unwrap().0, original);
        world.get_mut::<CharacterMovement>(entity).unwrap().position =
            Vec3([Scalar(7.0), Scalar(0.02), Scalar(9.0)]);
        let player = world.entity(entity).components::<PlayerQueryReadOnly>();
        let wire = crate::component::snapshots::public_state(&player);
        assert_eq!(wire.health, CharacterHealth(75));
        assert_eq!(
            wire.motor.position,
            Vec3([Scalar(7.0), Scalar(0.02), Scalar(9.0)])
        );
        assert!(wire.equipment.is_some());
    }
}

/// Temporarily exposes split live components to a fixture's scheduled systems.
pub(crate) struct LiveActors<'a>(pub(crate) &'a mut World);

impl std::ops::Deref for LiveActors<'_> {
    type Target = World;
    fn deref(&self) -> &World {
        self.0
    }
}

impl std::ops::DerefMut for LiveActors<'_> {
    fn deref_mut(&mut self) -> &mut World {
        self.0
    }
}

impl Drop for LiveActors<'_> {
    fn drop(&mut self) {
        crate::component::tests::unpack_fixture_actors(self.0);
    }
}
