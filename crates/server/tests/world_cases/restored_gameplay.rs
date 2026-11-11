//! Restored gameplay scenarios against concrete system owners.

use super::*;
use server::entities::player::Player as PlayerComponent;
use server::entities::vehicle::Vehicle as VehicleComponent;
use server::systems::police::Officer as OfficerComponent;
use server::systems::police::Unit as PoliceUnitComponent;
use server::systems::police::Wanted as WantedComponent;

// Check private, session-scoped mission acceptance, duplicate request handling and survival across resynchronisation.
#[test]
fn courier_acceptance_is_private_scoped_cached_and_survives_resync() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    let q = peer(2);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    join(&mut w, &mut a, &mut t, q, None);
    ready(&mut w, &mut a, &mut t, q);
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    assert!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .is_none()
    );
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ActionResult(v) if v.code==ActionCode::OutOfRange)));
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    courier_action(&w, &mut t, p, 2);
    step(&mut w, &mut a, &mut t);
    let instance = w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&p].player_id()]
        .as_ref()
        .unwrap()
        .id
        .clone();
    courier_action(&w, &mut t, p, 2);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .id,
        instance
    );
    courier_action(&w, &mut t, p, 3);
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ActionResult(v) if v.code==ActionCode::AlreadyActive)));
    send(
        &w,
        &mut t,
        p,
        ClientMessage::ResyncRequest(ResyncRequest {
            reason: ResyncReason::Stale,
        }),
    );
    step(&mut w, &mut a, &mut t);
    ready(&mut w, &mut a, &mut t, p);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .id,
        instance
    );
    assert!(t.sent.iter().any(|(target,delivery,m)| *target==p && *delivery==Delivery::Reliable && matches!(&m.message,ServerMessage::PlayerStatus(v) if v.revision.0>1 && v.mission.as_ref().is_some_and(|m|m.instance_id.0==instance))));
    assert!(!t.sent.iter().any(|(target,_,m)| *target==q && matches!(&m.message,ServerMessage::PlayerStatus(v) if v.mission.is_some() || v.credits.0!=0)));
}

// Require continuous slow dwell in the delivery zone and award a reward only once across retries and reconnects.
#[test]
fn courier_requires_continuous_slow_dwell_and_awards_once_across_retry_and_rejoin() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    let cookie = join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    let instance = w.entities.values::<server::metadata::PlayerMission>()
        [&w.sessions.peers[&p].player_id()]
        .as_ref()
        .unwrap()
        .id
        .clone();
    place(&mut w, p, [-44.0, 0.02, 52.0]);
    for _ in 0..29 {
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(a.profile(Some(&cookie)).unwrap().credits.0, 0);
    place(&mut w, p, [-44.0, 0.02, 60.0]);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .dwell,
        0
    );
    place(&mut w, p, [-44.0, 0.02, 52.0]);
    for _ in 0..30 {
        step(&mut w, &mut a, &mut t);
    }
    let profile = a.profile(Some(&cookie)).unwrap();
    assert_eq!((profile.credits.0, profile.completed_jobs), (150, 1));
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .phase,
        server::systems::missions::Phase::Completed
    );
    let duplicate = a
        .apply_mission_reward(&profile.profile_id.0, &instance, 150)
        .unwrap();
    assert_eq!(duplicate.credits.0, 150);
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    for _ in 0..151 {
        step(&mut w, &mut a, &mut t);
    }
    assert!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .is_none()
    );
    assert_eq!(a.profile(Some(&cookie)).unwrap().completed_jobs, 1);
    w.disconnect(p, &mut a);
    let q = peer(2);
    join(&mut w, &mut a, &mut t, q, Some(&cookie));
    ready(&mut w, &mut a, &mut t, q);
    assert_eq!(
        w.sessions.peers[&q].profile.as_ref().unwrap().credits.0,
        150
    );
    assert!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&q].player_id()]
            .is_none()
    );
}

// Prevent rewards after cancellation, timeout, death, disconnect or delivery without the required slow dwell.
#[test]
fn courier_cancel_timeout_death_disconnect_and_fast_delivery_do_not_pay() {
    for cause in ["cancel", "timeout", "death", "disconnect", "fast"] {
        let (mut w, mut a, mut t) = setup();
        let p = peer(1);
        let cookie = join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
        place(&mut w, p, [-172.0, 0.02, 180.0]);
        courier_action(&w, &mut t, p, 1);
        step(&mut w, &mut a, &mut t);
        place(&mut w, p, [-44.0, 0.02, 52.0]);
        let mission_player = w.sessions.peers[&p].player_id();
        // Arrange success on the next tick so terminal failures must win.
        w.entities
            .value_mut::<server::metadata::PlayerMission>(&(mission_player))
            .expect("admitted mission state")
            .as_mut()
            .unwrap()
            .dwell = 29;
        match cause {
            "cancel" => vehicle_action(&w, &mut t, p, 2, 1, Action::CancelMission(Empty {})),
            "timeout" => {
                w.entities
                    .value_mut::<server::metadata::PlayerMission>(&(mission_player))
                    .expect("admitted mission state")
                    .as_mut()
                    .unwrap()
                    .deadline = w.tick + 1
            }
            "death" => {
                let id = w.sessions.peers[&p].player.unwrap().0;
                w.entities
                    .get_mut_by_id::<PlayerComponent>(&id)
                    .unwrap()
                    .state
                    .health = CharacterHealth(0);
            }
            "disconnect" => {
                w.disconnect(p, &mut a);
            }
            "fast" => {
                batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
                let id = w.sessions.peers[&p].player.unwrap().0;
                w.entities
                    .get_mut_by_id::<PlayerComponent>(&id)
                    .unwrap()
                    .state
                    .motor
                    .velocity = Vec3([Scalar(0.0), Scalar(0.0), Scalar(-4.0)]);
            }
            _ => unreachable!(),
        }
        step(&mut w, &mut a, &mut t);
        assert_eq!(a.profile(Some(&cookie)).unwrap().credits.0, 0, "{cause}");
        if cause == "fast" {
            assert_eq!(
                w.entities.values::<server::metadata::PlayerMission>()
                    [&w.sessions.peers[&p].player_id()]
                    .as_ref()
                    .unwrap()
                    .dwell,
                0
            );
        } else if cause != "disconnect" {
            assert_eq!(
                w.entities.values::<server::metadata::PlayerMission>()
                    [&w.sessions.peers[&p].player_id()]
                    .as_ref()
                    .unwrap()
                    .phase,
                server::systems::missions::Phase::Failed,
                "{cause}"
            );
        }
    }
}

// Verify authoritative shooting, duplicate request handling, reload delay and reload cancellation on vehicle entry.
#[test]
fn pistol_is_idempotent_authoritative_and_reload_is_delayed_and_canceled_by_driving() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let car = 1;
    let pos = w.entities.view::<VehicleComponent>()[&car]
        .state
        .motor
        .position
        .0
        .map(|v| v.0);
    let player = w.sessions.peers[&p].player.unwrap().0;
    w.entities
        .get_mut_by_id::<PlayerComponent>(&player)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([pos[0], 0.02, pos[2] + 10.0], 0.0).unwrap();
    let initial = w.entities.view::<VehicleComponent>()[&car].state.health.0;
    fire(&w, &mut t, p, 1, 0.0);
    fire(&w, &mut t, p, 1, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<VehicleComponent>()[&car].state.health.0,
        initial - 25
    );
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        11
    );
    assert_eq!(
        t.sent
            .iter()
            .filter(|(_, _, m)| matches!(
                &m.message,
                ServerMessage::WorldEvent(WorldEvent::Shot(_))
            ))
            .count(),
        1
    );
    fire(&w, &mut t, p, 2, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        11
    );
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::ActionResult(r) if r.request_id.0==2 && r.code==ActionCode::Cooldown)));
    vehicle_action(&w, &mut t, p, 3, 1, Action::Reload(Empty {}));
    step(&mut w, &mut a, &mut t);
    let end = w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
        .weapon
        .reload_end
        .unwrap();
    fire(&w, &mut t, p, 4, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        11
    );
    while w.tick < end - 1 {
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        11
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        12
    );
    let id = w.sessions.peers[&p].player_id();
    w.core
        .entities
        .get_mut_by_id::<PlayerComponent>(&id)
        .unwrap()
        .weapon
        .magazine = 0;
    fire(&w, &mut t, p, 5, 0.0);
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::ActionResult(r) if r.request_id.0==5 && r.code==ActionCode::NoAmmo)));
    vehicle_action(&w, &mut t, p, 6, 1, Action::Reload(Empty {}));
    step(&mut w, &mut a, &mut t);
    at_door(&mut w, p, car);
    vehicle_action(
        &w,
        &mut t,
        p,
        7,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(car),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .reload_end
            .is_none()
    );
    fire(&w, &mut t, p, 8, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        0
    );
}

