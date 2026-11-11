use super::*;
use server::entities::player::Player as PlayerComponent;
use server::entities::vehicle::Vehicle as VehicleComponent;
use server::systems::police::Wanted as WantedComponent;
fn job(w: &World, t: &mut FakeTransport, p: PeerKey, request: u32, name: &str) {
    vehicle_action(
        w,
        t,
        p,
        request,
        w.sessions.peers[&p].epoch,
        Action::AcceptMission(AcceptMission {
            template_id: TemplateId(name.into()),
        }),
    );
}
fn car_at(w: &mut World, car: u32, pos: [f32; 3]) {
    w.entities
        .get_mut_by_id::<VehicleComponent>(&car)
        .unwrap()
        .state
        .motor = sim_core::spawn_vehicle(pos, 0.0).unwrap();
}
fn enter(
    w: &mut World,
    a: &mut Profiles,
    t: &mut FakeTransport,
    p: PeerKey,
    car: u32,
    request: u32,
) {
    at_door(w, p, car);
    step(w, a, t);
    vehicle_action(
        w,
        t,
        p,
        request,
        w.sessions.peers[&p].epoch,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(car),
        }),
    );
    step(w, a, t);
    assert_eq!(
        w.entities.view::<VehicleComponent>()[&car].state.driver_id,
        w.sessions.peers[&p].player
    );
}
fn ticks(w: &mut World, a: &mut Profiles, t: &mut FakeTransport, p: PeerKey, count: u32) {
    for _ in 0..count {
        keep_alive(w, t, p);
        step(w, a, t);
    }
}
fn repair_setup() -> (World, Profiles, FakeTransport, PeerKey, String) {
    let (mut w, mut a, mut t) = police_setup();
    let p = peer(0);
    let cookie = join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let profile = w.sessions.peers[&p]
        .profile
        .as_ref()
        .unwrap()
        .profile_id
        .0
        .clone();
    let value = a
        .apply_mission_reward(&profile, "repair-fixture", 150)
        .unwrap();
    w.sessions.peers.get_mut(&p).unwrap().profile = Some(value);
    enter(&mut w, &mut a, &mut t, p, 1, 1);
    car_at(&mut w, 1, [220.0, 0.0, -180.0]);
    w.entities
        .get_mut_by_id::<VehicleComponent>(&1)
        .unwrap()
        .state
        .health = VehicleHealth(50);
    step(&mut w, &mut a, &mut t);
    (w, a, t, p, cookie)
}
fn repair(w: &World, t: &mut FakeTransport, p: PeerKey, request: u32) {
    vehicle_action(
        w,
        t,
        p,
        request,
        w.sessions.peers[&p].epoch,
        Action::Repair(Repair {
            zone_id: TemplateId("repair_ironworks".into()),
        }),
    );
}
#[test]
fn repair_waits_sixty_ticks_and_a_commit_then_debits_exactly_once() {
    let (mut w, mut a, mut t, p, _) = repair_setup();
    repair(&w, &mut t, p, 2);
    repair(&w, &mut t, p, 2);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.sessions.peers[&p].repair_view.as_ref().unwrap().phase,
        RepairPhase::Dwell
    );
    ticks(&mut w, &mut a, &mut t, p, 58);
    assert_eq!(w.entities.view::<VehicleComponent>()[&1].state.health.0, 50);
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
        150
    );
    ticks(&mut w, &mut a, &mut t, p, 1);
    assert_eq!(
        w.sessions.peers[&p].repair_view.as_ref().unwrap().phase,
        RepairPhase::DebitPending
    );
    assert_eq!(w.entities.view::<VehicleComponent>()[&1].state.health.0, 50);
    ticks(&mut w, &mut a, &mut t, p, 1);
    assert_eq!(
        w.entities.view::<VehicleComponent>()[&1].state.health.0,
        100
    );
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
        100
    );
    assert_eq!(
        w.sessions.peers[&p].repair_view.as_ref().unwrap().phase,
        RepairPhase::Completed
    );
    repair(&w, &mut t, p, 2);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
        100
    );
}
#[test]
fn delayed_debit_after_disconnect_refunds_and_never_repairs_a_rejoined_guests_car() {
    let (mut w, mut a, mut t, p, cookie) = repair_setup();
    a.repair_delay_ticks = 20;
    repair(&w, &mut t, p, 2);
    step(&mut w, &mut a, &mut t);
    ticks(&mut w, &mut a, &mut t, p, 59);
    assert_eq!(
        w.sessions.peers[&p].repair_view.as_ref().unwrap().phase,
        RepairPhase::DebitPending
    );
    w.disconnect(p, &mut a);
    join(&mut w, &mut a, &mut t, p, Some(&cookie));
    ready(&mut w, &mut a, &mut t, p);
    enter(&mut w, &mut a, &mut t, p, 2, 1);
    w.entities
        .get_mut_by_id::<VehicleComponent>(&2)
        .unwrap()
        .state
        .health = VehicleHealth(35);
    ticks(&mut w, &mut a, &mut t, p, 45);
    assert_eq!(w.entities.view::<VehicleComponent>()[&1].state.health.0, 50);
    assert_eq!(w.entities.view::<VehicleComponent>()[&2].state.health.0, 35);
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
        150
    );
    assert_eq!(
        w.sessions.peers[&p].repair_view.as_ref().unwrap().phase,
        RepairPhase::Cancelled
    );
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::PlayerStatus(s) if s.repair.as_ref().is_some_and(|r|r.phase==RepairPhase::RefundPending))));
}
#[test]
fn memory_worker_reserves_refund_capacity_and_retries_without_double_debit() {
    let mut a = Profiles::default();
    let (profile, _) = a.session(None).unwrap();
    let id = profile.profile_id.0;
    a.apply_mission_reward(&id, "fund", 100).unwrap();
    for n in 0..4093 {
        a.apply_mission_reward(&id, &format!("fill-{n}"), 0)
            .unwrap();
    }
    a.submit_repair_event(&id, "repair:fixture:1", false, 0)
        .unwrap();
    a.submit_repair_event(&id, "repair:fixture:1", false, 0)
        .unwrap();
    assert_eq!(a.poll_economic(0).len(), 0);
    assert_eq!(a.poll_economic(1).len(), 1);
    assert_eq!(a.committed(&id).unwrap().credits.0, 50);
    assert!(a.apply_mission_reward(&id, "overflow", 1).is_err());
    a.submit_repair_event(&id, "refund:repair:fixture:1", true, 1)
        .unwrap();
    assert!(a.poll_economic(2)[0].result.is_ok());
    assert_eq!(a.committed(&id).unwrap().credits.0, 100);
    a.submit_repair_event(&id, "refund:repair:fixture:1", true, 2)
        .unwrap();
    assert!(a.poll_economic(3)[0].result.is_ok());
    assert_eq!(a.committed(&id).unwrap().credits.0, 100);
}

