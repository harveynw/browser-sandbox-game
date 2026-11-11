//! Shutdown schedule wiring; bounded draining is owned by the shutdown service.
use bevy_ecs::prelude::*;
pub(crate) fn schedules() -> (Schedule, Schedule) {
    let mut retire = Schedule::default();
    retire.add_systems(
        (
            crate::shutdown::request_all,
            crate::sessions::native::disconnect_requested,
            crate::sessions::native::retire_disconnects,
            crate::systems::missions::native::refresh_proxies,
        )
            .chain(),
    );
    let mut drain = Schedule::default();
    drain.add_systems(crate::systems::repairs::native::update);
    (retire, drain)
}
