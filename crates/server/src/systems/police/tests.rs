use super::*;
use crate::entities::player::Player as PlayerComponent;
use crate::entities::vehicle::Vehicle as VehicleComponent;
use crate::systems::police::Unit as PoliceUnitComponent;

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

// Preserve coverage in all home districts when retired patrols are replaced.
#[test]
fn retired_patrol_replacement_preserves_all_home_districts() {
    let (mut w, mut sessions, mut systems) = city();
    for (cycle, home) in PATROL_HOMES.into_iter().enumerate() {
        let id = *w
            .entities
            .view::<PoliceUnitComponent>()
            .iter()
            .find(|(_, u)| u.home == home)
            .unwrap()
            .0;
        w.entities.remove::<PoliceUnitComponent>(&id);
        w.entities.remove::<VehicleComponent>(&id);
        w.collision
            .set_vehicle_proxies(
                &w.entities
                    .view::<VehicleComponent>()
                    .values()
                    .map(Vehicle::proxy)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        systems
            .police
            .spawn_patrols(
                (cycle as u32 + 1) * 30,
                &[],
                &mut w,
                &mut sessions,
                &mut systems.combat,
            )
            .unwrap();
        assert_eq!(
            w.entities.view::<PoliceUnitComponent>().len(),
            PATROL_HOMES.len()
        );
        for home in PATROL_HOMES {
            assert_eq!(
                w.entities
                    .view::<PoliceUnitComponent>()
                    .values()
                    .filter(|u| u.home == home)
                    .count(),
                1
            );
        }
    }
}

// Use shared vehicle physics, remain collision-safe and avoid recycling visible patrols.

#[test]
fn patrols_use_shared_motor_and_remain_collision_safe_without_visible_recycling() {
    let (mut w, mut sessions, mut systems) = city();
    assert_eq!(
        w.entities.view::<PoliceUnitComponent>().len(),
        PATROL_HOMES.len()
    );
    let initial: BTreeMap<_, _> = w
        .entities
        .view::<PoliceUnitComponent>()
        .keys()
        .map(|id| {
            (
                *id,
                w.entities.view::<VehicleComponent>()[id]
                    .state
                    .motor
                    .position
                    .0
                    .map(|v| v.0),
            )
        })
        .collect();
    let mut travelled = BTreeMap::<u32, f32>::new();
    for tick in 1..=900 {
        w.collision
            .set_vehicle_proxies(
                &w.entities
                    .view::<VehicleComponent>()
                    .values()
                    .map(Vehicle::proxy)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        // Keep every patrol visible while checking that it retains its identity.
        let observers: Vec<_> = w
            .entities
            .view::<PoliceUnitComponent>()
            .keys()
            .map(|id| Observer {
                position: w.entities.view::<VehicleComponent>()[id]
                    .state
                    .motor
                    .position
                    .0
                    .map(|v| v.0),
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
        for id in w
            .entities
            .view::<PoliceUnitComponent>()
            .keys()
            .copied()
            .collect::<Vec<_>>()
        {
            let car = w.entities.view::<VehicleComponent>().get(&id).unwrap();
            let before = car.state.motor.position.0.map(|v| v.0);
            let input = systems
                .police
                .vehicle_input(id, tick, car.state.motor.speed.0, &w)
                .unwrap();
            w.collision.exclude_vehicle(Some(id));
            let car = w.entities.get_mut_by_id::<VehicleComponent>(&id).unwrap();
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
                .set_vehicle_proxies(
                    &w.entities
                        .view::<VehicleComponent>()
                        .values()
                        .map(Vehicle::proxy)
                        .collect::<Vec<_>>(),
                )
                .unwrap();
        }
        w.collision.exclude_vehicle(None);
    }
    assert_eq!(
        w.entities
            .view::<PoliceUnitComponent>()
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        initial.keys().copied().collect::<Vec<_>>()
    );
    println!("patrol distance over 900 ticks: {travelled:?}");
    assert!(travelled.values().all(|d| *d > 40.0));
}

// Distinguish hearing from sight, respect walls and range, and report fatal hits against police vehicles.

#[test]
fn witnesses_distinguish_acoustics_static_occlusion_range_and_fatal_police_hits() {
    let (mut w, _sessions, mut systems) = city();
    let id = *w
        .entities
        .view::<PoliceUnitComponent>()
        .keys()
        .next()
        .unwrap();
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
    w.entities
        .get_mut_by_id::<VehicleComponent>(&id)
        .unwrap()
        .state
        .motor = sim_core::spawn_vehicle(left, 0.0).unwrap();
    // Report needs an actor in EntityStore, but never trusts a client crime message.
    let player = 999;
    w.entities.insert(
        player,
        PlayerComponent {
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
    let others: Vec<_> = w
        .entities
        .view::<PoliceUnitComponent>()
        .keys()
        .copied()
        .filter(|u| *u != id)
        .collect();
    for other in others {
        w.entities
            .get_mut_by_id::<VehicleComponent>(&other)
            .unwrap()
            .state
            .health = VehicleHealth(0);
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
    systems
        .police
        .report_crime(player, CrimeId::Shot(3), CrimeKind::Shot, right, &mut w);
    assert_eq!(
        systems.police.crimes[&(player, CrimeId::Shot(3))]
            .kind
            .points(),
        1
    );
    w.entities
        .get_mut_by_id::<VehicleComponent>(&id)
        .unwrap()
        .state
        .health = VehicleHealth(0);
    systems.police.report_crime(
        player,
        CrimeId::Shot(3),
        CrimeKind::PoliceCar(id),
        [left[0] + 1.0, left[1], left[2]],
        &mut w,
    );
    assert_eq!(systems.police.crimes.len(), 1);
    let report = systems.police.crimes[&(player, CrimeId::Shot(3))];
    assert_eq!(report.kind.points(), 10);
}

#[test]
fn recovery_rechecks_rear_clearance_before_reversing() {
    let (mut world, mut sessions, mut systems) = city();
    let id = *world
        .entities
        .view::<PoliceUnitComponent>()
        .keys()
        .next()
        .unwrap();
    let car = &world.entities.view::<VehicleComponent>()[&id];
    let yaw = car.state.motor.yaw.0;
    let position = car.state.motor.position.0.map(|v| v.0);
    let rear = car.template.dimensions[2] as f32 / 2.0 + 0.4;
    let observer = Observer {
        position: [
            position[0] + yaw.sin() * rear,
            position[1],
            position[2] + yaw.cos() * rear,
        ],
        yaw,
    };
    world
        .entities
        .get_mut_by_id::<PoliceUnitComponent>(&id)
        .unwrap()
        .reverse_until = 45;
    systems
        .police
        .prepare_tick(
            1,
            &[observer],
            &mut world,
            &mut sessions,
            &mut systems.population,
        )
        .unwrap();
    let unit = &world.entities.view::<PoliceUnitComponent>()[&id];
    assert!(unit.blocked);
    assert_eq!(unit.reverse_until, 1);
    assert_eq!(
        systems
            .police
            .vehicle_input(id, 1, 0.0, &world)
            .unwrap()
            .buttons,
        Buttons(4)
    );
}

#[test]
fn message_reports_capture_evidence_bound_aggregation_and_reject_replaced_actors() {
    use crate::gameplay_messages::CrimeReported;
    use bevy_ecs::prelude::*;
    let (mut core, _, systems) = city();
    let actor = EntityId(9999);
    core.entities.insert(
        actor.0,
        PlayerComponent {
            tshirt_colour: Default::default(),
            weapon: Default::default(),
            lifecycle: Default::default(),
            id: actor,
            display_name: DisplayName("fixture".into()),
            slot: 0,
            state: CharacterState {
                equipment: None,
                motor: sim_core::spawn_character([0.0, 0.02, 0.0], 0.0).unwrap(),
                health: CharacterHealth(100),
                mode: CharacterMode::OnFoot,
                vehicle_id: None,
            },
        },
    );
    let original = core.entities.entity(actor).unwrap();
    let evidence = observe_crime(actor.0, CrimeKind::Officer, [31.0, 0.0, 31.0], &core).unwrap();
    crate::gameplay_messages::report_crime(
        actor.0,
        CrimeId::Shot(77),
        CrimeKind::Officer,
        [31.0, 0.0, 31.0],
        &mut core,
    );
    core.entities
        .get_mut::<PlayerComponent>(actor)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([999.0, 0.02, 999.0], 0.0).unwrap();
    let mut world = core.entities.into_world();
    world.insert_resource(systems.police);
    let mut schedule = Schedule::default();
    schedule.add_systems(receive_crimes);
    schedule.run(&mut world);
    let received = world.resource::<PoliceSystem>().crimes[&(actor.0, CrimeId::Shot(77))];
    assert_eq!(received.position, evidence.position);
    assert_eq!(received.confirmed, evidence.confirmed);
    world.resource_mut::<PoliceSystem>().crimes.clear();
    world.resource_mut::<Messages<CrimeReported>>().clear();
    let weak = CrimeReport {
        kind: CrimeKind::Shot,
        position: [0.0; 3],
        confirmed: false,
    };
    for id in 0..257 {
        world.write_message(CrimeReported {
            actor,
            actor_entity: original,
            cause: CrimeId::Shot(id),
            report: weak,
        });
    }
    world.write_message(CrimeReported {
        actor,
        actor_entity: original,
        cause: CrimeId::Shot(0),
        report: evidence,
    });
    schedule.run(&mut world);
    assert_eq!(world.resource::<PoliceSystem>().crimes.len(), 256);
    assert_eq!(
        world.resource::<PoliceSystem>().crimes[&(actor.0, CrimeId::Shot(0))]
            .kind
            .points(),
        10
    );
    assert!(
        !world
            .resource::<PoliceSystem>()
            .crimes
            .contains_key(&(actor.0, CrimeId::Shot(256)))
    );
    world.resource_mut::<PoliceSystem>().crimes.clear();
    let player = world
        .entity_mut(original)
        .take::<PlayerComponent>()
        .unwrap();
    world.despawn(original);
    let replacement = world.spawn((crate::ecs::GameId(actor), player)).id();
    world
        .resource_mut::<crate::ecs::EntityIndex>()
        .0
        .insert(actor.0, replacement);
    world.write_message(CrimeReported {
        actor,
        actor_entity: original,
        cause: CrimeId::Shot(999),
        report: evidence,
    });
    schedule.run(&mut world);
    assert!(world.resource::<PoliceSystem>().crimes.is_empty());
}

#[test]
fn driving_shares_two_route_jobs_and_services_waiting_units_next_tick() {
    let (mut world, mut sessions, mut systems) = city();
    let ids: Vec<_> = world
        .entities
        .view::<PoliceUnitComponent>()
        .keys()
        .copied()
        .collect();
    assert_eq!(ids.len(), 4);
    let before = systems.police.path_jobs;
    for tick in 1..=2 {
        systems
            .police
            .prepare_tick(
                tick,
                &[],
                &mut world,
                &mut sessions,
                &mut systems.population,
            )
            .unwrap();
        assert_eq!(systems.police.path_jobs - before, tick as u64 * 2);
        let units = world.entities.view::<PoliceUnitComponent>();
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(units[id].next_plan > 0, index < tick as usize * 2);
        }
    }
}