// Cancel the mission after a fatal crash, change the control epoch, respawn safely and retain the profile.
#[test]
fn fatal_crash_cancels_job_changes_epoch_respawns_safely_and_retains_profile() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    // Accept an ordinary courier before driving; downing must beat any completion.
    let player = w.sessions.peers[&p].player.unwrap().0;
    w.entities
        .get_mut_by_id::<PlayerComponent>(&player)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([-172.0, 0.02, 180.0], 0.0).unwrap();
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    assert!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .is_some()
    );
    at_door(&mut w, p, 1);
    vehicle_action(
        &w,
        &mut t,
        p,
        2,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(1),
        }),
    );
    step(&mut w, &mut a, &mut t);
    let car = w.entities.get_mut_by_id::<VehicleComponent>(&1).unwrap();
    car.state.health = VehicleHealth(1);
    // Building 6 occupies x [-232,-212], z [80,100]. Sweep into its south face.
    car.state.motor = sim_core::spawn_vehicle([-222.0, 0.0, 102.5], 0.0).unwrap();
    car.state.motor.speed = Scalar(22.0);
    car.state.motor.velocity = Vec3([Scalar(0.0), Scalar(0.0), Scalar(-22.0)]);
    for _ in 0..6 {
        batch(&w, &mut t, p, w.tick + 1, 0, 32767, 2);
        step(&mut w, &mut a, &mut t);
        if w.entities.view::<PlayerComponent>()[&player].state.mode == CharacterMode::Downed {
            break;
        }
    }
    assert!(w.entities.view::<VehicleComponent>()[&1].state.disabled);
    assert_eq!(
        w.entities.view::<VehicleComponent>()[&1].state.driver_id,
        None
    );
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&player].state.mode,
        CharacterMode::Downed
    );
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&player].state.health.0,
        0
    );
    assert_eq!(w.sessions.peers[&p].epoch, 3);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .phase,
        server::systems::missions::Phase::Failed
    );
    let until = w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
        .lifecycle
        .respawn
        .unwrap();
    let position = w.entities.view::<PlayerComponent>()[&player]
        .state
        .motor
        .position
        .clone();
    while w.tick < until - 1 {
        batch(&w, &mut t, p, w.tick + 1, 32767, 32767, 3);
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&player]
            .state
            .motor
            .position,
        position
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&player].state.health.0,
        100
    );
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&player].state.mode,
        CharacterMode::OnFoot
    );
    assert_eq!(w.sessions.peers[&p].epoch, 4);
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        12
    );
    assert!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .lifecycle
            .protected_until
            .unwrap()
            > w.tick
    );
    assert!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .is_none()
    );
    let fresh = w.entities.view::<PlayerComponent>()[&player]
        .state
        .motor
        .position
        .clone();
    batch(&w, &mut t, p, w.tick + 1, 0, 32767, 3);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&player]
            .state
            .motor
            .position
            .0[0],
        fresh.0[0]
    );
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&player]
            .state
            .motor
            .position
            .0[2],
        fresh.0[2]
    );
    // The disabled vehicle waits 900 ticks, independently of the player's respawn.
    for _ in 0..910 {
        if w.tick.is_multiple_of(60) {
            batch(&w, &mut t, p, w.tick + 1, 0, 0, 4);
        }
        step(&mut w, &mut a, &mut t);
    }
    assert!(!w.entities.view::<VehicleComponent>()[&1].state.disabled);
    assert_eq!(
        w.entities.view::<VehicleComponent>()[&1].state.health.0,
        100
    );
    assert!(
        t.sent.iter().any(|(_, _, m)| matches!(
            &m.message,
            ServerMessage::WorldEvent(WorldEvent::Impact(_))
        ))
    );
}

// Generate private wanted heat once from real shots and clear old police targets when the guest rejoins.
#[test]
fn police_hear_real_shots_once_publish_private_heat_and_clear_targets_on_rejoin() {
    let (mut w, mut a, mut t) = police_setup();
    let p = peer(0);
    let q = peer(1);
    let cookie = join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    join(&mut w, &mut a, &mut t, q, None);
    ready(&mut w, &mut a, &mut t, q);
    let id = w.sessions.peers[&p].player.unwrap().0;
    fire(&w, &mut t, p, 1, 0.0);
    fire(&w, &mut t, p, 1, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&q].player_id()].heat,
        0
    );
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        11
    );
    assert!(t.sent.iter().any(|(target,_,m)| *target==p && matches!(&m.message,ServerMessage::PlayerStatus(s) if s.heat.0==1 && s.pursuit==Pursuit::Pursuing)));
    assert!(!t.sent.iter().any(|(target, _, m)| *target == q
        && matches!(&m.message,ServerMessage::PlayerStatus(s) if s.heat.0>0)));
    for _ in 0..10 {
        step(&mut w, &mut a, &mut t);
    }
    fire(&w, &mut t, p, 2, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        10
    );
    assert!(
        w.entities
            .view::<PoliceUnitComponent>()
            .values()
            .any(|u| u.target == Some(id))
    );
    w.disconnect(p, &mut a);
    assert!(
        w.entities
            .view::<PoliceUnitComponent>()
            .values()
            .all(|u| u.target != Some(id))
    );
    join(&mut w, &mut a, &mut t, p, Some(&cookie));
    ready(&mut w, &mut a, &mut t, p);
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].heat,
        0
    );
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].pursuit,
        Pursuit::Clear
    );
    assert_ne!(w.sessions.peers[&p].player.unwrap().0, id);
}

// Prevent indefinite tracking of unseen players and allow escape when the search timer expires.
#[test]
fn unseen_target_is_not_tracked_and_escapes_after_search_timer() {
    let (mut w, mut a, mut t) = police_setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    fire(&w, &mut t, p, 1, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    let known = w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].last_known;
    let id = w.sessions.peers[&p].player.unwrap().0;
    // Fixture relocation simulates an unseen escape; police must retain the observed point.
    w.entities
        .get_mut_by_id::<PlayerComponent>(&id)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([340.0, 0.02, -340.0], 0.0).unwrap();
    let last_seen =
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].last_seen;
    while w.tick < last_seen + 150 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
        t.sent.clear();
    }
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].pursuit,
        Pursuit::Searching
    );
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].last_known,
        known
    );
    for _ in 0..599 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
        t.sent.clear();
    }
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].heat,
        0
    );
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].pursuit,
        Pursuit::Clear
    );
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::PlayerStatus(s) if s.heat.0==0 && s.pursuit==Pursuit::Clear)));
}