// Require driving through trial checkpoints in order and award the reward only once.
#[test]
fn trial_requires_driving_orders_gates_and_rewards_once() {
    let (mut w, mut a, mut t) = police_setup();
    let p = peer(0);
    let cookie = join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    place(&mut w, p, [0.0, 0.02, -256.0]);
    job(&w, &mut t, p, 1, "trial_city_ring");
    step(&mut w, &mut a, &mut t);
    assert!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .is_none()
    );
    enter(&mut w, &mut a, &mut t, p, 1, 2);
    car_at(&mut w, 1, [0.0, 0.0, -256.0]);
    job(&w, &mut t, p, 3, "trial_city_ring");
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .checkpoint_index,
        1
    );
    car_at(&mut w, 1, [0.0, 0.0, -256.0]);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .checkpoint_index,
        1
    );
    car_at(&mut w, 1, [-256.0, 0.0, 128.0]);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .checkpoint_index,
        1
    );
    let selected_gates = match &w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&p].player_id()]
        .as_ref()
        .unwrap()
        .objective
    {
        server::systems::missions::Objective::TimeTrial { checkpoints, .. } => checkpoints.clone(),
        _ => panic!("expected trial"),
    };
    for point in selected_gates.into_iter().skip(1) {
        car_at(&mut w, 1, point);
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .phase,
        server::systems::missions::Phase::Completed
    );
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
        200
    );
    job(&w, &mut t, p, 3, "trial_city_ring");
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
        200
    );
    w.disconnect(p, &mut a);
    join(&mut w, &mut a, &mut t, p, Some(&cookie));
    ready(&mut w, &mut a, &mut t, p);
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
        200
    );
}

