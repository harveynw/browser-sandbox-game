//! Bounded transport adapter. Failed sends emit lifecycle requests, then the
//! schedule commits cleanup before attempting any remaining queued envelopes.
use super::*;
use crate::{
    ecs::runtime::{Scratch, TickIo},
    ecs::state::Clock,
    ecs::{TickOutcome, TickPhase},
    sessions::native::DisconnectRequested,
    world::Metrics,
};
use bevy_ecs::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct Drain {
    sent: usize,
    failed: bool,
}
pub(crate) fn failed(drain: Res<Drain>) -> bool {
    drain.failed
}
pub(crate) fn send(
    mut network: NonSendMut<Network>,
    io: NonSend<TickIo>,
    clock: Res<Clock>,
    mut sessions: ResMut<Sessions>,
    mut metrics: ResMut<Metrics>,
    mut requests: MessageWriter<DisconnectRequested>,
    mut drain: ResMut<Drain>,
    mut outcome: ResMut<TickOutcome>,
) {
    drain.failed = false;
    while let Some((peer, delivery, message)) = sessions.output.pop_front() {
        if !sessions.peers.contains_key(&peer)
            && !matches!(
                message.message,
                ServerMessage::Reject(_) | ServerMessage::Disconnect(_)
            )
        {
            continue;
        }
        drain.sent += 1;
        if drain.sent > 4096 {
            outcome.fail("tick output budget exhausted".into());
            return;
        }
        let bytes = match encode_server(&message, delivery) {
            Ok(bytes) => bytes.len(),
            Err(error) => {
                outcome.fail(error.to_string());
                return;
            }
        };
        let is_snapshot = matches!(message.message, ServerMessage::Snapshot(_));
        if let Some(session) = sessions.peers.get_mut(&peer) {
            if !session.egress_budget.allow(
                bytes,
                io.now,
                if is_snapshot { LIFECYCLE_RESERVE } else { 0 },
            ) {
                metrics.budget_drops += 1;
                if delivery == Delivery::Reliable {
                    requests.write(DisconnectRequested {
                        peer,
                        reason: "reliable byte budget exhausted",
                        tick: clock.0,
                    });
                    drain.failed = true;
                    return;
                }
                continue;
            }
            session.traffic.down_bytes += bytes as u64;
            if let ServerMessage::Snapshot(data) = &message.message {
                metrics.snapshot_packets += 1;
                metrics.max_snapshot_bytes = metrics.max_snapshot_bytes.max(bytes);
                for record in &data.records {
                    let previous = session
                        .last_replicated
                        .insert(record.entity_id.0, clock.0)
                        .unwrap_or(clock.0);
                    metrics.max_refresh_age_ticks =
                        metrics.max_refresh_age_ticks.max(clock.0 - previous);
                }
            }
        }
        metrics.egress_bytes += bytes as u64;
        if matches!(
            network.send(peer, delivery, message),
            SendOutcome::Full | SendOutcome::Closed
        ) {
            requests.write(DisconnectRequested {
                peer,
                reason: "output unavailable",
                tick: clock.0,
            });
            drain.failed = true;
            return;
        }
    }
}
#[derive(Default)]
pub(crate) struct FlushSchedule(Option<Schedule>);
pub(crate) fn flush(host: &mut bevy_ecs::world::World, mut local: Local<FlushSchedule>) {
    let entered = host.resource_scope(|host, mut scratch: Mut<Scratch>| {
        scratch.enter(TickPhase::Flush, &mut host.resource_mut::<TickOutcome>())
    });
    if !entered {
        return;
    }
    if let Err(error) = host.resource_mut::<Scratch>().frame() {
        host.resource_mut::<TickOutcome>().fail(error);
        return;
    }
    let started = std::time::Instant::now();
    let schedule = local.0.get_or_insert_with(crate::ecs::checkpoints::egress);
    host.insert_resource(Drain::default());
    loop {
        schedule.run(host);
        if !host.resource::<TickOutcome>().is_healthy() {
            break;
        }
        if host.resource::<Sessions>().output.is_empty() {
            let closing = std::mem::take(&mut host.resource_mut::<Sessions>().closing);
            for (peer, reason) in closing {
                host.non_send_mut::<Network>().disconnect(peer, reason);
            }
            host.resource_mut::<crate::performance::WorldTimings>()
                .flush
                .observe(started.elapsed());
            break;
        }
    }
}
