//! Read-only projections for the HTTP control plane; never detach resources or
//! borrow the gameplay world through a mutable legacy wrapper.
use crate::{
    ecs::GameId,
    ecs::state::{Clock, WorldIdentity},
    entities::{pedestrian::Pedestrian, player::Player, vehicle::Vehicle},
    metadata::PoolMembership,
    sessions::{SessionPhase, Sessions},
    systems::{
        police::{Officer, PoliceSystem, Unit, Wanted},
        population::{PopulationSystem, Traffic, VehiclePool},
    },
};
use bevy_ecs::{prelude::*, system::SystemParam};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub struct Snapshot {
    pub tick: u32,
    pub players: usize,
    pub sessions: usize,
    pub loading: usize,
    pub playing: usize,
    pub(crate) metrics: Option<Value>,
}
#[derive(SystemParam)]
pub(crate) struct Diagnostics<'w, 's> {
    players: Query<'w, 's, &'static Player>,
    cars: Query<'w, 's, &'static Vehicle>,
    peds: Query<'w, 's, &'static Pedestrian>,
    traffic: Query<'w, 's, &'static Traffic>,
    pools: Query<'w, 's, &'static PoolMembership>,
    units: Query<'w, 's, (&'static GameId, &'static Unit)>,
    officers: Query<'w, 's, (&'static GameId, &'static Officer)>,
    wanted: Query<'w, 's, (&'static GameId, &'static Wanted)>,
    sessions: Res<'w, Sessions>,
    clock: Res<'w, Clock>,
    identity: Res<'w, WorldIdentity>,
    population: Res<'w, PopulationSystem>,
    police: Res<'w, PoliceSystem>,
    timings: Res<'w, crate::performance::WorldTimings>,
    metrics: Res<'w, crate::world::Metrics>,
}
impl Diagnostics<'_, '_> {
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