// Keep recovery reservations private, enforce capacity and release them on cancellation, death, timeout, disconnect or destruction.
#[test]
fn recovery_slots_are_private_capped_and_release_on_cancel_death_timeout_disconnect_and_destruction()
 {
    let (mut w, mut a, mut t) = police_setup();
    for n in 0..9 {
        let p = peer(n);
        join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
        place(&mut w, p, [-172.0, 0.02, 180.0]);
        job(&w, &mut t, p, 1, "recovery_docks_quay");
        step(&mut w, &mut a, &mut t);
    }
    assert!(
        w.entities.values::<server::metadata::PlayerMission>()
            [&w.sessions.peers[&peer(8)].player_id()]
            .is_none()
    );
    let ids: BTreeSet<_> = (0..8)
        .map(|n| {
            w.entities.values::<server::metadata::PlayerMission>()
                [&w.sessions.peers[&peer(n)].player_id()]
                .as_ref()
                .unwrap()
                .reserved_vehicle
                .unwrap()
        })
        .collect();
    assert_eq!(ids.len(), 8);
    assert_eq!(
        w.entities
            .values::<server::metadata::PoolMembership>()
            .values()
            .filter(|pool| **pool == server::systems::population::VehiclePool::Parked)
            .count(),
        24
    );
    let car = w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&peer(0)].player_id()]
        .as_ref()
        .unwrap()
        .reserved_vehicle
        .unwrap();
    at_door(&mut w, peer(8), car);
    step(&mut w, &mut a, &mut t);
    vehicle_action(
        &w,
        &mut t,
        peer(8),
        2,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(car),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert!(
        w.entities.view::<VehicleComponent>()[&car]
            .state
            .driver_id
            .is_none()
    );
    vehicle_action(&w, &mut t, peer(0), 2, 1, Action::CancelMission(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert!(!w.entities.view::<VehicleComponent>().contains_key(&car));
    place(&mut w, peer(8), [48.0, 0.02, -180.0]);
    job(&w, &mut t, peer(8), 3, "recovery_docks_quay");
    step(&mut w, &mut a, &mut t);
    assert!(
        w.entities.values::<server::metadata::PlayerMission>()
            [&w.sessions.peers[&peer(8)].player_id()]
            .is_some()
    );
    let disconnected = w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&peer(1)].player_id()]
        .as_ref()
        .unwrap()
        .reserved_vehicle
        .unwrap();
    w.disconnect(peer(1), &mut a);
    assert!(
        !w.entities
            .view::<VehicleComponent>()
            .contains_key(&disconnected)
    );
    let destroyed = w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&peer(2)].player_id()]
        .as_ref()
        .unwrap()
        .reserved_vehicle
        .unwrap();
    w.entities
        .get_mut_by_id::<VehicleComponent>(&destroyed)
        .unwrap()
        .state
        .health = VehicleHealth(0);
    w.entities
        .get_mut_by_id::<VehicleComponent>(&destroyed)
        .unwrap()
        .state
        .disabled = true;
    step(&mut w, &mut a, &mut t);
    assert!(
        !w.entities
            .view::<VehicleComponent>()
            .contains_key(&destroyed)
    );
    assert_eq!(
        w.sessions.peers[&peer(2)]
            .profile
            .as_ref()
            .unwrap()
            .credits
            .0,
        0
    );
    let dead = w.sessions.peers[&peer(3)].player.unwrap().0;
    let dead_car = w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&peer(3)].player_id()]
        .as_ref()
        .unwrap()
        .reserved_vehicle
        .unwrap();
    w.entities
        .get_mut_by_id::<PlayerComponent>(&dead)
        .unwrap()
        .state
        .health = CharacterHealth(0);
    step(&mut w, &mut a, &mut t);
    assert!(
        !w.entities
            .view::<VehicleComponent>()
            .contains_key(&dead_car)
    );
    let expired = w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&peer(4)].player_id()]
        .as_ref()
        .unwrap()
        .reserved_vehicle
        .unwrap();
    let deadline = w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&peer(4)].player_id()]
        .as_ref()
        .unwrap()
        .deadline;
    w.tick = deadline - 1;
    for n in [0, 2, 3, 4, 5, 6, 7, 8] {
        keep_alive(&w, &mut t, peer(n));
    }
    step(&mut w, &mut a, &mut t);
    assert!(!w.entities.view::<VehicleComponent>().contains_key(&expired));
}

