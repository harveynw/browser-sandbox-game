//! Shared monotonic event-ID allocation for native combat/impact presentation.
use protocol::EventId;
/// Allocates ordered IDs for combat events.
#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct CombatSystem {
    /// Last allocated combat event sequence number.
    event: u32,
}

impl CombatSystem {
    /// Allocates the next combat event ID.
    pub(crate) fn event_id(&mut self) -> EventId {
        self.event = self
            .event
            .checked_add(1)
            .expect("combat event IDs exhausted");
        EventId(self.event)
    }
}
