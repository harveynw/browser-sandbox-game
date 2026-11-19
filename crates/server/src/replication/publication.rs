//! Ordered publication responsibilities. Snapshot/status packets are staged only
//! until this checkpoint commits, preserving per-peer order and failure semantics.
use super::{
    native::{OwnerActors, PeerPackets, Projection, PublicActors, StatusActors},
    *,
};
use crate::{
    ecs::runtime::Scratch,
    ecs::state::{Clock, WorldIdentity},
    ecs::{TickOutcome, TickPhase},
    performance::WorldTimings,
};
use bevy_ecs::prelude::*;
use std::time::Instant;

/// Stages per-peer replication batches until publication succeeds.
#[derive(Resource, Default)]
pub(crate) struct Publication {
    /// Time when the current phase began.
    started: Option<Instant>,
    /// Per-peer packets staged for publication.
    batches: PeerPackets,
}
/// Starts the current phase and prepares its shared working state.
pub(crate) fn begin(
    mut publication: ResMut<Publication>,
    mut scratch: ResMut<Scratch>,
    mut outcome: ResMut<TickOutcome>,
) {
    publication.batches.clear();
    publication.started = scratch
        .enter(TickPhase::Publish, &mut outcome)
        .then(Instant::now);
}
/// Builds the map positions of all online players.
pub(crate) fn player_map(
    actors: OwnerActors,
    clock: Res<Clock>,
    identity: Res<WorldIdentity>,
    mut sessions: ResMut<Sessions>,
) {
    if clock.0.is_multiple_of(30) {
        Projection::player_map(&actors.view(clock.0, &identity.0), &mut sessions);
    }
}
/// Stages authoritative control updates for playing sessions.
pub(crate) fn owners(
    actors: OwnerActors,
    clock: Res<Clock>,
    identity: Res<WorldIdentity>,
    mut sessions: ResMut<Sessions>,
    mut outcome: ResMut<TickOutcome>,
) {
    if clock.0.is_multiple_of(3)
        && let Err(error) =
            Projection::owner_states(&actors.view(clock.0, &identity.0), &mut sessions)
    {
        outcome.fail(error);
    }
}
/// Builds movement snapshots for the current visible entities.
pub(crate) fn snapshots(
    actors: PublicActors,
    clock: Res<Clock>,
    identity: Res<WorldIdentity>,
    sessions: Res<Sessions>,
    mut publication: ResMut<Publication>,
    mut outcome: ResMut<TickOutcome>,
) {
    if !clock.0.is_multiple_of(3) {
        return;
    }
    match Projection::snapshots(&actors.view(clock.0, &identity.0), &sessions) {
        Ok(batches) => publication.batches = batches,
        Err(error) => outcome.fail(error),
    }
}
/// Stages private status updates after snapshot preparation succeeds.
pub(crate) fn statuses(
    actors: StatusActors,
    clock: Res<Clock>,
    identity: Res<WorldIdentity>,
    sessions: Res<Sessions>,
    mut publication: ResMut<Publication>,
    mut outcome: ResMut<TickOutcome>,
) {
    if !clock.0.is_multiple_of(30) {
        return;
    }
    let view = actors.view(clock.0, &identity.0);
    for (&peer, session) in &sessions.peers {
        if session.phase != SessionPhase::Playing {
            continue;
        }
        let status = match Projection::status(clock.0, session, &view) {
            Ok(status) => status,
            Err(error) => {
                outcome.fail(error);
                return;
            }
        };
        publication.batches.entry(peer).or_default().push((
            Delivery::Reliable,
            ServerEnvelope {
                v: ProtocolVersion(4),
                world_id: Some(OpaqueId(identity.0.clone())),
                session_id: session.id.clone().map(OpaqueId),
                message: ServerMessage::PlayerStatus(status),
            },
        ));
    }
}
/// Queues staged replication batches only after publication succeeds.
pub(crate) fn commit(mut publication: ResMut<Publication>, mut sessions: ResMut<Sessions>) {
    // Stable peer order, with each peer's snapshots followed by its periodic status.
    for (peer, packets) in std::mem::take(&mut publication.batches) {
        sessions.output.extend(
            packets
                .into_iter()
                .map(|(delivery, envelope)| (peer, delivery, envelope)),
        );
    }
}
/// Returns and clears the first recorded tick error.
pub(crate) fn finish(
    mut publication: ResMut<Publication>,
    outcome: Res<TickOutcome>,
    mut timings: ResMut<WorldTimings>,
) {
    // Also runs after a substep fails: no partial batch can survive into another tick.
    publication.batches.clear();
    if let Some(started) = publication.started.take()
        && outcome.is_healthy()
    {
        timings.replication.observe(started.elapsed());
    }
}