// Cancel the mission on arrest, change the control epoch and respawn without losing money.
#[test]
fn police_arrest_cancels_mission_changes_epoch_and_respawns_without_money_loss() {
    let (mut w, mut a, mut t) = police_setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    // Accept the real courier before arranging the stationary arrest fixture.
    let id = w.sessions.peers[&p].player.unwrap().0;
    w.entities
        .get_mut_by_id::<PlayerComponent>(&id)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([-172.0, 0.02, 180.0], 0.0).unwrap();
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    assert!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .is_some()
    );
    let unit = *w
        .entities
        .view::<PoliceUnitComponent>()
        .keys()
        .next()
        .unwrap();
    let position = w.entities.view::<VehicleComponent>()[&unit]
        .state
        .motor
        .position
        .0
        .map(|v| v.0);
    let yaw = w.entities.view::<VehicleComponent>()[&unit]
        .state
        .motor
        .yaw
        .0;
    let target = sim_core::anchor(
        &w.entities.view::<VehicleComponent>()[&unit].state.motor,
        [0.0, 0.02, -5.0],
    );
    w.entities
        .get_mut_by_id::<PlayerComponent>(&id)
        .unwrap()
        .state
        .motor = sim_core::spawn_character(target, yaw).unwrap();
    fire(&w, &mut t, p, 2, yaw);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    let credits = w.sessions.peers[&p].profile.as_ref().unwrap().credits;
    let epoch = w.sessions.peers[&p].epoch;
    for _ in 0..180 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
        if w.entities.view::<PlayerComponent>()[&id].state.mode == CharacterMode::Arrested {
            break;
        }
    }
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&id].state.mode,
        CharacterMode::Arrested,
        "cop began at {position:?}; now {:?}, target {:?}",
        w.entities.view::<VehicleComponent>()[&unit].state.motor,
        w.entities.view::<PlayerComponent>()[&id].state.motor
    );
    assert_eq!(w.sessions.peers[&p].epoch, epoch + 1);
    assert_eq!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .phase,
        server::systems::missions::Phase::Failed
    );
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].heat,
        0
    );
    let arrested = w.entities.view::<PlayerComponent>()[&id]
        .state
        .motor
        .position
        .clone();
    let due = w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
        .lifecycle
        .respawn
        .unwrap();
    while w.tick < due - 1 {
        batch(
            &w,
            &mut t,
            p,
            w.tick + 1,
            32767,
            32767,
            w.sessions.peers[&p].epoch,
        );
        step(&mut w, &mut a, &mut t);
        t.sent.clear();
        assert_eq!(
            w.entities.view::<PlayerComponent>()[&id]
                .state
                .motor
                .position,
            arrested
        );
    }
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&id].state.mode,
        CharacterMode::OnFoot
    );
    assert_eq!(w.sessions.peers[&p].epoch, epoch + 2);
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits,
        credits
    );
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        12
    );
    assert!(
        w.entities.values::<server::metadata::PlayerMission>()[&w.sessions.peers[&p].player_id()]
            .is_none()
    );
}

// Limit police to eight units while rotating assignments fairly among sixty_four wanted guests.
#[test]
fn configured_capacity_wanted_guests_share_at_most_eight_police_with_rotating_fair_assignments() {
    let (mut w, mut a, mut t) = police_setup();
    for n in 0..crate::world::MAX_PLAYERS as u32 {
        let p = peer(n);
        join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
    }
    let ids: BTreeSet<_> = w
        .entities
        .view::<PlayerComponent>()
        .keys()
        .copied()
        .collect();
    let mut assigned = BTreeSet::new();
    for turn in 1..=crate::world::MAX_PLAYERS as u32 {
        w.tick = turn * 150 - 1;
        for n in 0..crate::world::MAX_PLAYERS as u32 {
            let p = peer(n);
            let id = w.sessions.peers[&p].player_id();
            let pos = w.core.entities.view::<PlayerComponent>()[&id]
                .state
                .motor
                .position
                .0
                .map(|v| v.0);
            let tick = w.core.tick;
            let wanted = w.entities.get_mut_by_id::<WantedComponent>(&id).unwrap();
            wanted.heat = 3;
            wanted.pursuit = Pursuit::Pursuing;
            wanted.last_seen = tick;
            wanted.last_known = pos;
            keep_alive(&w, &mut t, p);
        }
        step(&mut w, &mut a, &mut t);
        t.sent.clear();
        assert!(w.entities.view::<PoliceUnitComponent>().len() <= 8);
        assert!(w.entities.view::<OfficerComponent>().len() <= 16);
        assigned.extend(
            w.entities
                .view::<PoliceUnitComponent>()
                .values()
                .filter_map(|u| u.target),
        );
    }
    assert_eq!(w.entities.view::<PoliceUnitComponent>().len(), 8);
    assert_eq!(assigned, ids);
}

// Generate heat once for witnessed secured-vehicle theft and prevent possession of police vehicles.
#[test]
fn secured_theft_is_witnessed_once_and_police_cars_cannot_be_possessed() {
    let (mut w, mut a, mut t) = police_setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::EntityShow(s) if s.descriptor.id.0==2 && s.descriptor.display_name.as_ref().is_some_and(|n|n.0.starts_with("Secured ")))));
    let secured = 2;
    let unit = *w
        .entities
        .view::<PoliceUnitComponent>()
        .keys()
        .next()
        .unwrap();
    let position = w.entities.view::<VehicleComponent>()[&secured]
        .state
        .motor
        .position
        .0
        .map(|v| v.0);
    w.entities
        .get_mut_by_id::<VehicleComponent>(&unit)
        .unwrap()
        .state
        .motor = sim_core::spawn_vehicle([position[0], 0.0, position[2] + 8.0], 0.0).unwrap();
    at_door(&mut w, p, secured);
    step(&mut w, &mut a, &mut t);
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(secured),
        }),
    );
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(secured),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<WantedComponent>()[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    assert_eq!(w.sessions.peers[&p].epoch, 2);
    vehicle_action(&w, &mut t, p, 2, 2, Action::ExitVehicle(Empty {}));
    step(&mut w, &mut a, &mut t);
    let unit = *w
        .entities
        .view::<PoliceUnitComponent>()
        .keys()
        .next()
        .unwrap();
    at_door(&mut w, p, unit);
    step(&mut w, &mut a, &mut t);
    vehicle_action(
        &w,
        &mut t,
        p,
        3,
        w.sessions.peers[&p].epoch,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(unit),
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert!(
        w.entities.view::<VehicleComponent>()[&unit]
            .state
            .driver_id
            .is_none()
    );
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::ActionResult(r) if r.request_id.0==3 && r.code==ActionCode::Occupied)));
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::EntityShow(s) if s.descriptor.entity_kind==EntityKind::Police && matches!(s.state,EntityState::Vehicle(_)))));
}

// Safely eject an arrested stationary driver and reject inputs from the previous control epoch.
#[test]
fn arrest_safely_releases_a_stationary_driver_and_rejects_old_epoch_inputs() {
    let (mut w, mut a, mut t) = police_setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let unit = *w
        .entities
        .view::<PoliceUnitComponent>()
        .keys()
        .next()
        .unwrap();
    let car = 1;
    at_door(&mut w, p, car);
    step(&mut w, &mut a, &mut t);
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        1,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(car),
        }),
    );
    step(&mut w, &mut a, &mut t);
    let id = w.sessions.peers[&p].player.unwrap().0;
    // Put a stationary police fixture behind the public compact on its clear parking aisle.
    let pos = w.entities.view::<VehicleComponent>()[&car]
        .state
        .motor
        .position
        .0
        .map(|v| v.0);
    w.entities
        .get_mut_by_id::<VehicleComponent>(&unit)
        .unwrap()
        .state
        .motor = sim_core::spawn_vehicle([pos[0], 0.0, pos[2] + 5.0], 0.0).unwrap();
    let epoch = w.sessions.peers[&p].epoch;
    let tick = w.core.tick;
    let wanted = w.entities.get_mut_by_id::<WantedComponent>(&id).unwrap();
    wanted.heat = 1;
    wanted.pursuit = Pursuit::Pursuing;
    wanted.last_seen = tick;
    wanted.last_known = pos;
    for _ in 0..240 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
        if w.entities.view::<PlayerComponent>()[&id].state.mode == CharacterMode::Arrested {
            break;
        }
    }
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&id].state.mode,
        CharacterMode::Arrested,
        "police {:?}, driver {:?}",
        w.entities.view::<VehicleComponent>()[&unit].state.motor,
        w.entities.view::<PlayerComponent>()[&id].state.motor
    );
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&id].state.vehicle_id,
        None
    );
    assert_eq!(
        w.entities.view::<VehicleComponent>()[&car].state.driver_id,
        None
    );
    assert_eq!(w.sessions.peers[&p].epoch, epoch + 1);
    let stopped = w.entities.view::<VehicleComponent>()[&car]
        .state
        .motor
        .position
        .clone();
    batch(&w, &mut t, p, w.tick + 1, 0, 32767, epoch);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<VehicleComponent>()[&car]
            .state
            .motor
            .position,
        stopped
    );
    assert!(w.metrics.stale_epochs > 0);
}

