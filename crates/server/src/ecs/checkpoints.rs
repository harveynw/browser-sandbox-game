//! Reusable schedule groups. Domain modules supply systems, not ordering.
use bevy_ecs::{prelude::*, schedule::ScheduleConfigs, system::ScheduleSystem};
pub(crate) fn reactions() -> ScheduleConfigs<ScheduleSystem> {
    use crate::{
        gameplay_messages::clear_batch,
        systems::{chat, police, population},
    };
    (
        police::receive_crimes.run_if(crate::ecs::healthy),
        population::hear_noise.run_if(crate::ecs::healthy),
        chat::announce_eliminations.run_if(crate::ecs::healthy),
        clear_batch,
    )
        .chain()
}
pub(crate) fn death_checkpoint()
-> bevy_ecs::schedule::ScheduleConfigs<bevy_ecs::system::ScheduleSystem> {
    use crate::{
        ecs::healthy,
        systems::{
            impact_consequences as impacts,
            lifecycle::players::{down_players, fail_downed_missions, retire_death_messages},
        },
    };
    (
        down_players.run_if(healthy),
        fail_downed_missions.run_if(healthy),
        impacts::find_disabled_occupants.run_if(healthy),
        impacts::recover_occupants.run_if(healthy),
        impacts::fail_crash_missions.run_if(healthy),
        crate::replication::apply_control_transitions.run_if(healthy),
        crate::replication::retire_control_transitions,
        retire_death_messages,
    )
        .chain()
}
pub(crate) fn publication() -> ScheduleConfigs<ScheduleSystem> {
    use crate::{ecs::healthy, replication::publication::*};
    (
        begin,
        player_map.run_if(healthy),
        crate::replication::native::sync_visible.run_if(healthy),
        owners.run_if(healthy),
        snapshots.run_if(healthy),
        statuses.run_if(healthy),
        commit.run_if(healthy),
        finish,
    )
        .chain()
}
pub(crate) fn egress() -> Schedule {
    use crate::replication::egress::{failed, send};
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            send,
            (
                crate::sessions::native::disconnect_requested,
                crate::sessions::native::retire_disconnects,
                crate::systems::missions::native::refresh_proxies,
                crate::systems::repairs::native::update.run_if(crate::ecs::healthy),
                crate::replication::native::sync_visible.run_if(crate::ecs::healthy),
            )
                .chain()
                .run_if(failed)
                .run_if(crate::ecs::healthy),
        )
            .chain(),
    );
    schedule
}
