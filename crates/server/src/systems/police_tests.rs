use super::*;

fn city() -> (World, Sessions, crate::systems::Systems) {
    let mut world = World::new(
        crate::content_loader::WorldContent::load_index(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../apps/client/public/content"),
            "city.json",
        )
        .unwrap(),
    )
    .unwrap();
    let mut sessions = Sessions::default();
    let systems = crate::systems::Systems::new(&mut world, &mut sessions).unwrap();
    (world, sessions, systems)
}

// Preserve patrol coverage in both home districts when retired police vehicles are replaced.

#[test]
fn retired_patrol_replacement_preserves_both_home_districts() {
    let (mut w, mut sessions, mut systems) = city();
    for cycle in 0..6 {
        let home = if cycle % 2 == 0 {
            [-192.0, 0.0, 160.0]
        } else {
            [0.0, 0.0, -160.0]
        };
        let id = *systems
            .police
            .units
            .iter()
            .find(|(_, u)| u.home == home)
            .unwrap()
            .0;
        systems.police.units.remove(&id);
        w.vehicles.remove(&id);
        w.collision
            .set_vehicle_proxies(&w.vehicles.values().map(Vehicle::proxy).collect::<Vec<_>>())
            .unwrap();
        systems
            .police
            .spawn_patrols(
                (cycle + 1) * 30,
                &[],
                &mut w,
                &mut sessions,
                &mut systems.combat,
            )
            .unwrap();
        assert_eq!(systems.police.units.len(), 2);
        assert_eq!(
            systems
                .police
                .units
                .values()
                .filter(|u| u.home == [-192.0, 0.0, 160.0])
                .count(),
            1,
            "Quay must receive its replacement"
        );
        assert_eq!(
            systems
                .police
                .units
                .values()
                .filter(|u| u.home == [0.0, 0.0, -160.0])
                .count(),
            1,
            "Civic must receive its replacement"
        );
    }
}

// Use shared vehicle physics, remain collision-safe and avoid recycling visible patrols.

#[test]
fn patrols_use_shared_motor_and_remain_collision_safe_without_visible_recycling() {
    let (mut w, mut sessions, mut systems) = city();
    assert_eq!(systems.police.units.len(), 2);
    let initial: BTreeMap<_, _> = systems
        .police
        .units
        .keys()
        .map(|id| (*id, w.vehicles[id].state.motor.position.0.map(|v| v.0)))
        .collect();
    let mut travelled = BTreeMap::<u32, f32>::new();
    for tick in 1..=900 {
        w.collision
            .set_vehicle_proxies(&w.vehicles.values().map(Vehicle::proxy).collect::<Vec<_>>())
            .unwrap();
        // Observers at each unit prevent the stuck-retirement branch from hiding failures.
        let observers: Vec<_> = systems
            .police
            .units
            .keys()
            .map(|id| Observer {
                position: w.vehicles[id].state.motor.position.0.map(|v| v.0),
                yaw: 0.0,
            })
            .collect();
        systems
            .police
            .spawn_patrols(tick, &observers, &mut w, &mut sessions, &mut systems.combat)
            .unwrap();
        systems
            .police
            .prepare_tick(tick, &[], &mut w, &mut sessions, &mut systems.population)
            .unwrap();
        for id in systems.police.units.keys().copied().collect::<Vec<_>>() {
            let car = w.vehicles.get_mut(&id).unwrap();
            let before = car.state.motor.position.0.map(|v| v.0);
            let input = systems
                .police
                .vehicle_input(id, tick, car.state.motor.speed.0)
                .unwrap();
            w.collision.exclude_vehicle(Some(id));
            car.state.motor = sim_core::step_vehicle(
                &car.state.motor,
                &input,
                &car.template,
                &sim_core::StepContext {
                    tick,
                    collision: &w.collision,
                    config: &sim_core::SimConfig::from_content(&w.content.config).unwrap(),
                },
            )
            .unwrap()
            .motor;
            assert!(
                !w.collision
                    .overlaps_vehicle(
                        car.state.motor.position.0.map(|v| v.0),
                        car.state.motor.yaw.0,
                        car.template.dimensions.map(|v| v as f32)
                    )
                    .unwrap()
            );
            assert!(car.state.motor.speed.0.abs() <= 12.01);
            *travelled.entry(id).or_default() +=
                distance(before, car.state.motor.position.0.map(|v| v.0));
            w.collision
                .set_vehicle_proxies(&w.vehicles.values().map(Vehicle::proxy).collect::<Vec<_>>())
                .unwrap();
        }
        w.collision.exclude_vehicle(None);
    }
    assert_eq!(
        systems.police.units.keys().copied().collect::<Vec<_>>(),
        initial.keys().copied().collect::<Vec<_>>()
    );
    println!("patrol distance over 900 ticks: {travelled:?}");
    assert!(travelled.values().all(|d| *d > 40.0));
}

