//! Restored gameplay scenarios against concrete system owners.

use super::*;

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
    assert!(w.systems.missions.active[&w.sessions.peers[&p].player_id()].is_none());
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ActionResult(v) if v.code==ActionCode::OutOfRange)));
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    courier_action(&w, &mut t, p, 2);
    step(&mut w, &mut a, &mut t);
    let instance = w.systems.missions.active[&w.sessions.peers[&p].player_id()]
        .as_ref()
        .unwrap()
        .id
        .clone();
    courier_action(&w, &mut t, p, 2);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.systems.missions.active[&w.sessions.peers[&p].player_id()]
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
        w.systems.missions.active[&w.sessions.peers[&p].player_id()]
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
    let instance = w.systems.missions.active[&w.sessions.peers[&p].player_id()]
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
        w.systems.missions.active[&w.sessions.peers[&p].player_id()]
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
        w.systems.missions.active[&w.sessions.peers[&p].player_id()]
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
    assert!(w.systems.missions.active[&w.sessions.peers[&p].player_id()].is_none());
    assert_eq!(a.profile(Some(&cookie)).unwrap().completed_jobs, 1);
    w.disconnect(p, &mut a);
    let q = peer(2);
    join(&mut w, &mut a, &mut t, q, Some(&cookie));
    ready(&mut w, &mut a, &mut t, q);
    assert_eq!(
        w.sessions.peers[&q].profile.as_ref().unwrap().credits.0,
        150
    );
    assert!(w.systems.missions.active[&w.sessions.peers[&q].player_id()].is_none());
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
        // Arrange success on the next tick so terminal failures must win.
        w.systems
            .missions
            .mission_mut(w.sessions.peers[&p].player_id())
            .as_mut()
            .unwrap()
            .dwell = 29;
        match cause {
            "cancel" => vehicle_action(&w, &mut t, p, 2, 1, Action::CancelMission(Empty {})),
            "timeout" => {
                w.systems
                    .missions
                    .mission_mut(w.sessions.peers[&p].player_id())
                    .as_mut()
                    .unwrap()
                    .deadline = w.tick + 1
            }
            "death" => {
                let id = w.sessions.peers[&p].player.unwrap().0;
                w.players.get_mut(&id).unwrap().state.health = CharacterHealth(0);
            }
            "disconnect" => {
                w.disconnect(p, &mut a);
            }
            "fast" => {
                batch(&w, &mut t, p, w.tick + 1, 0, 32767, 1);
                let id = w.sessions.peers[&p].player.unwrap().0;
                w.players.get_mut(&id).unwrap().state.motor.velocity =
                    Vec3([Scalar(0.0), Scalar(0.0), Scalar(-4.0)]);
            }
            _ => unreachable!(),
        }
        step(&mut w, &mut a, &mut t);
        assert_eq!(a.profile(Some(&cookie)).unwrap().credits.0, 0, "{cause}");
        if cause == "fast" {
            assert_eq!(
                w.systems.missions.active[&w.sessions.peers[&p].player_id()]
                    .as_ref()
                    .unwrap()
                    .dwell,
                0
            );
        } else if cause != "disconnect" {
            assert_eq!(
                w.systems.missions.active[&w.sessions.peers[&p].player_id()]
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
    let pos = w.vehicles[&car].state.motor.position.0.map(|v| v.0);
    let player = w.sessions.peers[&p].player.unwrap().0;
    w.players.get_mut(&player).unwrap().state.motor =
        sim_core::spawn_character([pos[0], 0.02, pos[2] + 10.0], 0.0).unwrap();
    let initial = w.vehicles[&car].state.health.0;
    fire(&w, &mut t, p, 1, 0.0);
    fire(&w, &mut t, p, 1, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.vehicles[&car].state.health.0, initial - 25);
    assert_eq!(
        w.core.players[&w.sessions.peers[&p].player_id()]
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
        w.core.players[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        11
    );
    assert!(t.sent.iter().any(|(_,_,m)|matches!(&m.message,ServerMessage::ActionResult(r) if r.request_id.0==2 && r.code==ActionCode::Cooldown)));
    vehicle_action(&w, &mut t, p, 3, 1, Action::Reload(Empty {}));
    step(&mut w, &mut a, &mut t);
    let end = w.core.players[&w.sessions.peers[&p].player_id()]
        .weapon
        .reload_end
        .unwrap();
    fire(&w, &mut t, p, 4, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.core.players[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        11
    );
    while w.tick < end - 1 {
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.core.players[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        11
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.core.players[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        12
    );
    let id = w.sessions.peers[&p].player_id();
    w.core.players.get_mut(&id).unwrap().weapon.magazine = 0;
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
        w.core.players[&w.sessions.peers[&p].player_id()]
            .weapon
            .reload_end
            .is_none()
    );
    fire(&w, &mut t, p, 8, 0.0);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.core.players[&w.sessions.peers[&p].player_id()]
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
    w.players.get_mut(&player).unwrap().state.motor =
        sim_core::spawn_character([-172.0, 0.02, 180.0], 0.0).unwrap();
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    assert!(w.systems.missions.active[&w.sessions.peers[&p].player_id()].is_some());
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
    let car = w.vehicles.get_mut(&1).unwrap();
    car.state.health = VehicleHealth(1);
    // Building 6 occupies x [-232,-212], z [80,100]. Sweep into its south face.
    car.state.motor = sim_core::spawn_vehicle([-222.0, 0.0, 102.5], 0.0).unwrap();
    car.state.motor.speed = Scalar(22.0);
    car.state.motor.velocity = Vec3([Scalar(0.0), Scalar(0.0), Scalar(-22.0)]);
    for _ in 0..6 {
        batch(&w, &mut t, p, w.tick + 1, 0, 32767, 2);
        step(&mut w, &mut a, &mut t);
        if w.players[&player].state.mode == CharacterMode::Downed {
            break;
        }
    }
    assert!(w.vehicles[&1].state.disabled);
    assert_eq!(w.vehicles[&1].state.driver_id, None);
    assert_eq!(w.players[&player].state.mode, CharacterMode::Downed);
    assert_eq!(w.players[&player].state.health.0, 0);
    assert_eq!(w.sessions.peers[&p].epoch, 3);
    assert_eq!(
        w.systems.missions.active[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .phase,
        server::systems::missions::Phase::Failed
    );
    let until = w.core.players[&w.sessions.peers[&p].player_id()]
        .lifecycle
        .respawn
        .unwrap();
    let position = w.players[&player].state.motor.position.clone();
    while w.tick < until - 1 {
        batch(&w, &mut t, p, w.tick + 1, 32767, 32767, 3);
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(w.players[&player].state.motor.position, position);
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.players[&player].state.health.0, 100);
    assert_eq!(w.players[&player].state.mode, CharacterMode::OnFoot);
    assert_eq!(w.sessions.peers[&p].epoch, 4);
    assert_eq!(
        w.core.players[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        12
    );
    assert!(
        w.core.players[&w.sessions.peers[&p].player_id()]
            .lifecycle
            .protected_until
            .unwrap()
            > w.tick
    );
    assert!(w.systems.missions.active[&w.sessions.peers[&p].player_id()].is_none());
    let fresh = w.players[&player].state.motor.position.clone();
    batch(&w, &mut t, p, w.tick + 1, 0, 32767, 3);
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.players[&player].state.motor.position.0[0], fresh.0[0]);
    assert_eq!(w.players[&player].state.motor.position.0[2], fresh.0[2]);
    // The disabled vehicle waits 900 ticks, independently of the player's respawn.
    for _ in 0..910 {
        if w.tick.is_multiple_of(60) {
            batch(&w, &mut t, p, w.tick + 1, 0, 0, 4);
        }
        step(&mut w, &mut a, &mut t);
    }
    assert!(!w.vehicles[&1].state.disabled);
    assert_eq!(w.vehicles[&1].state.health.0, 100);
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
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    assert_eq!(
        w.systems.police.wanted[&w.sessions.peers[&q].player_id()].heat,
        0
    );
    assert_eq!(
        w.core.players[&w.sessions.peers[&p].player_id()]
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
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    assert_eq!(
        w.core.players[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        10
    );
    assert!(
        w.systems
            .police
            .units
            .values()
            .any(|u| u.target == Some(id))
    );
    w.disconnect(p, &mut a);
    assert!(
        w.systems
            .police
            .units
            .values()
            .all(|u| u.target != Some(id))
    );
    join(&mut w, &mut a, &mut t, p, Some(&cookie));
    ready(&mut w, &mut a, &mut t, p);
    assert_eq!(
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].heat,
        0
    );
    assert_eq!(
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].pursuit,
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
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    let known = w.systems.police.wanted[&w.sessions.peers[&p].player_id()].last_known;
    let id = w.sessions.peers[&p].player.unwrap().0;
    // Fixture relocation simulates an unseen escape; police must retain the observed point.
    w.players.get_mut(&id).unwrap().state.motor =
        sim_core::spawn_character([340.0, 0.02, -340.0], 0.0).unwrap();
    let last_seen = w.systems.police.wanted[&w.sessions.peers[&p].player_id()].last_seen;
    while w.tick < last_seen + 150 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
        t.sent.clear();
    }
    assert_eq!(
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].pursuit,
        Pursuit::Searching
    );
    assert_eq!(
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].last_known,
        known
    );
    for _ in 0..599 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
        t.sent.clear();
    }
    assert_eq!(
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].heat,
        0
    );
    assert_eq!(
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].pursuit,
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
    w.players.get_mut(&id).unwrap().state.motor =
        sim_core::spawn_character([-172.0, 0.02, 180.0], 0.0).unwrap();
    courier_action(&w, &mut t, p, 1);
    step(&mut w, &mut a, &mut t);
    assert!(w.systems.missions.active[&w.sessions.peers[&p].player_id()].is_some());
    let unit = *w.systems.police.units.keys().next().unwrap();
    let position = w.vehicles[&unit].state.motor.position.0.map(|v| v.0);
    let yaw = w.vehicles[&unit].state.motor.yaw.0;
    let target = sim_core::anchor(&w.vehicles[&unit].state.motor, [0.0, 0.02, -5.0]);
    w.players.get_mut(&id).unwrap().state.motor = sim_core::spawn_character(target, yaw).unwrap();
    fire(&w, &mut t, p, 2, yaw);
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    let credits = w.sessions.peers[&p].profile.as_ref().unwrap().credits;
    let epoch = w.sessions.peers[&p].epoch;
    for _ in 0..180 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
        if w.players[&id].state.mode == CharacterMode::Arrested {
            break;
        }
    }
    assert_eq!(
        w.players[&id].state.mode,
        CharacterMode::Arrested,
        "cop began at {position:?}; now {:?}, target {:?}",
        w.vehicles[&unit].state.motor,
        w.players[&id].state.motor
    );
    assert_eq!(w.sessions.peers[&p].epoch, epoch + 1);
    assert_eq!(
        w.systems.missions.active[&w.sessions.peers[&p].player_id()]
            .as_ref()
            .unwrap()
            .phase,
        server::systems::missions::Phase::Failed
    );
    assert_eq!(
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].heat,
        0
    );
    let arrested = w.players[&id].state.motor.position.clone();
    let due = w.core.players[&w.sessions.peers[&p].player_id()]
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
        assert_eq!(w.players[&id].state.motor.position, arrested);
    }
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.players[&id].state.mode, CharacterMode::OnFoot);
    assert_eq!(w.sessions.peers[&p].epoch, epoch + 2);
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().credits,
        credits
    );
    assert_eq!(
        w.core.players[&w.sessions.peers[&p].player_id()]
            .weapon
            .magazine,
        12
    );
    assert!(w.systems.missions.active[&w.sessions.peers[&p].player_id()].is_none());
}

