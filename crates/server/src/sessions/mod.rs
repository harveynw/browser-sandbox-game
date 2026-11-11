//! Connection lifecycle, admission and validation of client intent.
pub(crate) mod ingress;
pub(crate) mod native;
#[cfg(test)]
use crate::admission::Profiles;
use crate::components::Player;
use crate::components::Vehicle as VehicleComponent;
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
#[derive(Default, serde::Serialize)]
pub struct TrafficCounters {
    pub up_bytes: u64,
    pub down_bytes: u64,
}
pub struct Session {
    pub traffic: TrafficCounters,
    pub id: Option<String>,
    pub profile: Option<ProfileView>,
    pub player: Option<EntityId>,
    pub(crate) pending_admission: Option<String>,
    pub phase: SessionPhase,
    pub epoch: u32,
    pub baseline: u32,
    pub(crate) visibility_counter: u32,
    pub(crate) visible: BTreeMap<u32, u32>,
    pub inputs: BTreeMap<u32, InputFrame>,
    pub(crate) actions: BTreeMap<u32, ActionResult>,
    pub(crate) last_request: u32,
    pub repair_view: Option<RepairView>,
    pub(crate) repair_clear: Option<u32>,
    pub(crate) status_revision: u32,
    pub(crate) status_dirty: bool,
    pub(crate) last_heard: u64,
    pub(crate) deadline: u64,
    pub(crate) last_resync: Option<u64>,
    pub(crate) ingress_budget: Budget,
    pub(crate) egress_budget: Budget,
    pub(crate) input_rate: InputRate,
    pub(crate) action_rate: Budget,
    pub(crate) trigger_rate: Budget,
    pub(crate) last_replicated: BTreeMap<u32, u32>,
}

#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct Sessions {
    pub peers: BTreeMap<PeerKey, Session>,
    pub(crate) output: VecDeque<(PeerKey, Delivery, ServerEnvelope)>,
    pub(crate) closing: BTreeMap<PeerKey, &'static str>,
}
#[cfg(test)]
impl Sessions {
    pub(crate) fn cleanup(
        &mut self,
        session: &Session,
        profiles: &mut Profiles,
        world: &mut World,
        systems: &mut Systems,
    ) {
        if let Some(id) = session.player {
            systems.chat.on_left(id.0, world, self);
            let mut state = bevy_ecs::system::SystemState::<native::LifecycleActors>::new(
                world.entities.ecs_mut(),
            );
            native::remove_player(
                id,
                &mut state
                    .get_mut(world.entities.ecs_mut())
                    .expect("session lifecycle queries"),
            );
            state.apply(world.entities.ecs_mut());
            systems.police.forget_crimes(id.0);
            crate::systems::missions::MissionSystem::refresh_job_proxies(world);
        }
        if let (Some(profile), Some(id)) = (&session.profile, &session.id) {
            profiles.deactivate(&profile.profile_id.0, id);
        }
    }
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
