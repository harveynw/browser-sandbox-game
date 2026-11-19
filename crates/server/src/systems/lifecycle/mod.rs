//! Native actor lifecycle systems. Removal and identity bookkeeping commit together.
pub(crate) mod players;
#[cfg(test)]
use crate::component::PedestrianSnapshot;
#[cfg(test)]
use crate::component::tests::ActorWorldExt as _;
use crate::{
    component::Incapacitated,
    ecs::state::Clock,
    ecs::{EntityIndex, GameId},
};
use bevy_ecs::prelude::*;

/// Removes an actor only if its current identity matches the indexed entity.
pub(crate) fn despawn_indexed(world: &mut World, entity: Entity, id: protocol::EntityId) {
    // A deferred command must never delete a replacement with the same game ID.
    if !world
        .get::<GameId>(entity)
        .is_some_and(|current| current.0 == id)
    {
        return;
    }
    world.despawn(entity);
    let mut index = world.resource_mut::<EntityIndex>();
    if index.0.get(&id.0) == Some(&entity) {
        index.0.remove(&id.0);
    }
}

/// Removes incapacitated pedestrians when their cleanup deadline expires.
pub(crate) fn despawn_expired_pedestrians(
    pedestrians: Query<(Entity, &GameId, &Incapacitated), With<crate::component::PedestrianData>>,
    clock: Res<Clock>,
    mut commands: Commands,
) {
    let mut expired: Vec<_> = pedestrians
        .iter()
        .filter(|(_, _, due)| due.0 <= clock.0)
        .map(|(entity, id, _)| (id.0, entity))
        .collect();
    expired.sort_by_key(|(id, _)| id.0);
    for (id, entity) in expired {
        commands.queue(move |world: &mut World| despawn_indexed(world, entity, id));
    }
}

type Orphans<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static GameId),
    (
        With<Incapacitated>,
        Without<crate::component::PedestrianData>,
    ),
>;

/// Removes stale incapacitation markers and empty actor shells.
pub(crate) fn remove_orphaned_incapacitation(orphans: Orphans<'_, '_>, mut commands: Commands) {
    for (entity, id) in &orphans {
        let id = id.0;
        commands.queue(move |world: &mut World| {
            if !world
                .get::<GameId>(entity)
                .is_some_and(|current| current.0 == id)
            {
                return;
            }
            world.entity_mut(entity).remove::<Incapacitated>();
            // Preserve actors with other components; discard identity-only shells.
            if world.entity(entity).archetype().components().len() == 1 {
                despawn_indexed(world, entity, id);
            }
        });
    }
}

/// Borrows geometry and nearby actors to find safe vehicle restoration positions.
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct RestorationSpace<'w, 's> {
    /// Queries players used by this system.
    players: Query<
        'w,
        's,
        crate::component::queries::PlayerQueryReadOnly,
        crate::component::queries::PlayerFilter,
    >,
    /// Queries pedestrians used by this system.
    pedestrians: Query<
        'w,
        's,
        crate::component::queries::PedestrianQueryReadOnly,
        crate::component::queries::PedestrianFilter,
    >,
    /// Collision queries used to validate movement and placement.
    collision: NonSendMut<'w, crate::ecs::state::Collision>,
    /// Validated world content used by this operation.
    content: Res<'w, crate::ecs::state::Content>,
    /// Current simulation tick resource.
    clock: Res<'w, Clock>,
}
type RestorableVehicles<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static GameId,
        &'static mut crate::component::DisabledVehicle,
        Option<crate::component::queries::VehicleQuery>,
        Option<&'static crate::component::VehicleHome>,
        Has<crate::systems::police::Unit>,
    ),
    crate::component::queries::VehicleFilter,
>;

