//! Shared monotonic event-ID allocation for native combat/impact presentation.
use protocol::EventId;
#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct CombatSystem {
    event: u32,
}

impl CombatSystem {
    pub(crate) fn event_id(&mut self) -> EventId {
        self.event = self
            .event
            .checked_add(1)
            .expect("combat event IDs exhausted");
        EventId(self.event)
    }
}
