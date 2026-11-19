//! Tick phase labels, failure gating and ordered phase configuration.
use bevy_ecs::{prelude::*, schedule::SingleThreadedExecutor};
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TickPhase {
    Admission,
    Planning,
    Movement,
    Combat,
    Police,
    FailedPeers,
    Repairs,
    Missions,
    Publish,
    Flush,
}

/// Schedule outcome is explicit: normal gameplay errors must not become Bevy panics.
#[derive(Resource, Default)]
pub struct TickOutcome {
    /// First error recorded during the current tick.
    error: Option<String>,
}
impl TickOutcome {
    pub(crate) fn is_healthy(&self) -> bool {
        self.error.is_none()
    }
    /// Records a failure and preserves the state needed by later cleanup.
    pub fn fail(&mut self, error: String) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
    /// Returns and clears the first recorded tick error.
    pub fn finish(&mut self) -> Result<(), String> {
        self.error.take().map_or(Ok(()), Err)
    }
}
/// Checks whether the service remains available for its next operation.
pub(crate) fn healthy(outcome: Res<TickOutcome>) -> bool {
    outcome.is_healthy()
}

/// Register existing gameplay phases in these sets. All systems require an explicit set.
/// Chained systems commit deferred lifecycle commands before dependent phases.
pub fn tick_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.set_executor(SingleThreadedExecutor::new());
    schedule.configure_sets(
        (
            TickPhase::Admission,
            TickPhase::Planning,
            TickPhase::Movement,
            TickPhase::Combat,
            TickPhase::Police,
            TickPhase::FailedPeers,
            TickPhase::Repairs,
            TickPhase::Missions,
            TickPhase::Publish,
            TickPhase::Flush,
        )
            .chain(),
    );
    for phase in [
        TickPhase::Admission,
        TickPhase::Planning,
        TickPhase::Movement,
        TickPhase::Combat,
        TickPhase::Police,
        TickPhase::FailedPeers,
        TickPhase::Repairs,
        TickPhase::Missions,
        TickPhase::Publish,
        TickPhase::Flush,
    ] {
        schedule.configure_sets(phase.run_if(healthy));
    }
    schedule
}