/// Restores disabled vehicles when their deadline and placement checks permit it.
pub(crate) fn restore_vehicles(
    mut vehicles: RestorableVehicles,
    mut space: RestorationSpace,
    mut restored: MessageWriter<crate::gameplay_messages::VehicleRestored>,
    mut commands: Commands,
    mut outcome: ResMut<crate::ecs::TickOutcome>,
) {
    use sim_core::CollisionQueries;
    let mut actors: Vec<_> = vehicles.iter_mut().collect();
    actors.sort_by_key(|(_, id, ..)| id.0.0);
    for (entity, id, mut disabled, car, home, police) in actors {
        let (due, retries) = disabled.0;
        if due > space.clock.0 || police {
            continue;
        }
        let Some(mut car) = car else {
            let id = id.0;
            commands.queue(move |world: &mut World| {
                if !world
                    .get::<GameId>(entity)
                    .is_some_and(|current| current.0 == id)
                {
                    return;
                }
                world
                    .entity_mut(entity)
                    .remove::<crate::component::DisabledVehicle>();
                if world.entity(entity).archetype().components().len() == 1 {
                    despawn_indexed(world, entity, id);
                }
            });
            continue;
        };
        if car.state.driver_id.is_some() {
            continue;
        }
        let Some(home) = home else {
            outcome.fail("restoring vehicle has no home".into());
            return;
        };
        let template = car.template.clone();
        let mut candidates = vec![home.0];
        if retries >= 30 {
            candidates.extend(
                space
                    .content
                    .0
                    .manifest
                    .parking
                    .iter()
                    .filter(|p| !p.mission_reserved && p.template_id == template.id)
                    .map(|p| (p.position.map(|v| v as f32), p.yaw as f32)),
            );
        }
        space.collision.0.exclude_vehicle(Some(id.0.0));
        let spot = candidates.into_iter().find(|(p, yaw)| {
            !space
                .collision
                .0
                .overlaps_vehicle(*p, *yaw, template.dimensions.map(|v| v as f32))
                .unwrap_or(true)
                && space.players.iter().all(|actor| {
                    let at = actor.state.motor.position.0.map(|v| v.0);
                    (p[0] - at[0]).hypot(p[2] - at[2]) > 6.0
                })
                && space.pedestrians.iter().all(|actor| {
                    let at = actor.state.motor.position.0.map(|v| v.0);
                    (p[0] - at[0]).hypot(p[2] - at[2]) > 6.0
                })
        });
        space.collision.0.exclude_vehicle(None);
        if let Some((position, yaw)) = spot {
            let motor = match sim_core::spawn_vehicle(position, yaw) {
                Ok(motor) => motor,
                Err(error) => {
                    outcome.fail(error.to_string());
                    return;
                }
            };
            car.state.motor.0 = motor;
            car.state.health.0 = template.health;
            car.state.disabled = false;
            car.idle_ticks = 30;
            commands
                .entity(entity)
                .remove::<crate::component::DisabledVehicle>();
            restored.write(crate::gameplay_messages::VehicleRestored {
                vehicle: id.0,
                entity,
                tick: space.clock.0,
            });
        } else {
            disabled.0 = (due.saturating_add(30), retries.saturating_add(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::*;
    /// Creates a pedestrian actor for lifecycle tests.
    fn pedestrian(id: u32) -> PedestrianSnapshot {
        PedestrianSnapshot {
            id: EntityId(id),
            state: CharacterState {
                equipment: None,
                motor: sim_core::spawn_character([0.0; 3], 0.0).unwrap(),
                health: CharacterHealth(0),
                mode: CharacterMode::Downed,
                vehicle_id: None,
            },
            phase: crate::systems::population::PedestrianPhase::Incapacitated,
            from: 0,
            to: 0,
            input: sim_core::neutral_input(0, Yaw(0.0)),
            wait_until: 0,
            far_since: None,
            progress_at: 0,
            progress_position: [0.0; 3],
            flee_until: 0,
            threat: [0.0; 3],
        }
    }
    /// Detects remaining actor components before removing an empty entity.
    #[derive(Component)]
    struct AttachedState;
    /// Verifies that expiry removes actor and index before the next system.
    #[test]
    fn expiry_removes_actor_and_index_before_the_next_system() {
        let mut world = World::new();
        world.init_resource::<EntityIndex>();
        world.insert_resource(Clock(299));
        let entity = world
            .spawn((
                GameId(EntityId(7)),
                pedestrian(7).into_bundle(),
                Incapacitated(300),
                AttachedState,
            ))
            .id();
        world.resource_mut::<EntityIndex>().0.insert(7, entity);
        let mut schedule = Schedule::default();
        schedule.add_systems(
            (
                despawn_expired_pedestrians,
                |clock: Res<Clock>,
                 index: Res<EntityIndex>,
                 peds: Query<
                    crate::component::queries::PedestrianQueryReadOnly,
                    crate::component::queries::PedestrianFilter,
                >| {
                    if clock.0 >= 300 {
                        assert!(peds.is_empty());
                        assert!(!index.0.contains_key(&7));
                    }
                },
            )
                .chain(),
        );
        schedule.run(&mut world);
        assert!(
            world
                .actor::<crate::component::queries::PedestrianQueryReadOnly>(entity)
                .is_some()
        );
        world.resource_mut::<Clock>().0 = 300;
        schedule.run(&mut world);
        assert!(world.get_entity(entity).is_err());
        assert!(
            world
                .query::<&AttachedState>()
                .iter(&world)
                .next()
                .is_none()
        );
        schedule.run(&mut world);
    }
    /// Verifies that stale deferred deletion preserves replacement identity.
    #[test]
    fn stale_deferred_deletion_preserves_replacement_identity() {
        let mut world = World::new();
        world.init_resource::<EntityIndex>();
        let old = world
            .spawn((GameId(EntityId(7)), pedestrian(7).into_bundle()))
            .id();
        world.resource_mut::<EntityIndex>().0.insert(7, old);
        let mut queue = bevy_ecs::world::CommandQueue::default();
        Commands::new(&mut queue, &world)
            .queue(move |world: &mut World| despawn_indexed(world, old, EntityId(7)));
        world.despawn(old);
        let replacement = world
            .spawn((GameId(EntityId(7)), pedestrian(7).into_bundle()))
            .id();
        world.resource_mut::<EntityIndex>().0.insert(7, replacement);
        queue.apply(&mut world);
        assert!(
            world
                .actor::<crate::component::queries::PedestrianQueryReadOnly>(replacement)
                .is_some()
        );
        assert_eq!(world.resource::<EntityIndex>().0[&7], replacement);
    }
    /// Verifies that orphan cleanup removes empty shells but preserves other components.
    #[test]
    fn orphan_cleanup_removes_empty_shells_but_preserves_other_components() {
        let mut world = World::new();
        world.init_resource::<EntityIndex>();
        let empty = world.spawn((GameId(EntityId(7)), Incapacitated(300))).id();
        let occupied = world
            .spawn((GameId(EntityId(8)), Incapacitated(300), AttachedState))
            .id();
        world
            .resource_mut::<EntityIndex>()
            .0
            .extend([(7, empty), (8, occupied)]);
        let mut schedule = Schedule::default();
        schedule.add_systems(remove_orphaned_incapacitation);
        schedule.run(&mut world);
        assert!(world.get_entity(empty).is_err());
        assert!(!world.resource::<EntityIndex>().0.contains_key(&7));
        assert!(world.get::<AttachedState>(occupied).is_some());
        assert!(world.get::<Incapacitated>(occupied).is_none());
        assert_eq!(world.resource::<EntityIndex>().0[&8], occupied);
    }
}