// Limit police to eight units while rotating assignments fairly among sixteen wanted guests.
#[test]
fn sixteen_wanted_guests_share_at_most_eight_police_with_rotating_fair_assignments() {
    let (mut w, mut a, mut t) = police_setup();
    for n in 0..16 {
        let p = peer(n);
        join(&mut w, &mut a, &mut t, p, None);
        ready(&mut w, &mut a, &mut t, p);
    }
    let ids: BTreeSet<_> = w.players.keys().copied().collect();
    let mut assigned = BTreeSet::new();
    for turn in 1..=16 {
        w.tick = turn * 150 - 1;
        for n in 0..16 {
            let p = peer(n);
            let id = w.sessions.peers[&p].player_id();
            let pos = w.core.players[&id].state.motor.position.0.map(|v| v.0);
            let wanted = w.systems.police.wanted.get_mut(&id).unwrap();
            wanted.heat = 3;
            wanted.pursuit = Pursuit::Pursuing;
            wanted.last_seen = w.core.tick;
            wanted.last_known = pos;
            keep_alive(&w, &mut t, p);
        }
        step(&mut w, &mut a, &mut t);
        t.sent.clear();
        assert!(w.systems.police.units.len() <= 8);
        assigned.extend(w.systems.police.units.values().filter_map(|u| u.target));
    }
    assert_eq!(w.systems.police.units.len(), 8);
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
    let unit = *w.systems.police.units.keys().next().unwrap();
    let position = w.vehicles[&secured].state.motor.position.0.map(|v| v.0);
    w.vehicles.get_mut(&unit).unwrap().state.motor =
        sim_core::spawn_vehicle([position[0], 0.0, position[2] + 8.0], 0.0).unwrap();
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
        w.systems.police.wanted[&w.sessions.peers[&p].player_id()].heat,
        1
    );
    assert_eq!(w.sessions.peers[&p].epoch, 2);
    vehicle_action(&w, &mut t, p, 2, 2, Action::ExitVehicle(Empty {}));
    step(&mut w, &mut a, &mut t);
    let unit = *w.systems.police.units.keys().next().unwrap();
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
    assert!(w.vehicles[&unit].state.driver_id.is_none());
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
    let unit = *w.systems.police.units.keys().next().unwrap();
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
    let pos = w.vehicles[&car].state.motor.position.0.map(|v| v.0);
    w.vehicles.get_mut(&unit).unwrap().state.motor =
        sim_core::spawn_vehicle([pos[0], 0.0, pos[2] + 5.0], 0.0).unwrap();
    let epoch = w.sessions.peers[&p].epoch;
    let wanted = w.systems.police.wanted.get_mut(&id).unwrap();
    wanted.heat = 1;
    wanted.pursuit = Pursuit::Pursuing;
    wanted.last_seen = w.core.tick;
    wanted.last_known = pos;
    for _ in 0..240 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
        if w.players[&id].state.mode == CharacterMode::Arrested {
            break;
        }
    }
    assert_eq!(
        w.players[&id].state.mode,
        CharacterMode::Arrested,
        "police {:?}, driver {:?}",
        w.vehicles[&unit].state.motor,
        w.players[&id].state.motor
    );
    assert_eq!(w.players[&id].state.vehicle_id, None);
    assert_eq!(w.vehicles[&car].state.driver_id, None);
    assert_eq!(w.sessions.peers[&p].epoch, epoch + 1);
    let stopped = w.vehicles[&car].state.motor.position.clone();
    batch(&w, &mut t, p, w.tick + 1, 0, 32767, epoch);
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.vehicles[&car].state.motor.position, stopped);
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
    let car = *w.vehicles.keys().next().unwrap();
    let id = w.sessions.peers[&p].player.unwrap();
    w.vehicles.get_mut(&car).unwrap().state.driver_id = Some(id);
    let seat = w.vehicles[&car].seat();
    let player = w.core.players.get_mut(&id.0).unwrap();
    player.state.mode = CharacterMode::Driving;
    player.state.vehicle_id = Some(EntityId(car));
    player.state.motor.position = Vec3(seat.map(Scalar));
    vehicle_action(&w, &mut t, p, 1, 1, Action::ExitVehicle(Empty {}));
    step(&mut w, &mut a, &mut t);
    assert_eq!(w.sessions.peers[&p].epoch, 1);
    assert_eq!(w.vehicles[&car].state.driver_id, Some(id));
    assert!(t.sent.iter().any(|(_,_,m)| matches!(&m.message,ServerMessage::ActionResult(v) if v.code==ActionCode::ExitBlocked)));
}
