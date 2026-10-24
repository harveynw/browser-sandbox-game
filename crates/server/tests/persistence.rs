use protocol::ProfileView;
use server::{
    admission::Profiles,
    persistence::{Command, SqliteWorker, StoreError},
};
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(server::admission::random_id("pm-t17-", 16).unwrap());
        std::fs::create_dir(&dir).unwrap();
        Self(dir.join("profiles.sqlite"))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}
fn run(w: &SqliteWorker, id: &str, command: Command) -> Result<ProfileView, StoreError> {
    w.submit(id.into(), command).unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(done) = w.poll().into_iter().next() {
            assert_eq!(done.id, id);
            return done.result;
        }
        assert!(Instant::now() < until, "worker result timeout");
        thread::sleep(Duration::from_millis(1));
    }
}
fn load(w: &SqliteWorker, hash: &str) -> ProfileView {
    run(
        w,
        "load",
        Command::Load {
            hash: hash.into(),
            new_id: server::admission::random_id("profile-", 16).unwrap(),
        },
    )
    .unwrap()
}
fn event(
    w: &SqliteWorker,
    p: &ProfileView,
    event: &str,
    delta: i64,
    job: bool,
) -> Result<ProfileView, StoreError> {
    run(
        w,
        "event",
        Command::Economic {
            profile: p.profile_id.0.clone(),
            event: event.into(),
            delta,
            job,
        },
    )
}
#[test]
fn migrations_hash_only_cookie_and_committed_rewards_survive_reopen_once() {
    let db = Database::new();
    let cookie = "a".repeat(64);
    let hash = content::canonical::sha256(cookie.as_bytes());
    let w = SqliteWorker::open(&db.0).unwrap();
    let p = load(&w, &hash);
    let named = run(
        &w,
        "name",
        Command::Name {
            profile: p.profile_id.0.clone(),
            name: "Persistent Guest".into(),
        },
    )
    .unwrap();
    assert_eq!(named.display_name.0, "Persistent Guest");
    assert_eq!(
        event(&w, &p, "mission:once", 150, true).unwrap().credits.0,
        150
    );
    assert_eq!(
        event(&w, &p, "mission:once", 150, true)
            .unwrap()
            .completed_jobs,
        1
    );
    drop(w);
    let w = SqliteWorker::open(&db.0).unwrap();
    let p2 = load(&w, &hash);
    assert_eq!(p2.profile_id, p.profile_id);
    assert_eq!(p2.display_name.0, "Persistent Guest");
    assert_eq!(p2.credits.0, 150);
    assert_eq!(
        event(&w, &p2, "mission:once", 150, true).unwrap().credits.0,
        150
    );
    assert_eq!(
        event(&w, &p2, "mission:once", 151, true),
        Err(StoreError::Conflict)
    );
    let c = rusqlite::Connection::open(&db.0).unwrap();
    let stored: Vec<u8> = c
        .query_row("SELECT cookie_secret_hash FROM profiles", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, hash.as_bytes());
    assert_ne!(stored, cookie.as_bytes());
    assert_eq!(
        c.pragma_query_value::<String, _>(None, "journal_mode", |r| r.get(0))
            .unwrap(),
        "wal"
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
}
#[test]
fn insufficient_funds_and_conflicting_or_unbacked_events_rollback_atomically() {
    let db = Database::new();
    let w = SqliteWorker::open(&db.0).unwrap();
    let p = load(&w, &"b".repeat(64));
    assert_eq!(
        event(&w, &p, "repair:session:1", -50, false),
        Err(StoreError::InsufficientFunds)
    );
    assert_eq!(
        event(&w, &p, "refund:repair:session:1", 50, false),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        event(&w, &p, "mission:fund", 100, true).unwrap().credits.0,
        100
    );
    let other = load(&w, &"c".repeat(64));
    assert_eq!(
        event(&w, &other, "mission:fund", 100, true),
        Err(StoreError::Conflict)
    );
    let c = rusqlite::Connection::open(&db.0).unwrap();
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM economic_events", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM repair_pending", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
}
#[test]
fn interrupted_repairs_refund_once_on_restart_but_acknowledged_repairs_stay_paid() {
    let db = Database::new();
    let w = SqliteWorker::open(&db.0).unwrap();
    let p = load(&w, &"d".repeat(64));
    event(&w, &p, "mission:fund", 200, true).unwrap();
    event(&w, &p, "repair:session:1", -50, false).unwrap();
    assert_eq!(
        event(&w, &p, "refund:repair:session:1", 50, false)
            .unwrap()
            .credits
            .0,
        200
    );
    assert_eq!(
        event(&w, &p, "refund:repair:session:1", 50, false)
            .unwrap()
            .credits
            .0,
        200
    );
    event(&w, &p, "repair:session:2", -50, false).unwrap();
    assert_eq!(
        event(&w, &p, "mission:during-repair", 100, true),
        Err(StoreError::Unavailable)
    );
    run(
        &w,
        "finish",
        Command::FinishRepair {
            event: "repair:session:2".into(),
        },
    )
    .unwrap();
    // Simulates a crash after debit commit but before its result can be applied to a live car.
    event(&w, &p, "repair:session:3", -50, false).unwrap();
    drop(w);
    for _ in 0..2 {
        let w = SqliteWorker::open(&db.0).unwrap();
        let restored = load(&w, &"d".repeat(64));
        assert_eq!(restored.credits.0, 150);
        assert_eq!(restored.completed_jobs, 1);
        assert_eq!(
            event(&w, &p, "refund:repair:session:3", 50, false)
                .unwrap()
                .credits
                .0,
            150
        );
        assert_eq!(
            event(&w, &p, "refund:repair:session:2", 50, false),
            Err(StoreError::Conflict)
        );
    }
}
#[test]
fn schema_mismatch_and_invalid_database_fail_startup_explicitly() {
    let db = Database::new();
    let c = rusqlite::Connection::open(&db.0).unwrap();
    c.pragma_update(None, "user_version", 2).unwrap();
    drop(c);
    assert!(SqliteWorker::open(&db.0).is_err());
    std::fs::write(&db.0, b"not a SQLite database").unwrap();
    assert!(SqliteWorker::open(&db.0).is_err());
}
fn prepare(a: &mut Profiles, cookie: Option<&str>) -> (ProfileView, String) {
    let (mut p, secret) = a.prepare_session(cookie).unwrap();
    let cookie = secret.or_else(|| cookie.map(str::to_owned)).unwrap();
    for tick in 0..500 {
        if let Some(p) = p {
            return (p, cookie);
        }
        thread::sleep(Duration::from_millis(1));
        a.poll_store(tick);
        p = a.prepare_session(Some(&cookie)).unwrap().0;
    }
    panic!("profile load timeout")
}
#[test]
fn facade_does_not_publish_uncommitted_rewards_and_retries_locked_database_with_same_id() {
    let db = Database::new();
    let mut a = Profiles::sqlite(&db.0).unwrap();
    let (p, cookie) = prepare(&mut a, None);
    let c = rusqlite::Connection::open(&db.0).unwrap();
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = Instant::now();
    assert!(
        a.apply_mission_reward(&p.profile_id.0, "locked", 150)
            .is_err()
    );
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "tick must not wait for SQLite lock"
    );
    assert_eq!(a.committed(&p.profile_id.0).unwrap().credits.0, 0);
    for tick in 0..60 {
        a.poll_store(tick);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(a.committed(&p.profile_id.0).unwrap().credits.0, 0);
    c.execute_batch("ROLLBACK").unwrap();
    let mut committed = None;
    for tick in 60..600 {
        a.poll_store(tick);
        if let Ok(p) = a.apply_mission_reward(&p.profile_id.0, "locked", 150) {
            committed = Some(p);
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(committed.unwrap().credits.0, 150);
    assert!(a.shutdown(Duration::from_secs(1)));
    drop(a);
    let mut a = Profiles::sqlite(&db.0).unwrap();
    assert_eq!(prepare(&mut a, Some(&cookie)).0.completed_jobs, 1);
}
#[test]
fn bounded_submission_rejects_excess_work_without_blocking() {
    let db = Database::new();
    let w = SqliteWorker::open(&db.0).unwrap();
    let c = rusqlite::Connection::open(&db.0).unwrap();
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    let start = Instant::now();
    let mut accepted = 0;
    for n in 0..1000 {
        if w.submit(
            format!("load-{n}"),
            Command::Load {
                hash: format!("{n:064x}"),
                new_id: format!("p-{n}"),
            },
        )
        .is_ok()
        {
            accepted += 1;
        }
    }
    assert!(accepted <= 65);
    assert!(start.elapsed() < Duration::from_millis(100));
    c.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn only_one_worker_can_own_a_database_and_recovery_does_not_run_in_a_second_server() {
    let db = Database::new();
    let w = SqliteWorker::open(&db.0).unwrap();
    assert!(SqliteWorker::open(&db.0).is_err());
    let p = load(&w, &"e".repeat(64));
    event(&w, &p, "mission:fund", 100, true).unwrap();
    assert_eq!(
        event(&w, &p, "repair:session:1", -50, false)
            .unwrap()
            .credits
            .0,
        50
    );
    assert!(SqliteWorker::open(&db.0).is_err());
    assert_eq!(load(&w, &"e".repeat(64)).credits.0, 50);
    drop(w);
    assert_eq!(
        load(&SqliteWorker::open(&db.0).unwrap(), &"e".repeat(64))
            .credits
            .0,
        100
    );
}

#[test]
fn durable_http_admission_returns_pending_cookie_then_only_committed_profile() {
    use server::{content_loader::WorldContent, control::Control, world::World};
    let db = Database::new();
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content");
    let world = World::new(WorldContent::load(&root).unwrap()).unwrap();
    let mut c = Control::new(&world);
    c.profiles = Profiles::sqlite(&db.0).unwrap();
    let r = c.route(
        "POST",
        "/api/session",
        Some("http://localhost:4173"),
        None,
        b"{}",
        0,
    );
    assert_eq!(r.code, 202);
    assert!(r.body["profile"].is_null());
    let header = r.cookie.unwrap();
    assert!(header.contains("Max-Age=31536000"));
    let secret = header
        .strip_prefix("pm_guest=")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        c.profiles.poll_store(1);
        let r = c.route(
            "POST",
            "/api/session",
            Some("http://localhost:4173"),
            Some(&secret),
            b"{}",
            1,
        );
        if r.code == 200 {
            assert_eq!(r.body["profile"]["credits"], 0);
            break;
        }
        assert_eq!(r.code, 202);
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn local_config_selects_sqlite_or_disposable_memory_and_rejects_bad_modes_or_paths() {
    use server::config::Config;
    let db = Database::new();
    let config = db.0.parent().unwrap().join("config.toml");
    for invalid in [
        "[persistence]\nmode='sqlite'",
        "[persistence]\nmode='remote'",
        "[persistence]\nmode='sqlite'\npath=''",
        "[persistence]\nmode='memory'\nunknown=true",
    ] {
        std::fs::write(&config, invalid).unwrap();
        assert!(Config::load(&config).is_err());
    }
    std::fs::write(
        &config,
        "[persistence]\nmode='sqlite'\npath='profiles.sqlite'",
    )
    .unwrap();
    assert!(
        Config::load(&config)
            .unwrap()
            .profiles(db.0.parent().unwrap())
            .unwrap()
            .durable()
    );
    std::fs::write(&config, "[persistence]\nmode='memory'").unwrap();
    assert!(
        !Config::load(&config)
            .unwrap()
            .profiles(db.0.parent().unwrap())
            .unwrap()
            .durable()
    );
}