// Accept only the reserved recovery car and wait for a safe driver exit before completing garage delivery.
#[test]
fn recovery_only_accepts_its_reserved_car_and_waits_for_a_safe_garage_exit() {
    let (mut w, mut a, mut t) = police_setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    job(&w, &mut t, p, 1, "recovery_docks_quay");
    step(&mut w, &mut a, &mut t);
    let car = w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&p].player_id()]
        .as_ref()
        .unwrap()
        .reserved_vehicle
        .unwrap();
    enter(&mut w, &mut a, &mut t, p, 1, 2);
    car_at(&mut w, 1, [-220.0, 0.0, 48.0]);
    ticks(&mut w, &mut a, &mut t, p, 31);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .dwell,
        0
    );
    vehicle_action(
        &w,
        &mut t,
        p,
        3,
        w.sessions.peers[&p].epoch,
        Action::ExitVehicle(Empty {}),
    );
    step(&mut w, &mut a, &mut t);
    car_at(&mut w, 1, [-152.0, 0.0, 176.0]);
    enter(&mut w, &mut a, &mut t, p, car, 4);
    car_at(&mut w, car, [-220.0, 0.0, 48.0]);
    let offsets = w.entities.view::<VehicleComponent>()[&car]
        .template
        .exit_offsets
        .clone();
    for (index, offset) in offsets.iter().enumerate() {
        let q = peer(index as u32 + 1);
        join(&mut w, &mut a, &mut t, q, None);
        ready(&mut w, &mut a, &mut t, q);
        let mut pos = sim_core::anchor(
            &w.entities.view::<VehicleComponent>()[&car].state.motor,
            *offset,
        );
        pos[1] = 0.02;
        place(&mut w, q, pos);
    }
    ticks(&mut w, &mut a, &mut t, p, 35);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .phase,
        server::systems::missions::Phase::Active
    );
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .checkpoint_index,
        2
    );
    assert_eq!(w.sessions.peers[&p].profile.as_ref().unwrap().credits.0, 0);
    for n in 1..=offsets.len() {
        place(
            &mut w,
            peer(n as u32),
            [-183.0 + n as f32 * 2.0, 0.02, 177.0],
        );
    }
    ticks(&mut w, &mut a, &mut t, p, 2);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .phase,
        server::systems::missions::Phase::Completed
    );
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
        250
    );
    let owner = w.sessions.peers[&p].player.unwrap().0;
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&owner].state.mode,
        CharacterMode::OnFoot
    );
    assert!(!w.entities.view::<VehicleComponent>().contains_key(&car));
    let position = w.entities.view::<PlayerComponent>()[&owner]
        .state
        .motor
        .position
        .0
        .map(|v| v.0);
    assert!(
        (position[0] + 220.0).hypot(position[2] - 48.0) < 5.0,
        "delivery must use a door, never a remote spawn"
    );
}

// Reject repairs with wanted heat or insufficient funds and cancel interrupted dwell without charging.
#[test]
fn repair_rejects_heat_and_funds_and_cancels_interrupted_dwell_without_debit() {
    let (mut w, mut a, mut t, p, _) = repair_setup();
    let id = w.sessions.peers[&p].player_id();
    w.entities
        .get_mut_by_id::<WantedComponent>(&(id))
        .expect("admitted wanted state")
        .heat = 1;
    w.entities
        .get_mut_by_id::<WantedComponent>(&(id))
        .expect("admitted wanted state")
        .pursuit = Pursuit::Pursuing;
    repair(&w, &mut t, p, 2);
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::ActionResult(r) if r.request_id.0==2 && r.code==ActionCode::PursuitActive)));
    w.entities
        .get_mut_by_id::<WantedComponent>(&(id))
        .expect("admitted wanted state")
        .heat = 0;
    w.entities
        .get_mut_by_id::<WantedComponent>(&(id))
        .expect("admitted wanted state")
        .pursuit = Pursuit::Clear;
    w.sessions
        .peers
        .get_mut(&p)
        .unwrap()
        .profile
        .as_mut()
        .unwrap()
        .credits = Credits(0);
    repair(&w, &mut t, p, 3);
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::ActionResult(r) if r.request_id.0==3 && r.code==ActionCode::InsufficientFunds)));
    w.sessions
        .peers
        .get_mut(&p)
        .unwrap()
        .profile
        .as_mut()
        .unwrap()
        .credits = Credits(150);
    repair(&w, &mut t, p, 4);
    step(&mut w, &mut a, &mut t);
    ticks(&mut w, &mut a, &mut t, p, 25);
    car_at(&mut w, 1, [220.0, 0.0, -160.0]);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.sessions.peers[&p].repair_view.as_ref().unwrap().phase,
        RepairPhase::Cancelled
    );
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
        150
    );
    assert_eq!(w.entities.view::<VehicleComponent>()[&1].state.health.0, 50);
}

