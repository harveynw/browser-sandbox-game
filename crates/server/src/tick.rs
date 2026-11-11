//! Small server-facing driver; ECS setup and schedule wiring live in ecs/.
use crate::{
    admission::Profiles,
    ecs::{
        TickOutcome,
        runtime::{Scratch, TickIo},
    },
    transport::Network,
};
#[cfg(test)]
use crate::{sessions::Sessions, systems::Systems, world::World as FixtureWorld};
use bevy_ecs::prelude::*;

/// Validated ECS startup state before sockets are bound. Content-check mode can
/// inspect this without creating a dummy transport or starting the live loop.
pub struct PreparedWorld {
    host: bevy_ecs::world::World,
}
impl PreparedWorld {
    pub fn new(content: crate::content_loader::WorldContent) -> Result<Self, String> {
        Ok(Self {
            host: crate::ecs::bootstrap::build(content)?,
        })
    }
    pub fn build(&self) -> protocol::ServerHello {
        crate::world::build(&self.host.resource::<crate::ecs::state::Content>().0)
    }
    pub fn world_id(&self) -> &str {
        &self.host.resource::<crate::ecs::state::WorldIdentity>().0
    }
    pub fn start(self, network: Network) -> TickDriver {
        TickDriver::from_host(self.host, network)
    }
}

pub struct TickDriver {
    host: bevy_ecs::world::World,
    schedule: Schedule,
    diagnostics: bevy_ecs::system::SystemState<crate::diagnostics::Diagnostics<'static, 'static>>,
}
impl TickDriver {
    #[cfg(test)]
    pub(crate) fn from_parts(
        core: FixtureWorld,
        sessions: Sessions,
        systems: Systems,
        network: Network,
    ) -> Self {
        let mut host = crate::ecs::state::install(core);
        host.insert_resource(sessions);
        crate::ecs::state::install_systems(&mut host, systems);
        Self::from_host(host, network)
    }
    pub fn new(
        content: crate::content_loader::WorldContent,
        network: Network,
    ) -> Result<Self, String> {
        Ok(PreparedWorld::new(content)?.start(network))
    }
    fn from_host(mut host: bevy_ecs::world::World, network: Network) -> Self {
        crate::ecs::schedule::initialize(&mut host, network);
        let schedule = crate::ecs::schedule::build();
        let diagnostics = bevy_ecs::system::SystemState::new(&mut host);
        Self {
            host,
            schedule,
            diagnostics,
        }
    }
    pub fn network(&self) -> &Network {
        self.host.non_send::<Network>()
    }
    pub fn network_mut(&mut self) -> &mut Network {
        self.host.non_send_mut::<Network>().into_inner()
    }
    pub fn tick(&self) -> u32 {
        self.host.resource::<crate::ecs::state::Clock>().0
    }
    pub fn diagnostics(&mut self) -> crate::diagnostics::Snapshot {
        self.diagnostics
            .get(&self.host)
            .expect("read-only diagnostics")
            .snapshot()
    }
    pub fn build(&self) -> protocol::ServerHello {
        crate::world::build(&self.host.resource::<crate::ecs::state::Content>().0)
    }
    pub fn world_id(&self) -> &str {
        &self.host.resource::<crate::ecs::state::WorldIdentity>().0
    }
    #[cfg(test)]
    pub(crate) fn into_parts(mut self) -> (FixtureWorld, Sessions, Systems, Network, Schedule) {
        let network = self.host.remove_non_send::<Network>().unwrap();
        let (core, sessions, systems) = crate::ecs::state::unpack(self.host);
        (core, sessions, systems, network, self.schedule)
    }
    #[cfg(test)]
    pub(crate) fn use_schedule(&mut self, schedule: Schedule) {
        self.schedule = schedule;
    }
    pub fn step(
        &mut self,
        profiles: &mut Profiles,
        now: u64,
        allow_joins: bool,
    ) -> Result<(), String> {
        if self.tick() >= u32::MAX - 1 {
            return Err("world tick exhausted; restart required".into());
        }
        crate::gameplay_messages::clear(&mut self.host);
        self.host.resource_mut::<Scratch>().reset();
        // A previous returned failure cannot suppress the next tick's admission phase.
        let _ = self.host.resource_mut::<TickOutcome>().finish();
        self.host.insert_non_send(TickIo {
            profiles: std::mem::take(profiles),
            now,
            allow_joins,
        });
        self.schedule.run(&mut self.host);
        let io = self
            .host
            .remove_non_send::<TickIo>()
            .expect("scoped tick IO");
        *profiles = io.profiles;
        crate::gameplay_messages::clear(&mut self.host);
        self.host.resource_mut::<Scratch>().frame = None;
        self.host.resource_mut::<Scratch>().gameplay_started = None;
        self.host.resource_mut::<Scratch>().movement_started = None;
        self.host.resource_mut::<TickOutcome>().finish()
    }
    pub fn shutdown(&mut self, profiles: &mut Profiles) -> bool {
        crate::shutdown::run(&mut self.host, profiles)
    }
}
#[cfg(test)]
#[path = "ecs/driver_tests.rs"]
mod tests;
