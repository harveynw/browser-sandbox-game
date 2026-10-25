use server::{
    config::Config,
    content_loader::WorldContent,
    control::{Control, HttpControl},
    scheduler::Scheduler,
    transport::NetCrateTransport,
    world::World,
};
use std::{
    path::Path,
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let check_content = args == ["--check-content"];
    let config_path = match args.as_slice() {
        [] => repository.join("config/dev.toml"),
        [arg] if arg == "--check-content" => repository.join("config/dev.toml"),
        [flag, path] if flag == "--config" => repository.join(path),
        _ => return Err("Usage: cargo run -p server -- [--check-content | --config PATH]".into()),
    };
    let content = WorldContent::load_index(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
        "city.json",
    )?;
    let mut world = World::new(content)?;
    if check_content {
        println!(
            "Server content and 16 character spawn slots verified: {}",
            world.build().content_hash.0
        );
        return Ok(());
    }
    let config = Config::load(&config_path)?;
    let profiles = config.profiles(&repository)?;
    let durable = profiles.durable();
    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || signal.store(false, Ordering::Relaxed))
        .map_err(|e| format!("shutdown handler: {e}"))?;
    let started = Instant::now();
    let mut state = Control::new(&world);
    state.profiles = profiles;
    let control = Arc::new(Mutex::new(state));
    let http = HttpControl::start(control.clone(), started)
        .map_err(|e| format!("HTTP bind/start: {e}"))?;
    let mut transport =
        NetCrateTransport::start("127.0.0.1:3000").map_err(|e| format!("WS bind/start: {e}"))?;
    println!(
        "T19 localhost world {}: WS/RTC 127.0.0.1:3000, HTTP 127.0.0.1:3001",
        world.id
    );
    println!(
        "Profile storage: {}",
        if durable {
            "SQLite WAL"
        } else {
            "memory (disposable)"
        }
    );
    println!(
        "Localhost development issuer only. Readiness UNPROVEN until the browser reports WS and RTC pong receipt. Open http://localhost:5173 and choose Play, or /diagnostic and Connect."
    );
    let mut scheduler = Scheduler::default();
    let mut previous = Instant::now();
    while running.load(Ordering::Relaxed) {
        if !transport.healthy() || !http.healthy() {
            return Err("transport/control worker failed".into());
        }
        let current = Instant::now();
        let gap = current.duration_since(previous);
        let (steps, overloaded) = scheduler.advance(gap);
        previous = current;
        for _ in 0..steps {
            let now = started.elapsed().as_millis() as u64;
            let tick_started = Instant::now();
            let mut shared = control.lock().map_err(|_| "control state poisoned")?;
            if !shared.profiles.healthy() {
                return Err("persistence worker failed".into());
            }
            world.step(&mut transport, &mut shared.profiles, now, !overloaded)?;
            let queues = if world.tick.is_multiple_of(30) {
                transport.queue_metrics()
            } else {
                serde_json::Value::Null
            };
            shared.record_performance(&world, queues, gap, now);
            shared.update(&world, now, transport.healthy(), overloaded);
            shared.finish_tick(tick_started.elapsed(), world.players.len() == 16);
        }
        thread::sleep(Duration::from_millis(2));
    }
    let mut shared = control.lock().map_err(|_| "control state poisoned")?;
    shared.status.overloaded = true;
    shared.profiles.probe_until_ms = 0;
    let drained = world.shutdown(&mut shared.profiles);
    println!(
        "Shutdown persistence drained: {drained}; unresolved repair debits recover at next boot."
    );
    Ok(())
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