// Preserve vehicle occupancy and the control epoch when every exit candidate is blocked.
#[test]
fn all_blocked_exit_candidates_preserve_occupancy_and_epoch() {
    let mut package = content();
    for (i, position) in [
        [-153.5, 1.0, 176.0],
        [-150.5, 1.0, 176.0],
        [-152.0, 1.0, 179.0],
    ]
    .into_iter()
    .enumerate()
    {
        package.collision.colliders.push(content::StaticCollider {
            id: 10000 + i as u32,
            shape: content::BoxShape::Box,
            role: content::ColliderRole::Building,
            center: position,
            half_extents: [0.45, 1.0, 0.4],
            yaw: 0.0,
            flags: 3,
        });
    }
    let mut w = World::new(package).unwrap();
    let mut a = Profiles::default();
    let mut t = FakeTransport::default();
    let p = peer(1);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let car = *w.entities.view::<VehicleComponent>().keys().next().unwrap();
    let id = w.sessions.peers[&p].player.unwrap();
    w.entities
        .get_mut_by_id::<VehicleComponent>(&car)
        .unwrap()
        .state
        .driver_id = Some(id);
    let seat = w.entities.view::<VehicleComponent>()[&car].seat();
    let player = w
        .core
        .entities
        .get_mut_by_id::<PlayerComponent>(&id.0)
        .unwrap();
    player.state.mode = CharacterMode::Driving;
    player.state.vehicle_id = Some(EntityId(car));
    player.state.motor.position = Vec3(seat.map(Scalar));
    vehicle_action(&w, &mut t, p, 1, 1, Action::ExitVehicle(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions.peers[&p].epoch, 1);
    assert_eq!(
        w.entities.view::<VehicleComponent>()[&car].state.driver_id,
        Some(id)
    );
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ActionResult(v) if v.code==ActionCode::ExitBlocked)));
}

#[test]
fn police_officers_deploy_and_fire_at_a_suspect_who_attacked_the_patrol() {
    use crate::systems::police::{CrimeId, CrimeKind};
    let (mut w, mut a, mut t) = police_setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    let player = w.sessions.peers[&p].player_id();
    let patrol = *w
        .entities
        .view::<PoliceUnitComponent>()
        .keys()
        .next()
        .unwrap();
    let motor = &w.entities.view::<VehicleComponent>()[&patrol].state.motor;
    let position = motor.position.0.map(|v| v.0);
    let target = sim_core::anchor(motor, [0.0, 0.02, -9.0]);
    place(&mut w, p, target);
    w.core
        .entities
        .get_mut_by_id::<PlayerComponent>(&player)
        .unwrap()
        .lifecycle
        .protected_until = None;
    {
        let Fixture { core, systems, .. } = w.fixture.as_mut().unwrap();
        systems.police.report_crime(
            player,
            CrimeId::Shot(123),
            CrimeKind::PoliceCar(patrol),
            position,
            core,
        );
    }
    for _ in 0..24 {
        let tick = w.core.tick + 1;
        let mut input = sim_core::neutral_input(tick, Yaw(0.0));
        input.buttons = Buttons(4);
        w.sessions
            .peers
            .get_mut(&p)
            .unwrap()
            .inputs
            .insert(tick, input);
        step(&mut w, &mut a, &mut t);
    }
    assert!(
        w.entities
            .view::<OfficerComponent>()
            .values()
            .any(|officer| !officer.mounted)
    );
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&player].state.health.0,
        100
    );
    // Let the old five-second threat window expire before surrender ends.
    w.core.tick += 151;
    for _ in 0..90 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
        if w.entities.view::<PlayerComponent>()[&player].state.health.0 < 100 {
            break;
        }
    }
    assert!(
        w.entities
            .view::<OfficerComponent>()
            .values()
            .any(|officer| !officer.mounted)
    );
    assert!(
        w.entities.view::<PlayerComponent>()[&player].state.health.0 < 100,
        "target {:?}, officers {:?}, shots {}",
        w.entities.view::<PlayerComponent>()[&player].state,
        w.entities
            .view::<OfficerComponent>()
            .values()
            .map(|o| &o.state)
            .collect::<Vec<_>>(),
        t.sent
            .iter()
            .filter(|(_, _, m)| matches!(m.message, ServerMessage::WorldEvent(WorldEvent::Shot(_))))
            .count()
    );
    assert!(t.sent.iter().any(|(_, _, message)| {
        matches!(&message.message, ServerMessage::WorldEvent(WorldEvent::Shot(shot))
            if w.entities.view::<OfficerComponent>().contains_key(&shot.shooter_id.0))
    }));
}

#[test]
fn native_crash_recovery_retries_blocked_exits_without_resetting_respawn_or_replaying() {
    use crate::{
        ecs::state::{Clock, Content},
        gameplay_messages::{ControlTransitionRequested, CrashEjected},
    };
    use bevy_ecs::prelude::*;
    let (mut w, mut a, mut t) = setup();
    let peer = peer(0);
    join(&mut w, &mut a, &mut t, peer, None);
    ready(&mut w, &mut a, &mut t, peer);
    let id = w.sessions.peers[&peer].player.unwrap();
    let car_id = EntityId(1);
    let car = w
        .entities
        .get_mut_by_id::<VehicleComponent>(&car_id.0)
        .unwrap();
    car.state.driver_id = Some(id);
    car.state.health = VehicleHealth(0);
    std::sync::Arc::make_mut(&mut car.template)
        .exit_offsets
        .clear();
    let player = w.entities.get_mut_by_id::<PlayerComponent>(&id.0).unwrap();
    player.state.mode = CharacterMode::Driving;
    player.state.vehicle_id = Some(car_id);
    let Fixture { core, sessions, .. } = w.fixture.take().unwrap();
    let entity = core.entities.entity(id).unwrap();
    let car_entity = core.entities.entity(car_id).unwrap();
    let mut host = crate::ecs::state::install(core);
    host.insert_resource(sessions);
    let spawns = std::mem::take(&mut host.resource_mut::<Content>().0.manifest.spawns);
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            crate::systems::impact_consequences::find_disabled_occupants,
            crate::systems::impact_consequences::recover_occupants,
            crate::systems::impact_consequences::fail_crash_missions,
        )
            .chain(),
    );
    schedule.run(&mut host);
    let deadline = host
        .get::<PlayerComponent>(entity)
        .unwrap()
        .lifecycle
        .respawn
        .unwrap();
    assert_eq!(
        host.get::<PlayerComponent>(entity)
            .unwrap()
            .state
            .vehicle_id,
        Some(car_id)
    );
    assert!(
        host.resource::<Messages<ControlTransitionRequested>>()
            .is_empty()
    );
    crate::gameplay_messages::clear(&mut host);
    host.resource_mut::<Clock>().0 += 1;
    schedule.run(&mut host);
    assert_eq!(
        host.get::<PlayerComponent>(entity)
            .unwrap()
            .lifecycle
            .respawn,
        Some(deadline)
    );
    assert!(host.resource::<Messages<CrashEjected>>().is_empty());
    host.resource_mut::<Content>().0.manifest.spawns = spawns;
    crate::gameplay_messages::clear(&mut host);
    host.resource_mut::<Clock>().0 += 1;
    schedule.run(&mut host);
    assert_eq!(
        host.get::<PlayerComponent>(entity).unwrap().state.mode,
        CharacterMode::Downed
    );
    assert_eq!(
        host.get::<PlayerComponent>(entity)
            .unwrap()
            .lifecycle
            .respawn,
        Some(deadline)
    );
    assert!(
        host.get::<VehicleComponent>(car_entity)
            .unwrap()
            .state
            .disabled
    );
    assert_eq!(host.resource::<Messages<CrashEjected>>().len(), 1);
    assert_eq!(
        host.resource::<Messages<ControlTransitionRequested>>()
            .len(),
        1
    );
    schedule.run(&mut host);
    assert_eq!(
        host.resource::<Messages<ControlTransitionRequested>>()
            .len(),
        1
    );
}

