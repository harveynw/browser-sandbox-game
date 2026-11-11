use super::*;
#[derive(Component, Debug, PartialEq)]
struct Position(i32);
#[test]
fn identity_mutation_and_despawn_do_not_alias_reused_storage() {
    let mut store = EntityStore::new();
    let old = store.spawn(EntityId(9), (Position(10),)).unwrap();
    store.get_mut::<Position>(EntityId(9)).unwrap().0 = 12;
    assert_eq!(store.get::<Position>(EntityId(9)), Some(&Position(12)));
    assert!(store.spawn(EntityId(9), ()).is_err());
    assert!(store.despawn(EntityId(9)));
    let new = store.spawn(EntityId(10), (Position(20),)).unwrap();
    assert_ne!(old, new);
    assert!(store.get::<Position>(EntityId(9)).is_none());
    assert_eq!(
        store.get::<GameId>(EntityId(10)),
        Some(&GameId(EntityId(10)))
    );
    assert!(!store.despawn(EntityId(9)));
}
#[test]
fn ordering_does_not_depend_on_spawn_order() {
    let mut store = EntityStore::new();
    for id in [8, 2, 4] {
        store.spawn(EntityId(id), ()).unwrap();
    }
    assert_eq!(
        store
            .ordered()
            .iter()
            .map(|(id, _)| id.0)
            .collect::<Vec<_>>(),
        [2, 4, 8]
    );
}
#[derive(Component, Debug, PartialEq)]
struct Controller(u32);
#[test]
fn components_share_identity_and_partial_removal_preserves_other_state() {
    let mut store = EntityStore::new();
    store.insert(9, Position(20));
    let entity = store.entity(EntityId(9)).unwrap();
    store.insert(9, Controller(3));
    assert_eq!(store.entity(EntityId(9)), Some(entity));
    assert_eq!(store.view::<Position>().len(), 1);
    assert_eq!(store.view::<Controller>().len(), 1);
    assert_eq!(store.remove::<Controller>(&9), Some(Controller(3)));
    assert_eq!(store.entity(EntityId(9)), Some(entity));
    assert_eq!(store.get::<Position>(EntityId(9)), Some(&Position(20)));
    assert_eq!(store.remove::<Position>(&9), Some(Position(20)));
    assert!(store.entity(EntityId(9)).is_none());
    assert!(!store.world.entities().contains(entity));
}
#[test]
fn ordered_mutation_survives_archetype_changes_and_reused_slots() {
    let mut store = EntityStore::new();
    for id in [9, 2, 6] {
        store.insert(id, Position(0));
    }
    store.remove::<Position>(&2);
    store.insert(1, Position(0));
    store.insert(2, Position(0));
    store.insert(6, Controller(1));
    for (i, position) in store.values_mut::<Position>().enumerate() {
        position.0 = i as i32;
    }
    assert_eq!(
        store
            .view::<Position>()
            .iter()
            .map(|(id, p)| (*id, p.0))
            .collect::<Vec<_>>(),
        [(1, 0), (2, 1), (6, 2), (9, 3)]
    );
    store.retain::<Position>(|id, _| *id % 2 == 0);
    assert_eq!(
        store.view::<Position>().keys().copied().collect::<Vec<_>>(),
        [2, 6]
    );
    store.clear::<Position>();
    assert_eq!(store.get::<Controller>(EntityId(6)), Some(&Controller(1)));
    assert!(store.view::<Position>().is_empty());
}
#[test]
fn metadata_values_keep_first_home_and_share_actor_lifecycle() {
    use crate::metadata::{DisabledVehicle, PoolMembership, VehicleHome};
    use crate::systems::population::VehiclePool;
    let mut store = EntityStore::new();
    store.insert(8, Position(0));
    let actor = store.entity(EntityId(8)).unwrap();
    store.insert_value::<VehicleHome>(8, ([1.0, 0.0, 2.0], 0.5));
    store.value_or_insert::<VehicleHome>(8, ([99.0; 3], 1.0));
    assert_eq!(store.values::<VehicleHome>()[&8], ([1.0, 0.0, 2.0], 0.5));
    store.insert_value::<PoolMembership>(8, VehiclePool::Traffic);
    store.insert_value::<DisabledVehicle>(8, (900, 0));
    store.value_mut::<DisabledVehicle>(&8).unwrap().1 += 1;
    assert_eq!(store.values::<DisabledVehicle>().get(&8), Some(&(900, 1)));
    assert_eq!(store.entity(EntityId(8)), Some(actor));
    store.remove::<Position>(&8);
    store.remove::<DisabledVehicle>(&8);
    store.remove::<PoolMembership>(&8);
    assert!(store.entity(EntityId(8)).is_some());
    store.remove::<VehicleHome>(&8);
    assert!(store.entity(EntityId(8)).is_none());
}
#[derive(Resource, Default)]
struct Trace(Vec<&'static str>);
fn admission(mut trace: ResMut<Trace>) {
    trace.0.push("admission");
}
fn fail(mut trace: ResMut<Trace>, mut outcome: ResMut<TickOutcome>) {
    trace.0.push("movement");
    outcome.fail("unsafe movement".into());
}
fn publish(mut trace: ResMut<Trace>) {
    trace.0.push("publish");
}
#[test]
fn ordered_schedule_stops_later_phases_and_returns_first_error() {
    let mut world = World::new();
    world.init_resource::<TickOutcome>();
    world.init_resource::<Trace>();
    let mut schedule = tick_schedule();
    // Reverse registration order to exercise dependency ordering.
    schedule.add_systems(publish.in_set(TickPhase::Publish));
    schedule.add_systems(fail.in_set(TickPhase::Movement));
    schedule.add_systems(admission.in_set(TickPhase::Admission));
    schedule.run(&mut world);
    assert_eq!(world.resource::<Trace>().0, ["admission", "movement"]);
    assert_eq!(
        world.resource_mut::<TickOutcome>().finish(),
        Err("unsafe movement".into())
    );
    assert!(world.resource_mut::<TickOutcome>().finish().is_ok());
}
