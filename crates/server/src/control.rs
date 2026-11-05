//! Explicit localhost development issuer; not a public authentication service.
use crate::{admission::Profiles, sessions::Sessions, systems::Systems, world::World};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
pub const HTTP_ADDR: &str = "127.0.0.1:3001";
pub const COOKIE: &str = "pm_guest";
#[derive(Default)]
pub struct Status {
    pub last_tick_ms: u64,
    pub tick: u32,
    pub sessions: usize,
    pub players: usize,
    pub loading: usize,
    pub playing: usize,
    pub overloaded: bool,
    pub transport_healthy: bool,
}
pub struct Control {
    pub public_origin: Option<String>,
    pub metrics: Value,
    tick_work: crate::performance::Histogram,
    populated_tick_work: crate::performance::Histogram,
    scheduler_poll_gap: crate::performance::Histogram,
    pub profiles: Profiles,
    pub status: Status,
    pub build: Value,
}
pub type SharedControl = Arc<Mutex<Control>>;
pub struct Reply {
    pub code: u16,
    pub body: Value,
    pub cookie: Option<String>,
}
impl Reply {
    fn new(code: u16, body: Value) -> Self {
        Self {
            code,
            body,
            cookie: None,
        }
    }
    fn error(code: u16, message: &str) -> Self {
        Self::new(code, json!({"error":message}))
    }
}
// HTTP control requests keep their own JSON body contract.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Appearance {
    tshirt_colour: protocol::TshirtColour,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    session_id: String,
    ws: bool,
    rtc: bool,
}
impl Control {
    pub fn new(world: &World) -> Self {
        let hello = world.build();
        Self {
            public_origin: None,
            metrics: json!({}),
            tick_work: Default::default(),
            populated_tick_work: Default::default(),
            scheduler_poll_gap: Default::default(),
            profiles: Profiles::default(),
            status: Status::default(),
            build: json!({"protocol_version":hello.protocol_version.0,"build_id":hello.build_id.0,"content_hash":hello.content_hash.0,"sim_config_hash":hello.sim_config_hash.0}),
        }
    }
    pub fn update(
        &mut self,
        world: &World,
        sessions: &Sessions,
        now: u64,
        healthy: bool,
        overloaded: bool,
    ) {
        use crate::sessions::SessionPhase;
        self.status = Status {
            last_tick_ms: now,
            tick: world.tick,
            sessions: sessions.peers.len(),
            players: world.players.len(),
            loading: sessions
                .peers
                .values()
                .filter(|s| s.phase == SessionPhase::Loading)
                .count(),
            playing: sessions
                .peers
                .values()
                .filter(|s| s.phase == SessionPhase::Playing)
                .count(),
            overloaded,
            transport_healthy: healthy,
        };
    }
    pub fn finish_tick(&mut self, elapsed: Duration, populated: bool) {
        self.tick_work.observe(elapsed);
        if populated {
            self.populated_tick_work.observe(elapsed);
        }
    }
    pub fn record_performance(
        &mut self,
        world: &World,
        sessions: &Sessions,
        systems: &Systems,
        queues: Value,
        gap: Duration,
        now: u64,
    ) {
        self.scheduler_poll_gap.observe(gap);
        if !world.tick.is_multiple_of(30) {
            return;
        }
        let peers:Vec<_>=sessions.peers.values().map(|s|json!({"player_id":s.player.map(|id|id.0),"phase":format!("{:?}",s.phase),"epoch":s.epoch,"traffic":s.traffic,"input_ring":s.inputs.len()})).collect();
        let parked = systems
            .population
            .pools
            .values()
            .filter(|p| **p == crate::systems::population::VehiclePool::Parked)
            .count();
        self.metrics = json!({"world_id":world.id,"build":self.build,"tick":world.tick,"uptime_ms":now,"tick_work":self.tick_work.report(),"populated_tick_work":self.populated_tick_work.report(),"scheduler_poll_gap":self.scheduler_poll_gap.report(),"systems":world.timings.report(),"world":world.metrics,"population":{"players":world.players.len(),"sessions":sessions.peers.len(),"traffic":systems.population.traffic.len(),"parked":parked,"pedestrians":systems.population.pedestrians.len(),"police":systems.police.units.len(),"vehicles":world.vehicles.len(),"path_jobs":systems.population.metrics.path_jobs,"max_path_jobs_per_tick":systems.population.metrics.max_path_jobs_per_tick,"blocked_spawns":systems.population.metrics.blocked_spawns},"police":systems.police.diagnostics(),"queues":queues,"peers":peers});
    }
    pub fn route(
        &mut self,
        method: &str,
        path: &str,
        origin: Option<&str>,
        cookie: Option<&str>,
        body: &[u8],
        now: u64,
    ) -> Reply {
        if body.len() > 1024 {
            return Reply::error(413, "request body too large");
        }
        if method == "GET" {
            return match path {
                "/api/metrics" => Reply::new(200, self.metrics.clone()),
                "/api/build" => Reply::new(200, self.build.clone()),
                "/health/live" | "/health/ready" => {
                    let alive = self.status.tick > 0
                        && now.saturating_sub(self.status.last_tick_ms) < 1000
                        && self.status.transport_healthy;
                    let ready = alive
                        && !self.status.overloaded
                        && self.profiles.available()
                        && self.profiles.probe_until_ms > now;
                    Reply::new(
                        if if path.ends_with("ready") {
                            ready
                        } else {
                            alive
                        } {
                            200
                        } else {
                            503
                        },
                        json!({"live":alive,"ready":ready,"transport_probe":"trusted localhost browser WS/RTC receipt report; expires after 5s","tick":self.status.tick,"sessions":self.status.sessions,"players":self.status.players,"loading":self.status.loading,"playing":self.status.playing}),
                    )
                }
                _ => Reply::error(404, "not found"),
            };
        }
        if method != "POST" {
            return Reply::error(405, "method not allowed");
        }
        let allowed = if let Some(public) = &self.public_origin {
            origin == Some(public.as_str())
        } else {
            matches!(
                origin,
                Some(
                    "http://localhost:5173"
                        | "http://localhost:4173"
                        | "http://localhost:3001"
                        | "http://127.0.0.1:5173"
                        | "http://127.0.0.1:4173"
                        | "http://127.0.0.1:3001"
                )
            )
        };
        if !allowed {
            return Reply::error(403, "configured Origin required");
        }
        match path {
            "/api/session" => {
                if serde_json::from_slice::<EmptyBody>(body).is_err() {
                    return Reply::error(400, "expected empty JSON object");
                }
                match self.profiles.prepare_session(cookie) {
                    Ok((profile, secret)) => Reply {
                        code: if profile.is_some() { 200 } else { 202 },
                        body: json!({"profile":profile,"pending":profile.is_none()}),
                        cookie: secret.map(|s| {
                            format!(
                                "{COOKIE}={s}; HttpOnly; SameSite=Strict; Path=/api{}{}",
                                if self.public_origin.is_some() {
                                    "; Secure"
                                } else {
                                    ""
                                },
                                if self.profiles.durable() {
                                    "; Max-Age=31536000"
                                } else {
                                    ""
                                }
                            )
                        }),
                    },
                    Err(_) => Reply::error(503, "profile storage temporarily unavailable"),
                }
            }
            "/api/appearance" => {
                let Ok(request) = serde_json::from_slice::<Appearance>(body) else {
                    return Reply::error(400, "expected an RGB T-shirt colour");
                };
                if self.profiles.profile(cookie).is_none() {
                    return Reply::error(401, "guest session required");
                }
                match self
                    .profiles
                    .update_tshirt_colour(cookie, request.tshirt_colour)
                {
                    Ok(profile) => Reply::new(
                        if profile.is_some() { 200 } else { 202 },
                        json!({"pending":profile.is_none(), "profile":profile}),
                    ),
                    Err(_) => Reply::error(503, "appearance storage unavailable"),
                }
            }
            "/api/join-ticket" => {
                if serde_json::from_slice::<EmptyBody>(body).is_err() {
                    return Reply::error(400, "expected empty JSON object");
                }
                if self.status.overloaded {
                    return Reply::error(503, "server overloaded");
                }
                if self.profiles.profile(cookie).is_none() {
                    return Reply::error(401, "guest session required");
                }
                match self.profiles.issue(cookie, now) {
                    Ok(ticket) => Reply::new(
                        200,
                        json!({"join_ticket":ticket,"expires_in_ms":30000,"ws_url":self.public_origin.as_ref().map(|origin| format!("wss://{}/ws", &origin[8..])).unwrap_or_else(|| "ws://127.0.0.1:3000".into())}),
                    ),
                    Err(_) => Reply::error(503, "ticket capacity unavailable"),
                }
            }
            "/api/transport-probe" => {
                let Ok(probe) = serde_json::from_slice::<Probe>(body) else {
                    return Reply::error(400, "invalid probe report");
                };
                if !probe.ws
                    || !probe.rtc
                    || probe.session_id.len() > 64
                    || !self.profiles.report_probe(cookie, &probe.session_id, now)
                {
                    return Reply::error(403, "active guest and both channel receipts required");
                }
                Reply::new(200, json!({"accepted":true,"expires_in_ms":5000}))
            }
            _ => Reply::error(404, "not found"),
        }
    }
}
pub struct HttpControl {
    stop: Arc<AtomicBool>,
    worker: thread::JoinHandle<()>,
}
impl HttpControl {
    pub fn start(shared: SharedControl, started: Instant) -> std::io::Result<Self> {
        // Bind synchronously so a port conflict fails startup before readiness.
        let server =
            tiny_http::Server::http(HTTP_ADDR).map_err(|e| std::io::Error::other(e.to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let worker = thread::Builder::new()
            .name("local-control".into())
            .spawn(move || {
                while !signal.load(Ordering::Relaxed) {
                    let request = match server.recv_timeout(Duration::from_millis(50)) {
                        Ok(Some(request)) => request,
                        Ok(None) => continue,
                        Err(_) => return,
                    };
                    serve(request, &shared, started.elapsed().as_millis() as u64);
                }
            })?;
        Ok(Self { stop, worker })
    }
    pub fn healthy(&self) -> bool {
        !self.worker.is_finished()
    }
}
impl Drop for HttpControl {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
fn serve(mut request: tiny_http::Request, shared: &SharedControl, now: u64) {
    let header = |name: &'static str| {
        request
            .headers()
            .iter()
            .filter(|h| h.field.equiv(name))
            .map(|h| h.value.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    let origins = header("Origin");
    let cookies = header("Cookie");
    let hosts = header("Host");
    let types = header("Content-Type");
    let cookie = cookies.first().and_then(|c| parse_cookie(c));
    let method = request.method().as_str().to_owned();
    let path = request.url().to_owned();
    let reply = if hosts.len() != 1
        || !matches!(hosts[0].as_str(), "127.0.0.1:3001" | "localhost:3001")
        || origins.len() > 1
        || cookies.len() > 1
    {
        Reply::error(400, "invalid local headers")
    } else if method == "POST"
        && (request.body_length().is_none_or(|n| n > 1024)
            || types.len() != 1
            || types[0] != "application/json")
    {
        Reply::error(400, "bounded application/json body required")
    } else {
        let mut bytes = Vec::new();
        match request.as_reader().take(1025).read_to_end(&mut bytes) {
            Ok(_) => shared.lock().expect("control lock").route(
                &method,
                &path,
                origins.first().map(String::as_str),
                cookie.as_deref(),
                &bytes,
                now,
            ),
            Err(_) => Reply::error(400, "body read failed"),
        }
    };
    let mut response =
        tiny_http::Response::from_string(reply.body.to_string()).with_status_code(reply.code);
    for (key, value) in [
        ("Content-Type", "application/json"),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
    ] {
        response.add_header(tiny_http::Header::from_bytes(key, value).expect("static header"));
    }
    if let Some(cookie) = reply.cookie {
        response.add_header(
            tiny_http::Header::from_bytes("Set-Cookie", cookie).expect("cookie header"),
        );
    }
    let _ = request.respond(response);
}
use std::io::Read;
fn parse_cookie(header: &str) -> Option<String> {
    let values: Vec<_> = header
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .filter(|(key, _)| *key == COOKIE)
        .collect();
    if values.len() != 1 {
        return None;
    }
    let value = values[0].1;
    (value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())).then(|| value.to_owned())
}