#[test]
fn native_control_replication_refreshes_both_observers_once_without_legacy_resources() {
    use crate::{
        ecs::TickOutcome, ecs::state::Clock, gameplay_messages::ControlTransitionRequested,
    };
    use bevy_ecs::prelude::*;
    let (mut w, mut a, mut t) = setup();
    let owner = peer(0);
    let observer = peer(1);
    join(&mut w, &mut a, &mut t, owner, None);
    ready(&mut w, &mut a, &mut t, owner);
    join(&mut w, &mut a, &mut t, observer, None);
    ready(&mut w, &mut a, &mut t, observer);
    let id = w.sessions.peers[&owner].player.unwrap();
    let car_id = 1;
    let car = w
        .entities
        .get_mut_by_id::<VehicleComponent>(&car_id)
        .unwrap();
    car.state.health = VehicleHealth(0);
    car.state.disabled = true;
    car.state.driver_id = None;
    let player = w.entities.get_mut_by_id::<PlayerComponent>(&id.0).unwrap();
    player.state.mode = CharacterMode::Downed;
    player.state.vehicle_id = None;
    player.state.health = CharacterHealth(0);
    let Fixture {
        core, mut sessions, ..
    } = w.fixture.take().unwrap();
    let entity = core.entities.entity(id).unwrap();
    let old_epoch = sessions.peers[&owner].epoch;
    let old_observer_epoch = sessions.peers[&observer].epoch;
    for peer in [owner, observer] {
        let session = sessions.peers.get_mut(&peer).unwrap();
        session.visibility_counter = 100;
        session.visible.insert(id.0, 90);
        session.visible.insert(car_id, 91);
    }
    sessions.peers.get_mut(&owner).unwrap().inputs.insert(
        core.tick + 1,
        sim_core::neutral_input(core.tick + 1, Yaw(0.0)),
    );
    sessions.output.clear();
    let mut host = crate::ecs::state::install(core);
    host.insert_resource(sessions);
    host.init_resource::<TickOutcome>();
    let message = ControlTransitionRequested {
        peer: owner,
        player: id,
        player_entity: entity,
        changed: vec![id.0, car_id],
        tick: host.resource::<Clock>().0,
    };
    host.write_message(message.clone());
    let mut schedule = Schedule::default();
    schedule.add_systems(crate::replication::apply_control_transitions);
    schedule.run(&mut host);
    host.resource_mut::<TickOutcome>().finish().unwrap();
    let sessions = host.resource::<Sessions>();
    assert_eq!(sessions.peers[&owner].epoch, old_epoch + 1);
    assert_eq!(sessions.peers[&observer].epoch, old_observer_epoch);
    assert!(sessions.peers[&owner].inputs.is_empty());
    assert_eq!(sessions.output.len(), 5);
    for peer in [owner, observer] {
        assert_eq!(sessions.peers[&peer].visible[&id.0], 101);
        assert_eq!(sessions.peers[&peer].visible[&car_id], 102);
        let shows: Vec<_> = sessions
            .output
            .iter()
            .filter(|(target, _, envelope)| {
                *target == peer && matches!(envelope.message, ServerMessage::EntityShow(_))
            })
            .collect();
        assert_eq!(shows.len(), 2);
        for (_, delivery, envelope) in shows {
            assert_eq!(*delivery, Delivery::Reliable);
            let ServerMessage::EntityShow(show) = &envelope.message else {
                unreachable!()
            };
            match &show.state {
                EntityState::Character(state) => assert_eq!(state.mode, CharacterMode::Downed),
                EntityState::Vehicle(state) => {
                    assert!(state.disabled);
                    assert!(state.driver_id.is_none());
                }
            }
            protocol::decode_server(
                &protocol::encode_server(envelope, *delivery).unwrap(),
                *delivery,
            )
            .unwrap();
        }
    }
    assert!(matches!(&sessions.output.back().unwrap().2.message,
        ServerMessage::ControlChanged(state) if state.control_epoch.0 == old_epoch + 1 && state.controlled_entity_id == id));
    schedule.run(&mut host);
    assert_eq!(host.resource::<Sessions>().output.len(), 5);
    // Stale generational identity cannot refresh an owner's epoch.
    let mut stale = message.clone();
    stale.player_entity = Entity::PLACEHOLDER;
    host.write_message(stale);
    schedule.run(&mut host);
    assert_eq!(
        host.resource::<Sessions>().peers[&owner].epoch,
        old_epoch + 1
    );
    // Counter exhaustion fails before changing epochs, queues, or input state.
    host.resource_mut::<Sessions>()
        .peers
        .get_mut(&observer)
        .unwrap()
        .visibility_counter = u32::MAX;
    host.write_message(message);
    schedule.run(&mut host);
    assert!(host.resource_mut::<TickOutcome>().finish().is_err());
    assert_eq!(
        host.resource::<Sessions>().peers[&owner].epoch,
        old_epoch + 1
    );
    assert_eq!(host.resource::<Sessions>().output.len(), 5);
}

#[test]
fn native_vehicle_restoration_preserves_deadline_retries_and_releases_reservations_once() {
    use crate::{
        ecs::TickOutcome,
        ecs::state::Clock,
        gameplay_messages::VehicleRestored,
        metadata::{DisabledVehicle, VehicleHome},
    };
    use bevy_ecs::prelude::*;
    let (mut w, mut a, mut t) = setup();
    let peer = peer(0);
    join(&mut w, &mut a, &mut t, peer, None);
    ready(&mut w, &mut a, &mut t, peer);
    let player = w.sessions.peers[&peer].player.unwrap();
    let car = w.entities.get_mut_by_id::<VehicleComponent>(&1).unwrap();
    let home = (
        car.state.motor.position.0.map(|v| v.0),
        car.state.motor.yaw.0,
    );
    car.state.health = VehicleHealth(0);
    car.state.disabled = true;
    w.entities.insert_value::<VehicleHome>(1, home);
    w.entities.insert_value::<DisabledVehicle>(1, (10, 0));
    w.entities
        .get_mut_by_id::<PlayerComponent>(&player.0)
        .unwrap()
        .state
        .motor = sim_core::spawn_character(home.0, 0.0).unwrap();
    let Fixture { core, systems, .. } = w.fixture.take().unwrap();
    let entity = core.entities.entity(EntityId(1)).unwrap();
    let player_entity = core.entities.entity(player).unwrap();
    let mut host = crate::ecs::state::install(core);
    host.insert_resource(systems.population);
    host.init_resource::<TickOutcome>();
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            crate::systems::lifecycle::restore_vehicles,
            crate::systems::population::release_restored_vehicles,
        )
            .chain(),
    );
    host.resource_mut::<Clock>().0 = 9;
    assert!(
        host.resource_mut::<crate::systems::population::PopulationSystem>()
            .intersections
            .request((999, 999), 1, 9)
    );
    schedule.run(&mut host);
    assert_eq!(host.get::<DisabledVehicle>(entity).unwrap().0, (10, 0));
    host.resource_mut::<Clock>().0 = 10;
    schedule.run(&mut host);
    assert_eq!(host.get::<DisabledVehicle>(entity).unwrap().0, (40, 1));
    assert!(host.resource::<Messages<VehicleRestored>>().is_empty());
    host.get_mut::<PlayerComponent>(player_entity)
        .unwrap()
        .state
        .motor = sim_core::spawn_character([-180.0, 0.02, 180.0], 0.0).unwrap();
    host.resource_mut::<Clock>().0 = 39;
    schedule.run(&mut host);
    assert!(host.get::<VehicleComponent>(entity).unwrap().state.disabled);
    host.resource_mut::<Clock>().0 = 40;
    schedule.run(&mut host);
    host.resource_mut::<TickOutcome>().finish().unwrap();
    assert!(!host.get::<VehicleComponent>(entity).unwrap().state.disabled);
    assert!(host.get::<DisabledVehicle>(entity).is_none());
    assert_eq!(host.get::<VehicleComponent>(entity).unwrap().idle_ticks, 30);
    assert_eq!(host.resource::<Messages<VehicleRestored>>().len(), 1);
    assert!(
        host.resource_mut::<crate::systems::population::PopulationSystem>()
            .intersections
            .request((999, 999), 2, 40)
    );
    // A later claim by the restored car must not be cleared by replay of the old fact.
    assert!(
        host.resource_mut::<crate::systems::population::PopulationSystem>()
            .intersections
            .request((998, 998), 1, 40)
    );
    schedule.run(&mut host);
    assert!(
        !host
            .resource_mut::<crate::systems::population::PopulationSystem>()
            .intersections
            .request((998, 998), 2, 40)
    );
    assert_eq!(host.resource::<Messages<VehicleRestored>>().len(), 1);
}

