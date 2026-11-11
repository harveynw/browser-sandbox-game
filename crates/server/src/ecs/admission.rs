//! Packet-ordered admission schedules and their invocation.
use super::{TickOutcome, TickPhase, runtime::Scratch};
use bevy_ecs::prelude::*;
use std::time::Instant;
pub(super) struct AdmissionSchedules {
    begin: Schedule,
    packet: Schedule,
    finish: Schedule,
    appearance: Schedule,
}
fn protocol_checkpoint() -> bevy_ecs::schedule::ScheduleConfigs<bevy_ecs::system::ScheduleSystem> {
    use crate::sessions::{ingress as protocol, native};
    (
        protocol::replace_guests,
        native::disconnect_requested,
        native::retire_disconnects,
        protocol::admit,
        protocol::baselines,
        native::disconnect_requested,
        native::retire_disconnects,
        crate::systems::missions::native::refresh_proxies,
        protocol::retire,
    )
        .chain()
}
impl Default for AdmissionSchedules {
    fn default() -> Self {
        use crate::sessions::{ingress as protocol, native};
        let mut begin = Schedule::default();
        begin.add_systems(
            (
                protocol::drain,
                native::disconnect_requested,
                native::retire_disconnects,
                crate::systems::missions::native::refresh_proxies,
                protocol::open,
            )
                .chain(),
        );
        let mut packet = Schedule::default();
        packet.add_systems(
            (
                protocol::receive_packet,
                protocol_checkpoint(),
                (
                    crate::systems::weapons::receive_triggers,
                    crate::systems::weapons::execute_actions,
                    crate::systems::weapons::retire_actions,
                    crate::systems::missions::native::actions,
                    crate::systems::missions::native::refresh_proxies,
                    crate::systems::missions::native::retire_actions,
                    native::possess,
                    crate::replication::apply_control_transitions,
                    crate::replication::retire_control_transitions,
                    native::reply_possession,
                    native::retire_possession,
                )
                    .chain()
                    .run_if(crate::ecs::healthy),
            )
                .chain(),
        );
        let mut finish = Schedule::default();
        finish.add_systems((protocol::pending_profiles, protocol_checkpoint()).chain());
        let mut appearance = Schedule::default();
        appearance.add_systems(protocol::appearance);
        Self {
            begin,
            packet,
            finish,
            appearance,
        }
    }
}
pub(super) fn admission(
    host: &mut bevy_ecs::world::World,
    mut schedules: Local<AdmissionSchedules>,
) {
    let started = Instant::now();
    let entered = host.resource_scope(|host, mut scratch: Mut<Scratch>| {
        scratch.enter(
            TickPhase::Admission,
            &mut host.resource_mut::<TickOutcome>(),
        )
    });
    if !entered {
        return;
    }
    host.init_resource::<crate::sessions::ingress::Ingress>();
    schedules.begin.run(host);
    while !host
        .resource::<crate::sessions::ingress::Ingress>()
        .packets
        .is_empty()
        && host.resource::<TickOutcome>().is_healthy()
    {
        // Every actor effect and baseline is committed before the next FIFO packet.
        schedules.packet.run(host);
    }
    while host.resource::<TickOutcome>().is_healthy() {
        schedules.finish.run(host);
        if !host
            .resource::<crate::sessions::ingress::Ingress>()
            .completed_profile
        {
            break;
        }
    }
    if host.resource::<TickOutcome>().is_healthy() {
        schedules.appearance.run(host);
        host.resource_mut::<crate::performance::WorldTimings>()
            .admission
            .observe(started.elapsed());
    }
}
