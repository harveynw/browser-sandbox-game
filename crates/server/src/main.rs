use server::{
    content_loader::WorldContent,
    control::{Control, HttpControl},
    scheduler::Scheduler,
    transport::NetCrateTransport,
    world::World,
};
use std::{
    path::Path,
    process::ExitCode,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.is_empty() && args != ["--check-content"] {
        return Err("Usage: cargo run -p server -- [--check-content]".into());
    }
    let content = WorldContent::load_index(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
        "city.json",
    )?;
    let mut world = World::new(content)?;
    if !args.is_empty() {
        println!(
            "Server content and 16 character spawn slots verified: {}",
            world.build().content_hash.0
        );
        return Ok(());
    }
    let started = Instant::now();
    let control = Arc::new(Mutex::new(Control::new(&world)));
    let http = HttpControl::start(control.clone(), started)
        .map_err(|e| format!("HTTP bind/start: {e}"))?;
    let mut transport =
        NetCrateTransport::start("127.0.0.1:3000").map_err(|e| format!("WS bind/start: {e}"))?;
    println!(
        "T16 localhost world {}: WS/RTC 127.0.0.1:3000, HTTP 127.0.0.1:3001",
        world.id
    );
    println!(
        "Memory profiles; localhost development issuer only. Readiness UNPROVEN until the browser reports WS and RTC pong receipt. Open http://localhost:5173 and choose Play, or /diagnostic and Connect."
    );
    let mut scheduler = Scheduler::default();
    let mut previous = Instant::now();
    loop {
        if !transport.healthy() || !http.healthy() {
            return Err("transport/control worker failed".into());
        }
        let current = Instant::now();
        let (steps, overloaded) = scheduler.advance(current.duration_since(previous));
        previous = current;
        for _ in 0..steps {
            let now = started.elapsed().as_millis() as u64;
            let mut shared = control.lock().map_err(|_| "control state poisoned")?;
            world.step(&mut transport, &mut shared.profiles, now, !overloaded)?;
            shared.update(&world, now, transport.healthy(), overloaded);
        }
        thread::sleep(Duration::from_millis(2));
    }
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Server failed: {error}");
            ExitCode::FAILURE
        }
    }
}