#[test]
fn native_respawn_retries_safe_spawn_resets_state_and_expires_protection_exactly() {
    use crate::{
        ecs::TickOutcome,
        ecs::state::{Clock, Content},
        gameplay_messages::PlayerRespawned,
    };
    use bevy_ecs::prelude::*;
    let (mut w, mut a, mut t) = setup();
    let peer = peer(0);
    join(&mut w, &mut a, &mut t, peer, None);
    ready(&mut w, &mut a, &mut t, peer);
    let id = w.sessions.peers[&peer].player.unwrap();
    let epoch = w.sessions.peers[&peer].epoch;
    let player = w.entities.get_mut_by_id::<PlayerComponent>(&id.0).unwrap();
    player.state.health = CharacterHealth(0);
    player.state.mode = CharacterMode::Downed;
    player.lifecycle.respawn = Some(100);
    player.lifecycle.protected_until = None;
    player.weapon.shot_sequence = 17;
    player.weapon.magazine = 0;
    player.weapon.reload_end = Some(200);
    w.entities
        .get_mut_by_id::<crate::systems::police::Wanted>(&id.0)
        .unwrap()
        .heat = 10;
    let Fixture {
        core,
        sessions,
        systems,
        ..
    } = w.fixture.take().unwrap();
    let entity = core.entities.entity(id).unwrap();
    let mut host = crate::ecs::state::install(core);
    host.insert_resource(sessions);
    host.insert_resource(systems.police);
    host.init_resource::<TickOutcome>();
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            crate::systems::lifecycle::players::respawn_players,
            crate::systems::lifecycle::players::reset_respawned_missions,
            crate::systems::police::reset_respawned_players,
            crate::replication::apply_control_transitions,
            crate::replication::retire_control_transitions,
        )
            .chain(),
    );
    host.resource_mut::<Clock>().0 = 99;
    schedule.run(&mut host);
    assert_eq!(
        host.get::<PlayerComponent>(entity).unwrap().state.health.0,
        0
    );
    let spawns = std::mem::take(&mut host.resource_mut::<Content>().0.manifest.spawns);
    host.resource_mut::<Clock>().0 = 100;
    schedule.run(&mut host);
    assert_eq!(
        host.get::<PlayerComponent>(entity)
            .unwrap()
            .lifecycle
            .respawn,
        Some(100)
    );
    assert!(host.resource::<Messages<PlayerRespawned>>().is_empty());
    assert_eq!(host.resource::<Sessions>().peers[&peer].epoch, epoch);
    host.resource_mut::<Content>().0.manifest.spawns = spawns;
    host.resource_mut::<Clock>().0 = 101;
    schedule.run(&mut host);
    host.resource_mut::<TickOutcome>().finish().unwrap();
    let player = host.get::<PlayerComponent>(entity).unwrap();
    assert_eq!(player.state.mode, CharacterMode::OnFoot);
    assert_eq!(player.state.health.0, 100);
    assert_eq!(player.weapon.shot_sequence, 17);
    assert_eq!(player.weapon.reload_end, None);
    assert!(
        player
            .weapon
            .slots
            .iter()
            .all(|slot| slot.owned && slot.magazine > 0 && slot.reserve > 0)
    );
    assert_eq!(player.lifecycle.respawn, None);
    assert_eq!(player.lifecycle.protected_until, Some(191));
    assert_eq!(
        host.get::<crate::systems::police::Wanted>(entity)
            .unwrap()
            .heat,
        0
    );
    assert_eq!(host.resource::<Sessions>().peers[&peer].epoch, epoch + 1);
    assert_eq!(host.resource::<Messages<PlayerRespawned>>().len(), 1);
    schedule.run(&mut host);
    assert_eq!(host.resource::<Sessions>().peers[&peer].epoch, epoch + 1);
    host.resource_mut::<Clock>().0 = 190;
    schedule.run(&mut host);
    assert_eq!(
        host.get::<PlayerComponent>(entity)
            .unwrap()
            .lifecycle
            .protected_until,
        Some(191)
    );
    host.resource_mut::<Sessions>()
        .peers
        .get_mut(&peer)
        .unwrap()
        .status_dirty = false;
    host.resource_mut::<Clock>().0 = 191;
    schedule.run(&mut host);
    assert_eq!(
        host.get::<PlayerComponent>(entity)
            .unwrap()
            .lifecycle
            .protected_until,
        None
    );
    assert!(host.resource::<Sessions>().peers[&peer].status_dirty);
}

#[test]
fn native_death_checkpoint_applies_once_and_rejects_stale_or_living_targets() {
    use crate::{ecs::TickOutcome, ecs::state::Clock, gameplay_messages::PlayerDeathRequested};
    use bevy_ecs::prelude::*;
    let (mut w, mut a, mut t) = setup();
    let peer = peer(0);
    join(&mut w, &mut a, &mut t, peer, None);
    ready(&mut w, &mut a, &mut t, peer);
    let id = w.sessions.peers[&peer].player.unwrap();
    let Fixture {
        core, mut sessions, ..
    } = w.fixture.take().unwrap();
    let entity = core.entities.entity(id).unwrap();
    let epoch = sessions.peers[&peer].epoch;
    sessions.output.clear();
    let mut host = crate::ecs::state::install(core);
    host.insert_resource(sessions);
    host.init_resource::<TickOutcome>();
    let tick = host.resource::<Clock>().0;
    let death = PlayerDeathRequested {
        player: id,
        entity,
        tick,
    };
    let mut schedule = Schedule::default();
    schedule.add_systems(crate::ecs::checkpoints::death_checkpoint());
    host.write_message(death);
    schedule.run(&mut host);
    assert_eq!(host.resource::<Sessions>().peers[&peer].epoch, epoch);
    let mut stale = death;
    stale.entity = Entity::PLACEHOLDER;
    host.write_message(stale);
    let mut player = host.get_mut::<PlayerComponent>(entity).unwrap();
    player.state.health = CharacterHealth(0);
    player.state.motor.velocity = Vec3([Scalar(3.0); 3]);
    player.weapon.reload_end = Some(tick + 30);
    player.lifecycle.protected_until = Some(tick + 90);
    host.write_message(death);
    host.write_message(death);
    schedule.run(&mut host);
    host.resource_mut::<TickOutcome>().finish().unwrap();
    let player = host.get::<PlayerComponent>(entity).unwrap();
    assert_eq!(player.state.mode, CharacterMode::Downed);
    assert_eq!(player.state.motor.velocity, Vec3([Scalar(0.0); 3]));
    assert_eq!(player.weapon.reload_end, None);
    assert_eq!(player.lifecycle.protected_until, None);
    assert_eq!(player.lifecycle.respawn, Some(tick + 150));
    assert_eq!(host.resource::<Sessions>().peers[&peer].epoch, epoch + 1);
    let controls = |host: &bevy_ecs::world::World| {
        host.resource::<Sessions>()
            .output
            .iter()
            .filter(|(_, _, envelope)| matches!(envelope.message, ServerMessage::ControlChanged(_)))
            .count()
    };
    assert_eq!(controls(&host), 1);
    // Independent reader instances at later checkpoints must not replay this batch.
    let mut later_checkpoint = Schedule::default();
    later_checkpoint.add_systems(crate::ecs::checkpoints::death_checkpoint());
    later_checkpoint.run(&mut host);
    assert_eq!(controls(&host), 1);
    assert_eq!(host.resource::<Sessions>().peers[&peer].epoch, epoch + 1);
}

