use std::time::Duration;
pub const STEP: Duration = Duration::from_nanos(1_000_000_000 / 30);
#[derive(Default)]
pub struct Scheduler {
    accumulated: Duration,
    overload_streak: u32,
}
impl Scheduler {
    /// Drop excess wall time, never skip simulation ticks or vary physics DT.
    pub fn advance(&mut self, elapsed: Duration) -> (u32, bool) {
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
