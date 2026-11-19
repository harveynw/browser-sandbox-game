//! Scoped tick services, scratch state and phase instrumentation.
use super::{TickOutcome, TickPhase};
use crate::{admission::Profiles, systems::movement::MovementFrame};
use bevy_ecs::prelude::*;
use std::time::Instant;
/// Lends profile storage and admission settings to one tick.
pub(crate) struct TickIo {
    /// Profile storage and committed identity state.
    pub(crate) profiles: Profiles,
    /// Current monotonic time in milliseconds.
    pub(crate) now: u64,
    /// Whether the current tick may admit new players.
    pub(crate) allow_joins: bool,
}
/// Holds temporary movement data and timing state for one tick.
#[derive(Resource, Default)]
pub(crate) struct Scratch {
    /// Temporary movement results shared with later phases.
    pub(crate) frame: Option<MovementFrame>,
    /// Start time used to measure gameplay work.
    pub(crate) gameplay_started: Option<Instant>,
    /// Start time used to measure movement work.
    pub(crate) movement_started: Option<Instant>,
    /// Tick phases entered during a test step.
    #[cfg(test)]
    pub(crate) trace: Vec<TickPhase>,
    /// Phase where a test injects a failure.
    #[cfg(test)]
    pub(crate) fail_at: Option<TickPhase>,
}
impl Scratch {
    /// Clears temporary state before the next simulation tick.
    pub(crate) fn reset(&mut self) {
        self.frame = None;
        self.gameplay_started = None;
        self.movement_started = None;
        #[cfg(test)]
        self.trace.clear();
    }
    /// Records phase entry and applies configured test failures.
    pub(crate) fn enter(&mut self, phase: TickPhase, outcome: &mut TickOutcome) -> bool {
        #[cfg(test)]
        {
            self.trace.push(phase);
            if self.fail_at == Some(phase) {
                self.fail_at = None;
                outcome.fail(format!("injected {phase:?} failure"));
                return false;
            }
        }
        #[cfg(not(test))]
        let _ = (phase, outcome);
        true
    }
    /// Returns the temporary movement frame or reports that it is missing.
    pub(crate) fn frame(&mut self) -> Result<&mut MovementFrame, String> {
        self.frame
            .as_mut()
            .ok_or_else(|| "tick frame missing".into())
    }
}

/// Starts the combat phase after movement succeeds.
pub(super) fn combat(mut scratch: ResMut<Scratch>, mut outcome: ResMut<TickOutcome>) {
    if scratch.enter(TickPhase::Combat, &mut outcome) {
        scratch.gameplay_started = Some(Instant::now());
    }
}
/// Starts the police phase after combat succeeds.
pub(super) fn police(mut scratch: ResMut<Scratch>, mut outcome: ResMut<TickOutcome>) {
    scratch.enter(TickPhase::Police, &mut outcome);
}
/// Starts the repair phase after police processing succeeds.
pub(super) fn repairs(mut scratch: ResMut<Scratch>, mut outcome: ResMut<TickOutcome>) {
    scratch.enter(TickPhase::Repairs, &mut outcome);
}
/// Records entry into the mission phase and applies configured test failures.
pub(super) fn begin_missions(mut scratch: ResMut<Scratch>, mut outcome: ResMut<TickOutcome>) {
    scratch.enter(TickPhase::Missions, &mut outcome);
}
/// Completes the mission phase and records gameplay duration.
pub(super) fn missions(
    scratch: Res<Scratch>,
    mut timings: ResMut<crate::performance::WorldTimings>,
) {
    if let Some(started) = scratch.gameplay_started {
        timings.gameplay.observe(started.elapsed());
    }
}
