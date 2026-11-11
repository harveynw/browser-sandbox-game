use server::{
    config::Config,
    content_loader::WorldContent,
    control::{Control, HttpControl, SharedControl},
    tick::{PreparedWorld, TickDriver},
    transport::Network,
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
    let prepared = PreparedWorld::new(content)?;
    if check_content {
        println!(
            "Server content and 16 character spawn slots verified: {}",
            prepared.build().content_hash.0
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
    let mut state = Control::new(prepared.build());
    state.profiles = profiles;
    state.public_origin = config.network.public_origin.clone();
    let control = Arc::new(Mutex::new(state));
    let http = HttpControl::start(control.clone(), started)
        .map_err(|e| format!("HTTP bind/start: {e}"))?;
    let network = Network::start_with_host("127.0.0.1:3000", config.network.webrtc_host)
        .map_err(|e| format!("WS bind/start: {e}"))?;
    println!(
        "Port Meridian localhost world {}: WS/RTC 127.0.0.1:3000, HTTP 127.0.0.1:3001",
        prepared.world_id()
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
    let mut driver = prepared.start(network);
    let result = game_loop(
        &mut driver,
        &control,
        &http,
        &running,
        &mut scheduler,
        started,
    );
    let mut shared = control.lock().map_err(|_| "control state poisoned")?;
    shared.status.overloaded = true;
    shared.profiles.probe_until_ms = 0;
    let drained = driver.shutdown(&mut shared.profiles);
    println!(
        "Shutdown persistence drained: {drained}; unresolved repair debits recover at next boot."
    );
    result
}
/// Poll networking and advance fixed world ticks until shutdown or a resource failure.
fn game_loop(
    driver: &mut TickDriver,
    control: &SharedControl,
    http: &HttpControl,
    running: &AtomicBool,
    scheduler: &mut Scheduler,
    started: Instant,
) -> Result<(), String> {
    let mut previous = Instant::now();
    while running.load(Ordering::Relaxed) {
        if !driver.network().healthy() || !http.healthy() {
            return Err("transport boundary or control worker failed".into());
        }
        driver.network_mut().poll();
        if !driver.network().healthy() {
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
            driver.step(&mut shared.profiles, now, !overloaded)?;
            let healthy = driver.network().healthy();
            let queues = if driver.tick().is_multiple_of(30) {
                driver.network().queue_metrics()
            } else {
                serde_json::Value::Null
            };
            let snapshot = driver.diagnostics();
            shared.record_performance(&snapshot, queues, gap, now);
            shared.update(&snapshot, now, healthy, overloaded);
            shared.finish_tick(tick_started.elapsed(), snapshot.players == 16);
        }
        driver.network_mut().flush();
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
