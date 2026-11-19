//! Connection lifecycle, admission and validation of client intent.
pub(crate) mod ingress;
pub(crate) mod native;
#[cfg(test)]
use crate::admission::Profiles;
use crate::component::PlayerSnapshot;
use crate::replication::{Budget, InputRate};
use crate::systems::police::Unit as PoliceUnitComponent;
use crate::systems::police::Wanted as WantedComponent;
use crate::transport::*;
use crate::world::{MAX_PLAYERS, spawn_position};
#[cfg(test)]
use crate::{systems::Systems, world::World};
use protocol::*;
use std::collections::{BTreeMap, VecDeque};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPhase {
    AwaitHello,
    Loading,
    Playing,
}
/// Counts gameplay bytes received and sent by a session.
#[derive(Default, serde::Serialize)]
pub struct TrafficCounters {
    /// Total gameplay bytes received from the peer.
    pub up_bytes: u64,
    /// Total gameplay bytes sent to the peer.
    pub down_bytes: u64,
}
/// Tracks one peer's identity, input, visibility, and delivery limits.
pub struct Session {
    /// Client-reported round-trip time and the tick when it was received.
    pub(crate) reported_ping: Option<(u16, u32)>,
    /// Gameplay bytes received and sent by this session.
    pub traffic: TrafficCounters,
    /// Session ID assigned after a successful hello.
    pub id: Option<String>,
    /// Committed guest profile.
    pub profile: Option<ProfileView>,
    /// Stable gameplay ID of the player.
    pub player: Option<EntityId>,
    /// Persistence operation ID awaited before admission.
    pub(crate) pending_admission: Option<String>,
    /// Current stage of this item's lifecycle.
    pub phase: SessionPhase,
    /// Control generation used to reject stale client actions and inputs.
    pub epoch: u32,
    /// Current full-state baseline sequence number.
    pub baseline: u32,
    /// Next visibility generation assigned by this session.
    pub(crate) visibility_counter: u32,
    /// Entity IDs mapped to their current visibility generations.
    pub(crate) visible: BTreeMap<u32, u32>,
    /// Accepted input frames indexed by simulation tick.
    pub inputs: BTreeMap<u32, InputFrame>,
    /// Cached action results indexed by client request ID.
    pub(crate) actions: BTreeMap<u32, ActionResult>,
    /// Highest action request ID accepted by the session.
    pub(crate) last_request: u32,
    /// Repair progress last exposed to this session.
    pub repair_view: Option<RepairView>,
    /// Tick when the repair status must be hidden.
    pub(crate) repair_clear: Option<u32>,
    /// Sequence number of the private status state.
    pub(crate) status_revision: u32,
    /// Whether private status must be published again.
    pub(crate) status_dirty: bool,
    /// Monotonic time of the last accepted client message.
    pub(crate) last_heard: u64,
    /// Monotonic deadline for the current session phase, in milliseconds.
    pub(crate) deadline: u64,
    /// Monotonic time of the last accepted resynchronisation request.
    pub(crate) last_resync: Option<u64>,
    /// Byte budget for incoming session traffic.
    pub(crate) ingress_budget: Budget,
    /// Byte budget for outgoing session traffic.
    pub(crate) egress_budget: Budget,
    /// Rolling limit on accepted movement inputs.
    pub(crate) input_rate: InputRate,
    /// Rate budget for client actions.
    pub(crate) action_rate: Budget,
    /// Rate budget for automatic-fire trigger updates.
    pub(crate) trigger_rate: Budget,
    /// Last publication tick recorded for each visible entity.
    pub(crate) last_replicated: BTreeMap<u32, u32>,
}

/// Owns active peer sessions and their pending output and disconnects.
#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct Sessions {
    /// Session state indexed by connection incarnation.
    pub peers: BTreeMap<PeerKey, Session>,
    /// Protocol messages queued for delivery.
    pub(crate) output: VecDeque<(PeerKey, Delivery, ServerEnvelope)>,
    /// Peers waiting for cleanup, paired with their disconnect reasons.
    pub(crate) closing: BTreeMap<PeerKey, &'static str>,
}
#[cfg(test)]
impl Sessions {
    /// Releases session-owned state and pending output.
    pub(crate) fn cleanup(
        &mut self,
        session: &Session,
        profiles: &mut Profiles,
        world: &mut World,
        systems: &mut Systems,
    ) {
        if let Some(id) = session.player {
            systems.chat.on_left(id.0, world, self);
            let mut live = world.entities.live();
            let mut state =
                bevy_ecs::system::SystemState::<native::LifecycleActors>::new(&mut live);
            native::remove_player(
                id,
                &mut state.get_mut(&mut live).expect("session lifecycle queries"),
            );
            state.apply(&mut live);
            drop(live);
            systems.police.forget_crimes(id.0);
            crate::systems::missions::MissionSystem::refresh_job_proxies(world);
        }
        if let (Some(profile), Some(id)) = (&session.profile, &session.id) {
            profiles.deactivate(&profile.profile_id.0, id);
        }
    }
    /// Requests peer cleanup and records its reason.
    pub(crate) fn disconnect(
        &mut self,
        peer: PeerKey,
        profiles: &mut Profiles,
        world: &mut World,
        systems: &mut Systems,
    ) {
        if let Some(session) = self.peers.remove(&peer) {
            self.cleanup(&session, profiles, world, systems);
        }
    }
}

impl Session {
    /// The entity allocated at admission; callers must have checked the session phase.
    pub(crate) fn player_id(&self) -> u32 {
        self.player.expect("admitted player").0
    }
}
