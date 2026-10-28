use server::{
    config::Config,
    content_loader::WorldContent,
    control::{Control, HttpControl, SharedControl},
    sessions::Sessions,
    systems::Systems,
    tick,
    transport::Network,
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
const STEP: Duration = Duration::from_nanos(1_000_000_000 / 30);
#[derive(Default)]
struct Scheduler {
    accumulated: Duration,
    overload_streak: u32,
}
impl Scheduler {
    /// Drop excess wall time, never skip simulation ticks or vary physics DT.
    fn advance(&mut self, elapsed: Duration) -> (u32, bool) {
        self.accumulated = self.accumulated.saturating_add(elapsed);
        let due = self.accumulated.as_nanos() / STEP.as_nanos();
        let steps = due.min(5) as u32;
        if due > 5 {
            self.accumulated =
                Duration::from_nanos((self.accumulated.as_nanos() % STEP.as_nanos()) as u64);
            self.overload_streak = self.overload_streak.saturating_add(1);
        } else {
            self.accumulated = self.accumulated.saturating_sub(STEP * steps);
            if steps > 0 {
                self.overload_streak = 0;
            }
        }
        (steps, self.overload_streak >= 3)
    }
}

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
    let mut sessions = Sessions::default();
    let mut systems = Systems::new(&mut world, &mut sessions)?;
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
    let mut network =
        Network::start("127.0.0.1:3000").map_err(|e| format!("WS bind/start: {e}"))?;
    println!(
        "Port Meridian localhost world {}: WS/RTC 127.0.0.1:3000, HTTP 127.0.0.1:3001",
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
        "Localhost development issuer only. Readiness UNPROVEN until a real client probe reports WS and RTC pong receipt. Open http://localhost:5173 and choose Play, or /diagnostic and Connect."
    );
    let mut scheduler = Scheduler::default();
    game_loop(
        &mut world,
        &mut sessions,
        &mut systems,
        &mut network,
        &control,
        &http,
        &running,
        &mut scheduler,
        started,
    )?;
    let mut shared = control.lock().map_err(|_| "control state poisoned")?;
    shared.status.overloaded = true;
    shared.profiles.probe_until_ms = 0;
    let drained = tick::shutdown(
        &mut world,
        &mut sessions,
        &mut systems,
        &mut shared.profiles,
    );
    println!(
        "Shutdown persistence drained: {drained}; unresolved repair debits recover at next boot."
    );
    Ok(())
}
/// Poll networking and advance fixed world ticks until shutdown or a resource failure.
fn game_loop(
    world: &mut World,
    sessions: &mut Sessions,
    systems: &mut Systems,
    network: &mut Network,
    control: &SharedControl,
    http: &HttpControl,
    running: &AtomicBool,
    scheduler: &mut Scheduler,
    started: Instant,
) -> Result<(), String> {
    let mut previous = Instant::now();
    while running.load(Ordering::Relaxed) {
        if !network.healthy() || !http.healthy() {
            return Err("transport boundary or control worker failed".into());
        }
        network.poll();
        if !network.healthy() {
            return Err("transport boundary failed".into());
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
            tick::step(
                world,
                sessions,
                systems,
                network,
                &mut shared.profiles,
                now,
                !overloaded,
            )?;
            let queues = if world.tick.is_multiple_of(30) {
                network.queue_metrics()
            } else {
                serde_json::Value::Null
            };
            shared.record_performance(world, sessions, systems, queues, gap, now);
            shared.update(world, sessions, now, network.healthy(), overloaded);
            shared.finish_tick(tick_started.elapsed(), world.players.len() == 16);
        }
        network.flush();
        thread::sleep(Duration::from_millis(2));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