// Release ownership and the reserved slot when cancelling a driven recovery, without rewarding the player.
#[test]
fn cancelling_a_driven_recovery_releases_ownership_and_slot_without_reward() {
    let (mut w, mut a, mut t) = police_setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    job(&w, &mut t, p, 1, "recovery_docks_quay");
    step(&mut w, &mut a, &mut t);
    let car = w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&p].player_id()]
        .as_ref()
        .unwrap()
        .reserved_vehicle
        .unwrap();
    enter(&mut w, &mut a, &mut t, p, car, 2);
    let epoch = w.sessions.peers[&p].epoch;
    vehicle_action(&w, &mut t, p, 3, epoch, Action::CancelMission(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert!(!w.entities.view::<VehicleComponent>().contains_key(&car));
    assert_eq!(w.sessions.peers[&p].epoch, epoch + 1);
    assert_eq!(w.sessions.peers[&p].profile.as_ref().unwrap().credits.0, 0);
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player.unwrap().0]
            .state
            .mode,
        CharacterMode::OnFoot
    );
}

#[test]
fn authored_courier_variants_select_private_targets_and_commit_their_own_rewards() {
    // Exercise every authored option deterministically, through real acceptance and payout.
    for index in 0..3 {
        let (mut w, mut profiles, mut transport) = police_setup();
        let template = w
            .content
            .manifest
            .mission_templates
            .iter_mut()
            .find(|t| t.id == "courier_quay_market")
            .unwrap();
        let variant = template.variants[index].clone();
        template.variants = vec![variant.clone()];
        let p = peer(0);
        join(&mut w, &mut profiles, &mut transport, p, None);
        ready(&mut w, &mut profiles, &mut transport, p);
        place(&mut w, p, [-172.0, 0.02, 180.0]);
        let accepted_at = w.tick;
        job(&w, &mut transport, p, 1, "courier_quay_market");
        step(&mut w, &mut profiles, &mut transport);
        let mission = w
            .entities
            .values::<server::metadata::PlayerMission>()
            .get(&w.sessions.peers[&p].player_id())
            .unwrap()
            .as_ref()
            .unwrap();
        assert_eq!(mission.variant_index, Some(0));
        assert_eq!(mission.view().variant_index, Some(0));
        assert_eq!(mission.reward, variant.reward_credits);
        assert_eq!(mission.deadline, accepted_at + variant.duration_ticks);
        let target = mission.target;
        let instance = mission.id.clone();
        let content::MissionObjective::Courier {
            destination_zone_id,
            ..
        } = &variant.objective
        else {
            panic!("courier")
        };
        let zone = w
            .content
            .manifest
            .zones
            .iter()
            .find(|z| &z.id == destination_zone_id)
            .unwrap();
        assert_eq!(target, zone.center.map(|v| v as f32));
        // Retried requests must not roll a new contract or change its payout.
        job(&w, &mut transport, p, 1, "courier_quay_market");
        step(&mut w, &mut profiles, &mut transport);
        assert_eq!(
            w.entities.values::<server::metadata::PlayerMission>()
                [&w.sessions.peers[&p].player_id()]
                .as_ref()
                .unwrap()
                .id,
            instance
        );
        place(&mut w, p, [target[0], 0.02, target[2]]);
        ticks(&mut w, &mut profiles, &mut transport, p, 31);
        assert_eq!(
            w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
            variant.reward_credits
        );
        ticks(&mut w, &mut profiles, &mut transport, p, 2);
        assert_eq!(
            w.sessions.peers[&p].profile.as_ref().unwrap().credits.0,
            variant.reward_credits
        );
    }
}