// Distinguish hearing from sight, respect walls and range, and report fatal hits against police vehicles.

#[test]
fn witnesses_distinguish_acoustics_static_occlusion_range_and_fatal_police_hits() {
    let (mut w, mut sessions, mut systems) = city();
    let id = *systems.police.units.keys().next().unwrap();
    // The ray crosses a real building; a sound still reaches the same police car.
    let collider = w
        .content
        .collision
        .colliders
        .iter()
        .find(|c| c.flags & 2 != 0 && c.half_extents[1] > 2.0)
        .unwrap();
    let center = collider.center.map(|v| v as f32);
    let left = [
        center[0] - collider.half_extents[0] as f32 - 2.0,
        0.0,
        center[2],
    ];
    let right = [
        center[0] + collider.half_extents[0] as f32 + 2.0,
        0.0,
        center[2],
    ];
    assert!(!sight(&w.collision, left, right, 60.0));
    assert!(distance(left, right) < 80.0);
    w.vehicles.get_mut(&id).unwrap().state.motor = sim_core::spawn_vehicle(left, 0.0).unwrap();
    // Report needs an actor in EntityStore, but never trusts a client crime message.
    let player = 999;
    w.players.insert(
        player,
        crate::entities::player::Player {
            tshirt_colour: Default::default(),
            weapon: Default::default(),
            lifecycle: Default::default(),
            id: EntityId(player),
            display_name: DisplayName("fixture".into()),
            slot: 0,
            state: CharacterState {
                equipment: None,
                motor: sim_core::spawn_character(right, 0.0).unwrap(),
                health: CharacterHealth(100),
                mode: CharacterMode::OnFoot,
                vehicle_id: None,
            },
        },
    );
    let others: Vec<_> = systems
        .police
        .units
        .keys()
        .copied()
        .filter(|u| *u != id)
        .collect();
    for other in others {
        w.vehicles.get_mut(&other).unwrap().state.health = VehicleHealth(0);
    }
    systems.police.report_crime(
        player,
        CrimeId::Theft(1, 1),
        CrimeKind::Theft,
        right,
        &mut w,
    );
    systems.police.report_crime(
        player,
        CrimeId::Impact(1, 1),
        CrimeKind::Harm,
        right,
        &mut w,
    );
    assert!(systems.police.crimes.is_empty());
    systems
        .police
        .report_crime(player, CrimeId::Shot(1), CrimeKind::Shot, right, &mut w);
    systems
        .police
        .report_crime(player, CrimeId::Shot(1), CrimeKind::Shot, right, &mut w);
    assert_eq!(systems.police.crimes.len(), 1);
    systems.police.crimes.clear();
    systems.police.report_crime(
        player,
        CrimeId::Shot(2),
        CrimeKind::Shot,
        [left[0] + 81.0, 0.0, left[2]],
        &mut w,
    );
    assert!(systems.police.crimes.is_empty());
    w.vehicles.get_mut(&id).unwrap().state.health = VehicleHealth(0);
    systems.police.report_crime(
        player,
        CrimeId::Shot(3),
        CrimeKind::PoliceCar(id),
        [left[0] + 1.0, left[1], left[2]],
        &mut w,
    );
    assert_eq!(systems.police.crimes.len(), 1);
}