#[test]
fn native_automatic_shots_observe_earlier_damage_and_do_not_replay() {
    use crate::{
        ecs::state::Clock,
        gameplay_messages::{AutomaticShotRequested, ShotEmitted},
    };
    use bevy_ecs::prelude::*;
    let (mut w, mut a, mut t) = setup();
    for n in 0..3 {
        join(&mut w, &mut a, &mut t, peer(n), None);
        ready(&mut w, &mut a, &mut t, peer(n));
    }
    let ids: Vec<_> = (0..3)
        .map(|n| w.sessions.peers[&peer(n)].player.unwrap())
        .collect();
    for (n, id) in ids.iter().enumerate() {
        let p = w.entities.get_mut_by_id::<PlayerComponent>(&id.0).unwrap();
        p.state.motor =
            sim_core::spawn_character([-128.0, 0.02, 120.0 - n as f32 * 5.0], 0.0).unwrap();
        p.lifecycle.protected_until = None;
        if n == 1 {
            p.state.health = CharacterHealth(1);
        }
    }
    let Fixture {
        core,
        sessions,
        systems,
        ..
    } = w.fixture.take().unwrap();
    let mut host = crate::ecs::state::install(core);
    let entities: Vec<_> = ids
        .iter()
        .map(|id| host.resource::<crate::ecs::EntityIndex>().0[&id.0])
        .collect();
    let epoch = sessions.peers[&peer(0)].epoch;
    host.insert_resource(sessions);
    crate::ecs::state::install_systems(&mut host, systems);
    let tick = host.resource::<Clock>().0;
    let request = AutomaticShotRequested {
        peer: peer(0),
        player: ids[0],
        entity: entities[0],
        tick,
        epoch,
        aim: Fire {
            weapon_id: WeaponId::Pistol,
            aim_yaw: Yaw(0.0),
            aim_pitch: Pitch(0.0),
        },
    };
    host.write_message(request.clone());
    // A target killed by the preceding request must not execute its queued shot.
    host.write_message(AutomaticShotRequested {
        peer: peer(1),
        player: ids[1],
        entity: entities[1],
        epoch: host.resource::<Sessions>().peers[&peer(1)].epoch,
        ..request.clone()
    });
    let mut schedule = Schedule::default();
    schedule.add_systems(crate::systems::weapons::execute_automatic);
    schedule.run(&mut host);
    assert_eq!(
        host.get::<PlayerComponent>(entities[1])
            .unwrap()
            .state
            .health
            .0,
        0
    );
    assert_eq!(
        host.get::<PlayerComponent>(entities[1])
            .unwrap()
            .weapon
            .shot_sequence,
        0
    );
    assert_eq!(host.resource::<Messages<ShotEmitted>>().len(), 1);
    // Cooldown is lifted only for this test: the second ray must pass the dead blocker.
    host.get_mut::<PlayerComponent>(entities[0])
        .unwrap()
        .weapon
        .next_fire = tick;
    host.write_message(request);
    schedule.run(&mut host);
    let messages = host.resource::<Messages<ShotEmitted>>();
    let mut cursor = messages.get_cursor();
    let shots: Vec<_> = cursor.read(messages).collect();
    assert_eq!(shots.len(), 2);
    assert_eq!(shots[0].event.hit_entity_id, Some(ids[1]));
    assert_eq!(shots[1].event.hit_entity_id, Some(ids[2]));
    schedule.run(&mut host);
    assert_eq!(host.resource::<Messages<ShotEmitted>>().len(), 2);
}

#[test]
fn native_shot_presentation_delivers_once_and_retires_between_checkpoints() {
    use crate::{ecs::state::Clock, gameplay_messages::ShotEmitted};
    use bevy_ecs::prelude::*;
    let (mut w, mut a, mut t) = setup();
    for n in 0..3 {
        join(&mut w, &mut a, &mut t, peer(n), None);
        ready(&mut w, &mut a, &mut t, peer(n));
    }
    let shooter = w.sessions.peers[&peer(0)].player.unwrap();
    for n in 0..3 {
        let id = w.sessions.peers[&peer(n)].player.unwrap();
        w.entities
            .get_mut_by_id::<PlayerComponent>(&id.0)
            .unwrap()
            .state
            .motor
            .position = Vec3([
            Scalar(if n == 1 { 0.0 } else { 1000.0 }),
            Scalar(0.0),
            Scalar(0.0),
        ]);
    }
    let Fixture {
        core, mut sessions, ..
    } = w.fixture.take().unwrap();
    sessions.output.clear();
    let mut host = crate::ecs::state::install(core);
    host.insert_resource(sessions);
    let tick = host.resource::<Clock>().0;
    let shot = ShotEmitted {
        event: Shot {
            weapon_id: WeaponId::Pistol,
            shot_sequence: 1,
            event_id: EventId(1),
            server_tick: tick,
            shooter_id: shooter,
            origin: Vec3([Scalar(0.0); 3]),
            end: Vec3([Scalar(1.0); 3]),
            hit_entity_id: None,
        },
        position: [0.0; 3],
        source: Some(peer(0)),
        tick,
    };
    host.write_message(shot.clone());
    let mut stale = shot;
    stale.tick = tick.wrapping_sub(1);
    host.write_message(stale);
    let checkpoint = || {
        let mut schedule = Schedule::default();
        schedule.add_systems(
            (
                crate::systems::weapons::present_shots,
                crate::systems::weapons::retire_shots,
            )
                .chain(),
        );
        schedule
    };
    checkpoint().run(&mut host);
    let output = &host.resource::<Sessions>().output;
    assert_eq!(output.len(), 2);
    for n in 0..2 {
        assert_eq!(
            output
                .iter()
                .filter(|(p, delivery, envelope)| *p == peer(n)
                    && *delivery == Delivery::Reliable
                    && matches!(
                        envelope.message,
                        ServerMessage::WorldEvent(WorldEvent::Shot(_))
                    ))
                .count(),
            1
        );
    }
    checkpoint().run(&mut host);
    assert_eq!(host.resource::<Sessions>().output.len(), 2);
}

#[test]
fn admission_weapon_actions_commit_before_later_packets() {
    let (mut w, mut a, mut t) = setup();
    for n in 0..2 {
        join(&mut w, &mut a, &mut t, peer(n), None);
        ready(&mut w, &mut a, &mut t, peer(n));
    }
    let ids: Vec<_> = (0..2)
        .map(|n| w.sessions.peers[&peer(n)].player_id())
        .collect();
    for (n, id) in ids.iter().enumerate() {
        let p = w.entities.get_mut_by_id::<PlayerComponent>(id).unwrap();
        p.state.motor =
            sim_core::spawn_character([-128.0, 0.02, 120.0 - n as f32 * 5.0], 0.0).unwrap();
        p.lifecycle.protected_until = None;
        if n == 1 {
            p.state.health = CharacterHealth(1);
        }
    }
    t.sent.clear();
    // Receive sorting gives peer zero's shot precedence even when enqueued later.
    fire(&w, &mut t, peer(1), 1, std::f32::consts::PI);
    fire(&w, &mut t, peer(0), 1, 0.0);
    fire(&w, &mut t, peer(0), 1, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&ids[0]].state.health.0,
        100
    );
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&ids[0]]
            .weapon
            .magazine,
        11
    );
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&ids[1]].state.health.0,
        0
    );
    assert_eq!(
        w.entities.view::<PlayerComponent>()[&ids[1]]
            .weapon
            .magazine,
        12
    );
    assert!(t.sent.iter().any(|(p, _, m)| *p == peer(1)
        && matches!(&m.message,ServerMessage::ActionResult(r) if r.code==ActionCode::Dead)));
    let results: Vec<_> = t
        .sent
        .iter()
        .filter_map(|(p, _, m)| match &m.message {
            ServerMessage::ActionResult(r) if *p == peer(0) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.code == ActionCode::Ok));
}

