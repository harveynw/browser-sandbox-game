use super::*;
use std::{
    thread,
    time::{Duration, Instant},
};
struct Database(std::path::PathBuf);
impl Database {
    fn new() -> Self {
        let dir = std::env::temp_dir()
            .join(server::admission::random_id("pm-world-persistence-", 16).unwrap());
        std::fs::create_dir(&dir).unwrap();
        Self(dir.join("profiles.sqlite"))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}
fn pump(w: &mut World, a: &mut Profiles, t: &mut FakeTransport, p: PeerKey) {
    if w.sessions
        .get(&p)
        .is_some_and(|s| s.phase == SessionPhase::Playing)
    {
        keep_alive(w, t, p);
    }
    step(w, a, t);
    thread::sleep(Duration::from_millis(1));
}
fn persistent_join(
    w: &mut World,
    a: &mut Profiles,
    t: &mut FakeTransport,
    p: PeerKey,
    cookie: Option<&str>,
) -> String {
    let (mut profile, secret) = a.prepare_session(cookie).unwrap();
    let cookie = secret.or_else(|| cookie.map(str::to_owned)).unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    while profile.is_none() {
        assert!(Instant::now() < until);
        pump(w, a, t, p);
        profile = a.prepare_session(Some(&cookie)).unwrap().0;
    }
    let ticket = a.issue(Some(&cookie), u64::from(w.tick) * 34).unwrap();
    t.events.push_back(TransportEvent::Opened(p));
    step(w, a, t);
    let build = w.build();
    send(
        w,
        t,
        p,
        ClientMessage::ClientHello(ClientHello {
            protocol_version: ProtocolVersion(3),
            build_id: build.build_id,
            content_hash: build.content_hash,
            sim_config_hash: build.sim_config_hash,
            join_ticket: OpaqueId(ticket),
            display_name: DisplayName("Durable Guest".into()),
        }),
    );
    step(w, a, t);
    assert!(
        w.sessions[&p].player.is_none(),
        "no player allocation before worker result"
    );
    while w.sessions[&p].phase == SessionPhase::AwaitHello {
        assert!(Instant::now() < until);
        assert!(w.sessions[&p].player.is_none());
        pump(w, a, t, p);
    }
    ready(w, a, t, p);
    cookie
}
#[test]
fn sqlite_world_rewards_publish_after_commit_and_rejoin_after_fresh_world_restores_profile_only() {
    let db = Database::new();
    let mut a = Profiles::sqlite(&db.0).unwrap();
    let mut w = police_setup().0;
    let mut t = FakeTransport::default();
    let p = peer(0);
    let cookie = persistent_join(&mut w, &mut a, &mut t, p, None);
    let original = w.id.clone();
    place(&mut w, p, [-172.0, 0.02, 180.0]);
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        w.sessions[&p].epoch,
        Action::AcceptMission(AcceptMission {
            template_id: TemplateId("courier_quay_market".into()),
        }),
    );
    step(&mut w, &mut a, &mut t);
    let instance = w.sessions[&p].mission.as_ref().unwrap().id.clone();
    let target = w.sessions[&p].mission.as_ref().unwrap().target;
    let c = rusqlite::Connection::open(&db.0).unwrap();
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    place(&mut w, p, [target[0], 0.02, target[2]]);
    for _ in 0..35 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.sessions[&p].mission.as_ref().unwrap().phase,
        server::missions::Phase::RewardPending
    );
    assert_eq!(w.sessions[&p].profile.as_ref().unwrap().credits.0, 0);
    assert!(
        !t.sent
            .iter()
            .any(|(_, _, m)| matches!(&m.message,ServerMessage::PlayerStatus(s) if s.credits.0>0))
    );
    c.execute_batch("ROLLBACK").unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    while w.sessions[&p].profile.as_ref().unwrap().credits.0 == 0 {
        assert!(Instant::now() < until);
        pump(&mut w, &mut a, &mut t, p);
    }
    assert_eq!(
        w.sessions[&p].mission.as_ref().unwrap().phase,
        server::missions::Phase::Completed
    );
    let profile = w.sessions[&p]
        .profile
        .as_ref()
        .unwrap()
        .profile_id
        .0
        .clone();
    assert!(w.shutdown(&mut a));
    drop(a);
    drop(w);
    let mut a = Profiles::sqlite(&db.0).unwrap();
    let mut w = police_setup().0;
    let mut t = FakeTransport::default();
    persistent_join(&mut w, &mut a, &mut t, p, Some(&cookie));
    assert_ne!(w.id, original);
    assert_eq!(w.sessions[&p].profile.as_ref().unwrap().credits.0, 150);
    assert_eq!(w.sessions[&p].profile.as_ref().unwrap().completed_jobs, 1);
    assert!(w.sessions[&p].mission.is_none());
    assert_eq!(w.sessions[&p].wanted.heat, 0);
    assert_eq!(
        w.players[&w.sessions[&p].player.unwrap().0].state.mode,
        CharacterMode::OnFoot
    );
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(p) = a.apply_mission_reward(&profile, &instance, 150) {
            assert_eq!(p.credits.0, 150);
            assert_eq!(p.completed_jobs, 1);
            break;
        }
        assert!(Instant::now() < until);
        pump(&mut w, &mut a, &mut t, p);
    }
}
#[test]
fn sqlite_repair_disconnect_after_submission_refunds_once_without_healing_car() {
    let db = Database::new();
    let mut a = Profiles::sqlite(&db.0).unwrap();
    let mut w = police_setup().0;
    let mut t = FakeTransport::default();
    let p = peer(0);
    let cookie = persistent_join(&mut w, &mut a, &mut t, p, None);
    let profile = w.sessions[&p]
        .profile
        .as_ref()
        .unwrap()
        .profile_id
        .0
        .clone();
    let until = Instant::now() + Duration::from_secs(3);
    let funded = loop {
        if let Ok(p) = a.apply_mission_reward(&profile, "repair-fixture", 150) {
            break p;
        }
        assert!(Instant::now() < until);
        pump(&mut w, &mut a, &mut t, p);
    };
    w.sessions.get_mut(&p).unwrap().profile = Some(funded);
    at_door(&mut w, p, 1);
    step(&mut w, &mut a, &mut t);
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        w.sessions[&p].epoch,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(1),
        }),
    );
    step(&mut w, &mut a, &mut t);
    w.vehicles.get_mut(&1).unwrap().state.motor =
        sim_core::spawn_vehicle([220.0, 0.0, -180.0], 0.0).unwrap();
    w.vehicles.get_mut(&1).unwrap().state.health = VehicleHealth(50);
    let c = rusqlite::Connection::open(&db.0).unwrap();
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    vehicle_action(
        &w,
        &mut t,
        p,
        2,
        w.sessions[&p].epoch,
        Action::Repair(Repair {
            zone_id: TemplateId("repair_ironworks".into()),
        }),
    );
    for _ in 0..60 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(
        w.sessions[&p].repair_view.as_ref().unwrap().phase,
        RepairPhase::DebitPending
    );
    w.disconnect(p, &mut a);
    c.execute_batch("ROLLBACK").unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        pump(&mut w, &mut a, &mut t, p);
        let count: u32 = c
            .query_row(
                "SELECT COUNT(*) FROM economic_events WHERE kind='refund'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        if count == 1 {
            break;
        }
        assert!(Instant::now() < until);
    }
    for _ in 0..5 {
        pump(&mut w, &mut a, &mut t, p);
    }
    assert_eq!(w.vehicles[&1].state.health.0, 50);
    persistent_join(&mut w, &mut a, &mut t, p, Some(&cookie));
    assert_eq!(w.sessions[&p].profile.as_ref().unwrap().credits.0, 150);
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM economic_events WHERE kind='refund'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn sqlite_repair_applies_after_commit_and_acknowledged_payment_survives_restart() {
    let db = Database::new();
    let mut a = Profiles::sqlite(&db.0).unwrap();
    let mut w = police_setup().0;
    let mut t = FakeTransport::default();
    let p = peer(0);
    let cookie = persistent_join(&mut w, &mut a, &mut t, p, None);
    let profile = w.sessions[&p]
        .profile
        .as_ref()
        .unwrap()
        .profile_id
        .0
        .clone();
    let until = Instant::now() + Duration::from_secs(3);
    let funded = loop {
        if let Ok(p) = a.apply_mission_reward(&profile, "repair-fixture", 150) {
            break p;
        }
        assert!(Instant::now() < until);
        pump(&mut w, &mut a, &mut t, p);
    };
    w.sessions.get_mut(&p).unwrap().profile = Some(funded);
    at_door(&mut w, p, 1);
    step(&mut w, &mut a, &mut t);
    vehicle_action(
        &w,
        &mut t,
        p,
        1,
        w.sessions[&p].epoch,
        Action::EnterVehicle(EnterVehicle {
            vehicle_id: EntityId(1),
        }),
    );
    step(&mut w, &mut a, &mut t);
    w.vehicles.get_mut(&1).unwrap().state.motor =
        sim_core::spawn_vehicle([220.0, 0.0, -180.0], 0.0).unwrap();
    w.vehicles.get_mut(&1).unwrap().state.health = VehicleHealth(50);
    vehicle_action(
        &w,
        &mut t,
        p,
        2,
        w.sessions[&p].epoch,
        Action::Repair(Repair {
            zone_id: TemplateId("repair_ironworks".into()),
        }),
    );
    for _ in 0..60 {
        keep_alive(&w, &mut t, p);
        step(&mut w, &mut a, &mut t);
    }
    assert_eq!(w.vehicles[&1].state.health.0, 50);
    assert_eq!(w.sessions[&p].profile.as_ref().unwrap().credits.0, 150);
    let until = Instant::now() + Duration::from_secs(3);
    while w.vehicles[&1].state.health.0 < 100 {
        assert!(Instant::now() < until);
        pump(&mut w, &mut a, &mut t, p);
    }
    assert_eq!(w.sessions[&p].profile.as_ref().unwrap().credits.0, 100);
    assert_eq!(
        w.sessions[&p].repair_view.as_ref().unwrap().phase,
        RepairPhase::Completed
    );
    assert!(w.shutdown(&mut a));
    drop(a);
    let mut a = Profiles::sqlite(&db.0).unwrap();
    let mut w = police_setup().0;
    let mut t = FakeTransport::default();
    persistent_join(&mut w, &mut a, &mut t, p, Some(&cookie));
    assert_eq!(w.sessions[&p].profile.as_ref().unwrap().credits.0, 100);
    let c = rusqlite::Connection::open(&db.0).unwrap();
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM repair_pending", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM economic_events WHERE kind='refund'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
}
