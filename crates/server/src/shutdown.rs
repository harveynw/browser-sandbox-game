//! Stop actors through the live lifecycle consumer, then drain the bounded
//! persistence ledger without advancing gameplay, AI, replication or networking.
use crate::{
    admission::Profiles,
    ecs::TickOutcome,
    ecs::runtime::TickIo,
    ecs::state::Clock,
    sessions::{Sessions, native::DisconnectRequested},
    systems::repairs::RepairSystem,
};
use bevy_ecs::prelude::*;
use std::time::{Duration, Instant};
pub(crate) fn request_all(
    sessions: Res<Sessions>,
    clock: Res<Clock>,
    mut requests: MessageWriter<DisconnectRequested>,
) {
    for &peer in sessions.peers.keys() {
        requests.write(DisconnectRequested {
            peer,
            reason: "server shutdown",
            tick: clock.0,
        });
    }
}
pub(crate) fn run(world: &mut World, profiles: &mut Profiles) -> bool {
    crate::gameplay_messages::clear(world);
    let _ = world.resource_mut::<TickOutcome>().finish();
    world.insert_non_send(TickIo {
        profiles: std::mem::take(profiles),
        now: 0,
        allow_joins: false,
    });
    let (mut retire, mut drain) = crate::ecs::shutdown::schedules();
    retire.run(world);
    let until = Instant::now() + Duration::from_millis(3500);
    while !world.resource::<RepairSystem>().pending.is_empty() && Instant::now() < until {
        let tick = world.resource::<Clock>().0.saturating_add(1);
        world.resource_mut::<Clock>().0 = tick;
        drain.run(world);
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut io = world.remove_non_send::<TickIo>().expect("shutdown IO");
    let persisted = io.profiles.shutdown(Duration::from_secs(1));
    *profiles = io.profiles;
    crate::gameplay_messages::clear(world);
    let clean = world.resource_mut::<TickOutcome>().finish().is_ok();
    persisted && world.resource::<RepairSystem>().pending.is_empty() && clean
}