#[test]
fn native_weapon_actions_reject_stale_entities_and_epochs_without_spending_ammo() {
    use crate::{ecs::state::Clock, systems::weapons::WeaponActionRequested};
    use bevy_ecs::prelude::*;
    let (mut w, mut a, mut t) = setup();
    join(&mut w, &mut a, &mut t, peer(0), None);
    ready(&mut w, &mut a, &mut t, peer(0));
    let Fixture {
        core,
        mut sessions,
        systems,
        ..
    } = w.fixture.take().unwrap();
    let id = sessions.peers[&peer(0)].player.unwrap();
    let entity = core.entities.entity(id).unwrap();
    let epoch = sessions.peers[&peer(0)].epoch;
    sessions.output.clear();
    let mut host = crate::ecs::state::install(core);
    host.insert_resource(sessions);
    crate::ecs::state::install_systems(&mut host, systems);
    let tick = host.resource::<Clock>().0;
    for (request_id, target, control_epoch) in
        [(1, Entity::PLACEHOLDER, epoch), (2, entity, epoch + 1)]
    {
        host.write_message(WeaponActionRequested {
            peer: peer(0),
            entity: Some(target),
            tick,
            request: ActionRequest {
                request_id: RequestId(request_id),
                control_epoch: ControlEpoch(control_epoch),
                action: Action::Fire(Fire {
                    weapon_id: WeaponId::Pistol,
                    aim_yaw: Yaw(0.0),
                    aim_pitch: Pitch(0.0),
                }),
            },
        });
    }
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            crate::systems::weapons::execute_actions,
            crate::systems::weapons::retire_actions,
        )
            .chain(),
    );
    schedule.run(&mut host);
    let p = host.get::<PlayerComponent>(entity).unwrap();
    assert_eq!(p.weapon.magazine, 12);
    assert_eq!(p.weapon.shot_sequence, 0);
    let codes: Vec<_> = host
        .resource::<Sessions>()
        .output
        .iter()
        .filter_map(|(_, _, m)| match &m.message {
            ServerMessage::ActionResult(r) => Some(r.code),
            _ => None,
        })
        .collect();
    assert_eq!(codes, vec![ActionCode::NotReady, ActionCode::StaleEpoch]);
    // A separate checkpoint must not apply or reply to a retired batch again.
    let mut later = Schedule::default();
    later.add_systems(crate::systems::weapons::execute_actions);
    later.run(&mut host);
    assert_eq!(host.resource::<Sessions>().output.len(), 2);
}

#[test]
fn native_reward_commit_rejects_stale_identity_and_replay() {
    use crate::{
        ecs::state::Clock,
        metadata::PlayerMission,
        systems::missions::{
            Phase,
            native::{RewardCommitted, RewardRequested},
        },
    };
    use bevy_ecs::prelude::*;
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    let Fixture { core, sessions, .. } = w.fixture.take().unwrap();
    let id = sessions.peers[&p].player.unwrap();
    let entity = core.entities.entity(id).unwrap();
    let profile = sessions.peers[&p].profile.clone().unwrap();
    let sid = sessions.peers[&p].id.clone().unwrap();
    let mut host = crate::ecs::state::install(core);
    host.insert_resource(sessions);
    let tick = host.resource::<Clock>().0;
    let mut mission = host.get_mut::<PlayerMission>(entity).unwrap();
    let m = mission.0.as_mut().unwrap();
    m.phase = Phase::RewardPending;
    let request = RewardRequested {
        peer: p,
        player: id,
        entity,
        session: sid,
        profile: profile.profile_id.0.clone(),
        mission: m.id.clone(),
        amount: m.reward,
        tick,
    };
    let mut committed = profile.clone();
    committed.credits = Credits(profile.credits.0 + request.amount);
    committed.completed_jobs += 1;
    let mut stale = request.clone();
    stale.entity = Entity::PLACEHOLDER;
    host.write_message(RewardCommitted {
        request: stale,
        profile: committed.clone(),
    });
    let mut wrong_session = request.clone();
    wrong_session.session.push_str("-old");
    host.write_message(RewardCommitted {
        request: wrong_session,
        profile: committed.clone(),
    });
    let mut schedule = Schedule::default();
    schedule.add_systems(crate::systems::missions::native::apply_rewards);
    schedule.run(&mut host);
    assert_eq!(
        host.get::<PlayerMission>(entity)
            .unwrap()
            .0
            .as_ref()
            .unwrap()
            .phase,
        Phase::RewardPending
    );
    assert_eq!(
        host.resource::<Sessions>().peers[&p]
            .profile
            .as_ref()
            .unwrap()
            .credits,
        profile.credits
    );
    host.write_message(RewardCommitted {
        request: request.clone(),
        profile: committed.clone(),
    });
    schedule.run(&mut host);
    assert_eq!(
        host.get::<PlayerMission>(entity)
            .unwrap()
            .0
            .as_ref()
            .unwrap()
            .phase,
        Phase::Completed
    );
    let mut replay = committed.clone();
    replay.credits = Credits(committed.credits.0 + 100);
    host.write_message(RewardCommitted {
        request,
        profile: replay,
    });
    schedule.run(&mut host);
    assert_eq!(
        host.resource::<Sessions>().peers[&p]
            .profile
            .as_ref()
            .unwrap()
            .credits,
        committed.credits
    );
}

#[test]
fn native_possession_rejects_stale_entities_and_commits_control_once() {
    use crate::{ecs::TickOutcome, ecs::state::Clock, sessions::native::PossessionRequested};
    use bevy_ecs::prelude::*;
    let (mut w, mut a, mut t) = setup();
    let p = peer(0);
    join(&mut w, &mut a, &mut t, p, None);
    ready(&mut w, &mut a, &mut t, p);
    at_door(&mut w, p, 1);
    let Fixture {
        core, mut sessions, ..
    } = w.fixture.take().unwrap();
    let id = sessions.peers[&p].player.unwrap();
    let entity = core.entities.entity(id).unwrap();
    let epoch = sessions.peers[&p].epoch;
    sessions.output.clear();
    let mut host = crate::ecs::state::install(core);
    host.insert_resource(sessions);
    host.init_resource::<TickOutcome>();
    let tick = host.resource::<Clock>().0;
    let request = ActionRequest {
        request_id: RequestId(1),
        control_epoch: ControlEpoch(epoch),
        action: Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(1),
        }),
    };
    host.write_message(PossessionRequested {
        peer: p,
        request: request.clone(),
        entity: Some(Entity::PLACEHOLDER),
        tick,
    });
    let mut valid = request;
    valid.request_id = RequestId(2);
    for _ in 0..2 {
        host.write_message(PossessionRequested {
            peer: p,
            request: valid.clone(),
            entity: Some(entity),
            tick,
        });
    }
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            crate::sessions::native::possess,
            crate::replication::apply_control_transitions,
            crate::replication::retire_control_transitions,
            crate::sessions::native::reply_possession,
            crate::sessions::native::retire_possession,
        )
            .chain(),
    );
    schedule.run(&mut host);
    host.resource_mut::<TickOutcome>().finish().unwrap();
    assert_eq!(
        host.get::<PlayerComponent>(entity)
            .unwrap()
            .state
            .vehicle_id,
        Some(EntityId(1))
    );
    let s = host.resource::<Sessions>();
    assert_eq!(s.peers[&p].epoch, epoch + 1);
    let replies: Vec<_> = s
        .output
        .iter()
        .filter_map(|(_, _, m)| match &m.message {
            ServerMessage::ActionResult(r) => Some(r.code),
            _ => None,
        })
        .collect();
    assert_eq!(
        replies,
        vec![ActionCode::NotReady, ActionCode::Ok, ActionCode::Ok]
    );
    assert_eq!(
        s.output
            .iter()
            .filter(|(_, _, m)| matches!(m.message, ServerMessage::ControlChanged(_)))
            .count(),
        1
    );
    let count = s.output.len();
    schedule.run(&mut host);
    assert_eq!(host.resource::<Sessions>().output.len(), count);
}
