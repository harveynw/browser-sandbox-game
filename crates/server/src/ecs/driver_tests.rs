//! Driver integration tests, included as a child of tick for private driver access.
use crate::ecs::TickPhase;

use super::*;
use crate::content_loader::WorldContent;
use std::path::Path;
const PHASES: [TickPhase; 10] = [
    TickPhase::Admission,
    TickPhase::Planning,
    TickPhase::Movement,
    TickPhase::Combat,
    TickPhase::Police,
    TickPhase::FailedPeers,
    TickPhase::Repairs,
    TickPhase::Missions,
    TickPhase::Publish,
    TickPhase::Flush,
];
fn driver() -> TickDriver {
    let content = WorldContent::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
    )
    .unwrap();
    TickDriver::new(content, Network::default()).unwrap()
}
#[test]
fn native_startup_and_read_only_diagnostics_share_the_live_actor_world() {
    use crate::{
        ecs::{EntityIndex, GameId},
        entities::vehicle::Vehicle,
        metadata::PoolMembership,
        systems::police::Unit,
    };
    let content = WorldContent::load_index(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
        "city.json",
    )
    .unwrap();
    let mut driver = TickDriver::new(content, Network::default()).unwrap();
    let cars: Vec<_> = driver
        .host
        .query::<(Entity, &GameId, &Vehicle)>()
        .iter(&driver.host)
        .map(|(e, id, _)| (e, id.0))
        .collect();
    assert!(!cars.is_empty());
    let patrols = driver.host.query::<&Unit>().iter(&driver.host).count();
    assert!(
        patrols > 0,
        "initial patrols exist before the first client baseline"
    );
    let parked = driver
        .host
        .query::<&PoolMembership>()
        .iter(&driver.host)
        .count();
    assert!(parked > 0);
    for (entity, id) in &cars {
        assert_eq!(
            driver.host.resource::<EntityIndex>().0.get(&id.0),
            Some(entity)
        );
    }
    for _ in 0..2 {
        let snapshot = driver.diagnostics();
        assert_eq!(snapshot.tick, 0);
        assert_eq!(snapshot.players, 0);
        let metrics = snapshot.metrics.unwrap();
        assert_eq!(metrics["population"]["vehicles"], cars.len());
        assert_eq!(metrics["population"]["police"], patrols);
        assert_eq!(metrics["population"]["parked"], parked);
        assert_eq!(
            metrics["police"]["units"].as_array().unwrap().len(),
            patrols
        );
    }
    assert_eq!(driver.host.resource::<EntityIndex>().0.len(), cars.len());
    driver.step(&mut Profiles::default(), 34, true).unwrap();
    let snapshot = driver.diagnostics();
    assert_eq!(snapshot.tick, 1);
    assert!(
        snapshot.metrics.is_none(),
        "detailed diagnostics retain their once-per-second cadence"
    );
}
#[test]
fn native_shutdown_restores_profile_owner_and_clears_scoped_state() {
    let mut driver = driver();
    let mut profiles = Profiles::default();
    let (profile, cookie) = profiles.session(None).unwrap();
    let tick = driver.tick();
    // A failed final gameplay tick must not suppress lifecycle cleanup.
    driver
        .host
        .resource_mut::<TickOutcome>()
        .fail("injected final-tick error".into());
    assert!(driver.shutdown(&mut profiles));
    assert_eq!(
        driver.tick(),
        tick,
        "idle shutdown does not run gameplay ticks"
    );
    assert_eq!(profiles.profile(cookie.as_deref()).unwrap(), profile);
    assert!(!driver.host.contains_non_send::<TickIo>());
    assert!(driver.host.resource::<TickOutcome>().is_healthy());
    assert!(
        driver
            .host
            .resource::<Messages<crate::sessions::native::DisconnectRequested>>()
            .is_empty()
    );
}
#[derive(Resource, Default)]
struct ObservedActors(Vec<(u32, usize)>);
fn observe_actors(
    cars: Query<&crate::entities::vehicle::Vehicle>,
    clock: Res<crate::ecs::state::Clock>,
    mut observed: ResMut<ObservedActors>,
) {
    observed.0.push((clock.0, cars.iter().count()));
}
#[test]
fn scheduled_queries_see_live_actors_and_resources_on_every_tick() {
    let mut driver = driver();
    let count = driver
        .host
        .query::<&crate::entities::vehicle::Vehicle>()
        .iter(&driver.host)
        .count();
    assert!(count > 0);
    driver.host.init_resource::<ObservedActors>();
    driver.schedule.add_systems(
        observe_actors
            .in_set(TickPhase::Publish)
            .after(crate::replication::publication::finish),
    );
    let mut profiles = Profiles::default();
    for tick in 1..=3 {
        driver.step(&mut profiles, tick * 34, true).unwrap();
    }
    assert_eq!(
        driver.host.resource::<ObservedActors>().0,
        [(1, count), (2, count), (3, count)]
    );
}
#[test]
fn live_schedule_runs_each_phase_once_and_advances_one_tick() {
    let mut driver = driver();
    let mut profiles = Profiles::default();
    for expected in 1..=3 {
        driver
            .step(&mut profiles, expected as u64 * 34, true)
            .unwrap();
        assert_eq!(driver.tick(), expected);
        let scratch = driver.host.resource::<Scratch>();
        assert_eq!(scratch.trace, PHASES);
        assert!(scratch.frame.is_none());
        assert!(scratch.gameplay_started.is_none());
        assert!(!driver.host.contains_non_send::<TickIo>());
    }
}
#[test]
fn every_failed_phase_stops_later_work_restores_profiles_and_clears_scratch() {
    for (index, phase) in PHASES.iter().copied().enumerate() {
        let mut driver = driver();
        let mut profiles = Profiles::default();
        let (profile, cookie) = profiles.session(None).unwrap();
        let cookie = cookie.unwrap();
        driver.host.resource_mut::<Scratch>().fail_at = Some(phase);
        assert_eq!(
            driver.step(&mut profiles, 34, true),
            Err(format!("injected {phase:?} failure"))
        );
        let scratch = driver.host.resource::<Scratch>();
        assert_eq!(scratch.trace, &PHASES[..=index]);
        assert!(scratch.frame.is_none());
        assert!(scratch.gameplay_started.is_none());
        assert!(!driver.host.contains_non_send::<TickIo>());
        assert_eq!(
            profiles.session(Some(&cookie)).unwrap().0.profile_id.0,
            profile.profile_id.0
        );
        // Pre-movement failures do not commit the clock; post-movement failures do.
        let committed = if index <= 2 { 0 } else { 1 };
        assert_eq!(driver.tick(), committed);
        driver.step(&mut profiles, 68, true).unwrap();
        assert_eq!(driver.tick(), committed + 1);
        assert_eq!(driver.host.resource::<Scratch>().trace, PHASES);
    }
}
#[test]
fn tick_exhaustion_rejects_before_admission_and_leaves_profiles_owned_by_caller() {
    let mut driver = driver();
    let mut profiles = Profiles::default();
    let (profile, cookie) = profiles.session(None).unwrap();
    driver.host.resource_mut::<crate::ecs::state::Clock>().0 = u32::MAX - 1;
    assert!(
        driver
            .step(&mut profiles, 34, true)
            .unwrap_err()
            .contains("tick exhausted")
    );
    assert!(driver.host.resource::<Scratch>().trace.is_empty());
    assert!(!driver.host.contains_non_send::<TickIo>());
    assert_eq!(
        profiles.session(cookie.as_deref()).unwrap().0.profile_id.0,
        profile.profile_id.0
    );
}
