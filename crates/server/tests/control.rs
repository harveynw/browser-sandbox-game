use protocol::DisplayName;
use server::{admission::Profiles, content_loader::WorldContent, control::Control, world};
use std::path::Path;
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
    let hello = world::build(&content);
    let mut c = Control::new(hello);
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
fn appearance_requires_guest_cookie_origin_and_valid_rgb_without_touching_other_profiles() {
    let content = WorldContent::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
    )
    .unwrap();
    let mut control = Control::new(world::build(&content));
    let (old, cookie) = control.profiles.session(None).unwrap();
    let cookie = cookie.unwrap();
    let (other, other_cookie) = control.profiles.session(None).unwrap();
    let request = br#"{"tshirt_colour":2272358}"#;
    assert_eq!(
        control
            .route("POST", "/api/appearance", None, Some(&cookie), request, 0)
            .code,
        403
    );
    assert_eq!(
        control
            .route(
                "POST",
                "/api/appearance",
                Some("http://localhost:4173"),
                None,
                request,
                0
            )
            .code,
        401
    );
    for invalid in [
        br#"{"tshirt_colour":-1}"#.as_slice(),
        br#"{"tshirt_colour":16777216}"#,
        br#"{"tshirt_colour":1,"profile_id":"other"}"#,
    ] {
        assert_eq!(
            control
                .route(
                    "POST",
                    "/api/appearance",
                    Some("http://localhost:4173"),
                    Some(&cookie),
                    invalid,
                    0
                )
                .code,
            400
        );
    }
    let reply = control.route(
        "POST",
        "/api/appearance",
        Some("http://localhost:4173"),
        Some(&cookie),
        request,
        0,
    );
    assert_eq!(reply.code, 200);
    assert_eq!(reply.body["profile"]["tshirt_colour"], 2272358);
    let updated = control.profiles.profile(Some(&cookie)).unwrap();
    assert_eq!(updated.display_name, old.display_name);
    assert_eq!(
        control.profiles.profile(other_cookie.as_deref()).unwrap(),
        other
    );
}
