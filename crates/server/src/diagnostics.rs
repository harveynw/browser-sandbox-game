//! Read-only projections for the HTTP control plane; never detach resources or
//! borrow the gameplay world through a mutable legacy wrapper.
use crate::{
    component::PoolMembership,
    ecs::GameId,
    ecs::state::{Clock, WorldIdentity},
    sessions::{SessionPhase, Sessions},
    systems::{
        police::{PoliceSystem, Unit, Wanted},
        population::{PopulationSystem, Traffic, VehiclePool},
    },
};
use bevy_ecs::{prelude::*, system::SystemParam};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Reports the current tick, session counts, and optional diagnostic metrics.
pub struct Snapshot {
    /// Simulation tick recorded for this item.
    pub tick: u32,
    /// Number of admitted player actors.
    pub players: usize,
    /// Number of connected sessions.
    pub sessions: usize,
    /// Number of sessions waiting for readiness.
    pub loading: usize,
    /// Number of sessions currently playing.
    pub playing: usize,
    /// Measurements collected for server diagnostics.
    pub(crate) metrics: Option<Value>,
}
/// Borrows live actors and resources to produce a read-only diagnostic snapshot.
#[derive(SystemParam)]
pub(crate) struct Diagnostics<'w, 's> {
    /// Queries players used by this system.
    players: Query<'w, 's, (), With<crate::component::PlayerData>>,
    /// Queries vehicles used by this system.
    cars: Query<'w, 's, (), With<crate::component::VehicleData>>,
    /// Queries pedestrians used by this system.
    peds: Query<'w, 's, (), With<crate::component::PedestrianData>>,
    /// Queries traffic used by this system.
    traffic: Query<'w, 's, &'static Traffic>,
    /// Queries vehicle pool memberships used by this system.
    pools: Query<'w, 's, &'static PoolMembership>,
    /// Queries police units used by this system.
    units: Query<'w, 's, (&'static GameId, &'static Unit)>,
    /// Queries police officers used by this system.
    officers: Query<
        'w,
        's,
        (
            &'static GameId,
            crate::component::queries::OfficerQueryReadOnly,
        ),
        crate::component::queries::OfficerFilter,
    >,
    /// Queries pursuit state used by this system.
    wanted: Query<'w, 's, (&'static GameId, &'static Wanted)>,
    /// Live session state used by this system.
    sessions: Res<'w, Sessions>,
    /// Current simulation tick resource.
    clock: Res<'w, Clock>,
    /// Unique world identity used to scope protocol messages.
    identity: Res<'w, WorldIdentity>,
    /// Ambient population system state.
    population: Res<'w, PopulationSystem>,
    /// Police response system state.
    police: Res<'w, PoliceSystem>,
    /// Distributions of work duration by tick phase.
    timings: Res<'w, crate::performance::WorldTimings>,
    /// Measurements collected for server diagnostics.
    metrics: Res<'w, crate::world::Metrics>,
}
impl Diagnostics<'_, '_> {
    /// Builds a snapshot from the current authoritative state.
    pub(crate) fn snapshot(&self) -> Snapshot {
        let players = self.players.iter().count();
        let sessions = self.sessions.peers.len();
        let metrics = self.clock.0.is_multiple_of(30).then(|| {
            let officers: BTreeMap<_,_> = self.officers.iter().map(|(id,v)| (id.0.0,v)).collect();
            let wanted: BTreeMap<_,_> = self.wanted.iter().map(|(id,v)| (id.0.0,v)).collect();
            let units: BTreeMap<_,_> = self.units.iter().map(|(id,v)| (id.0.0,v)).collect();
            let peers: Vec<_> = self.sessions.peers.values().map(|s| json!({"player_id":s.player.map(|id|id.0),"phase":format!("{:?}",s.phase),"epoch":s.epoch,"traffic":s.traffic,"input_ring":s.inputs.len()})).collect();
            json!({"world_id":self.identity.0,"tick":self.clock.0,"systems":self.timings.report(),"world":*self.metrics,"population":{"players":players,"sessions":sessions,"traffic":self.traffic.iter().count(),"parked":self.pools.iter().filter(|p|p.0 == VehiclePool::Parked).count(),"pedestrians":self.peds.iter().count(),"police":units.len(),"vehicles":self.cars.iter().count(),"path_jobs":self.population.metrics.path_jobs,"max_path_jobs_per_tick":self.population.metrics.max_path_jobs_per_tick,"blocked_spawns":self.population.metrics.blocked_spawns},"police":self.police.diagnostics(&officers,&wanted,&units),"peers":peers})
        });
        Snapshot {
            tick: self.clock.0,
            players,
            sessions,
            loading: self
                .sessions
                .peers
                .values()
                .filter(|s| s.phase == SessionPhase::Loading)
                .count(),
            playing: self
                .sessions
                .peers
                .values()
                .filter(|s| s.phase == SessionPhase::Playing)
                .count(),
            metrics,
        }
    }
}
