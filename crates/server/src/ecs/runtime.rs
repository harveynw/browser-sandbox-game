//! Scoped tick services, scratch state and phase instrumentation.
use super::{TickOutcome, TickPhase};
use crate::{admission::Profiles, systems::movement::MovementFrame};
use bevy_ecs::prelude::*;
use std::time::Instant;
pub(crate) struct TickIo {
    pub(crate) profiles: Profiles,
    pub(crate) now: u64,
    pub(crate) allow_joins: bool,
}
#[derive(Resource, Default)]
pub(crate) struct Scratch {
    pub(crate) frame: Option<MovementFrame>,
    pub(crate) gameplay_started: Option<Instant>,
    pub(crate) movement_started: Option<Instant>,
    #[cfg(test)]
    pub(crate) trace: Vec<TickPhase>,
    #[cfg(test)]
    pub(crate) fail_at: Option<TickPhase>,
}
impl Scratch {
    pub(crate) fn reset(&mut self) {
        self.frame = None;
        self.gameplay_started = None;
        self.movement_started = None;
        #[cfg(test)]
        self.trace.clear();
    }
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
    pub(crate) fn frame(&mut self) -> Result<&mut MovementFrame, String> {
        self.frame
            .as_mut()
            .ok_or_else(|| "tick frame missing".into())
    }
}

pub(super) fn combat(mut scratch: ResMut<Scratch>, mut outcome: ResMut<TickOutcome>) {
    if scratch.enter(TickPhase::Combat, &mut outcome) {
        scratch.gameplay_started = Some(Instant::now());
    }
}
pub(super) fn police(mut scratch: ResMut<Scratch>, mut outcome: ResMut<TickOutcome>) {
    scratch.enter(TickPhase::Police, &mut outcome);
}
pub(super) fn repairs(mut scratch: ResMut<Scratch>, mut outcome: ResMut<TickOutcome>) {
    scratch.enter(TickPhase::Repairs, &mut outcome);
}
pub(super) fn begin_missions(mut scratch: ResMut<Scratch>, mut outcome: ResMut<TickOutcome>) {
    scratch.enter(TickPhase::Missions, &mut outcome);
}
pub(super) fn missions(
    scratch: Res<Scratch>,
    mut timings: ResMut<crate::performance::WorldTimings>,
) {
    if let Some(started) = scratch.gameplay_started {
        timings.gameplay.observe(started.elapsed());
    }
}
