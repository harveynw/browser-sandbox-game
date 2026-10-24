use protocol::DisplayName;
use server::{
    admission::Profiles,
    content_loader::WorldContent,
    control::Control,
    scheduler::{STEP, Scheduler},
    world::World,
};
use std::{path::Path, time::Duration};
#[test]
fn tickets_are_one_use_expiring_bounded_and_require_the_cookie_secret() {
    let mut a = Profiles::default();
    let (p, cookie) = a.session(None).unwrap();
    let cookie = cookie.unwrap();
    assert_eq!(a.session(Some(&cookie)).unwrap().0, p);
    assert!(a.session(Some(&cookie)).unwrap().1.is_none());
    assert!(a.issue(Some("wrong"), 0).is_err());
    let ticket = a.issue(Some(&cookie), 0).unwrap();
    assert_eq!(ticket.len(), 64);
    assert!(
        a.consume(&ticket, 1, DisplayName("Test One".into()))
            .is_some()
    );
    assert!(
        a.consume(&ticket, 2, DisplayName("Test One".into()))
            .is_none()
    );
    let ticket = a.issue(Some(&cookie), 0).unwrap();
    assert!(
        a.consume(&ticket, 30_000, DisplayName("Test One".into()))
            .is_none()
    );
    let old = a.issue(Some(&cookie), 0).unwrap();
    let new = a.issue(Some(&cookie), 1).unwrap();
    assert!(a.consume(&old, 2, DisplayName("Test One".into())).is_none());
    assert!(a.consume(&new, 2, DisplayName("Test One".into())).is_some());
    a.activate(&p.profile_id.0, "session-new");
    a.deactivate(&p.profile_id.0, "session-old");
    assert!(!a.report_probe(Some(&cookie), "session-old", 10));
    assert!(a.report_probe(Some(&cookie), "session-new", 10));
    assert_eq!(a.probe_until_ms, 5010);
}
#[test]
fn localhost_origin_cookie_json_and_expiring_readiness_gates_are_explicit() {
    let content = WorldContent::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
    )
    .unwrap();
    let world = World::new(content).unwrap();
    let mut c = Control::new(&world);
    assert_eq!(
        c.route("POST", "/api/session", None, None, b"{}", 0).code,
        403
    );
    assert_eq!(
        c.route(
            "POST",
            "/api/session",
            Some("https://evil.example"),
            None,
            b"{}",
            0
        )
        .code,
        403
    );
    assert_eq!(
        c.route(
            "POST",
            "/api/session",
            Some("http://localhost:5173"),
            None,
            b"{\"extra\":1}",
            0
        )
        .code,
        400
    );
    let reply = c.route(
        "POST",
        "/api/session",
        Some("http://localhost:5173"),
        None,
        b"{}",
        0,
    );
    assert_eq!(reply.code, 200);
    let header = reply.cookie.unwrap();
    assert!(header.contains("HttpOnly; SameSite=Strict; Path=/api"));
    let cookie = header
        .split(';')
        .next()
        .unwrap()
        .strip_prefix("pm_guest=")
        .unwrap();
    assert_eq!(
        c.route("GET", "/health/ready", None, None, b"", 0).code,
        503
    );
    assert_eq!(
        c.route(
            "POST",
            "/api/join-ticket",
            Some("http://localhost:5173"),
            None,
            b"{}",
            0
        )
        .code,
        401
    );
    assert_eq!(
        c.route(
            "POST",
            "/api/join-ticket",
            Some("http://localhost:5173"),
            Some(cookie),
            b"{}",
            0
        )
        .code,
        200
    );
    c.status.tick = 1;
    c.status.last_tick_ms = 100;
    c.status.transport_healthy = true;
    assert_eq!(
        c.route("GET", "/health/live", None, None, b"", 100).code,
        200
    );
    assert_eq!(
        c.route("GET", "/health/ready", None, None, b"", 100).code,
        503
    );
    let id = c.profiles.profile(Some(cookie)).unwrap().profile_id.0;
    c.profiles.activate(&id, "active-session");
    assert_eq!(
        c.route(
            "POST",
            "/api/transport-probe",
            Some("http://localhost:5173"),
            Some(cookie),
            br#"{"session_id":"active-session","ws":true,"rtc":false}"#,
            100
        )
        .code,
        403
    );
    assert_eq!(
        c.route(
            "POST",
            "/api/transport-probe",
            Some("http://localhost:5173"),
            Some(cookie),
            br#"{"session_id":"active-session","ws":true,"rtc":true}"#,
            100
        )
        .code,
        200
    );
    assert_eq!(
        c.route("GET", "/health/ready", None, None, b"", 100).code,
        200
    );
    c.status.last_tick_ms = 5101;
    assert_eq!(
        c.route("GET", "/health/ready", None, None, b"", 5101).code,
        503
    );
    c.profiles.probe_until_ms = 20_000;
    c.status.overloaded = true;
    assert_eq!(
        c.route("GET", "/health/ready", None, None, b"", 5101).code,
        503
    );
    assert_eq!(
        c.route(
            "POST",
            "/api/join-ticket",
            Some("http://localhost:5173"),
            Some(cookie),
            b"{}",
            5101
        )
        .code,
        503
    );
}
#[test]
fn scheduler_caps_catchup_drops_wall_time_and_keeps_contiguous_fixed_steps() {
    let mut s = Scheduler::default();
    assert_eq!(s.advance(Duration::ZERO), (0, false));
    assert_eq!(s.advance(STEP), (1, false));
    assert_eq!(s.advance(Duration::from_secs(10)), (5, false));
    assert_eq!(s.advance(Duration::ZERO), (0, false));
    assert_eq!(s.advance(Duration::from_secs(10)), (5, false));
    assert_eq!(s.advance(Duration::from_secs(10)), (5, true));
    assert_eq!(s.advance(STEP), (1, false));
}
