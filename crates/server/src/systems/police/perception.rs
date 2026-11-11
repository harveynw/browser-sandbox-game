//! Crime ingestion, witness sensing and arrest eligibility for police perception.
use super::native::ArrestRequested;
use super::*;
use crate::{
    ecs::GameId,
    ecs::state::{Clock, Collision},
};
use bevy_ecs::prelude::*;
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct PerceptionActors<'w, 's> {
    players: Query<'w, 's, (Entity, &'static GameId, &'static PlayerComponent)>,
    wanted: Query<'w, 's, (&'static GameId, &'static mut Wanted)>,
    cars: Query<'w, 's, (&'static GameId, &'static Vehicle)>,
    units: Query<'w, 's, (&'static GameId, &'static mut Unit)>,
    officers: Query<'w, 's, (&'static GameId, &'static Officer)>,
}
pub(crate) fn perceive(
    mut actors: PerceptionActors,
    mut police: ResMut<PoliceSystem>,
    mut sessions: ResMut<Sessions>,
    clock: Res<Clock>,
    collision: NonSend<Collision>,
    mut arrests: MessageWriter<ArrestRequested>,
) {
    let identities: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(e, id, _)| (id.0.0, e))
        .collect();
    let players: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(_, id, p)| (id.0.0, p))
        .collect();
    let mut wanted: BTreeMap<_, _> = actors
        .wanted
        .iter_mut()
        .map(|(id, w)| (id.0.0, w.into_inner()))
        .collect();
    let cars: BTreeMap<_, _> = actors.cars.iter().map(|(id, c)| (id.0.0, c)).collect();
    let mut units: BTreeMap<_, _> = actors
        .units
        .iter_mut()
        .map(|(id, u)| (id.0.0, u.into_inner()))
        .collect();
    let officers: BTreeMap<_, _> = actors.officers.iter().map(|(id, o)| (id.0.0, o)).collect();

    let crimes = std::mem::take(&mut police.crimes);
    for peer in sessions.peers.keys().copied().collect::<Vec<_>>() {
        let mut s = sessions.peers.remove(&peer).expect("police session");
        if let Some(id) = s.player {
            let player = &players[&id.0].state;
            if matches!(player.mode, CharacterMode::Downed | CharacterMode::Arrested)
                || player.health.0 == 0
            {
                if wanted
                    .get(&(s.player_id()))
                    .expect("admitted wanted state")
                    .heat
                    > 0
                {
                    **wanted.get_mut(&id.0).expect("wanted component") =
                        crate::systems::police::state::clear_wanted();
                    s.status_dirty = true;
                }
                for unit in units.values_mut().filter(|u| u.target == Some(id.0)) {
                    crate::systems::police::state::release_target(unit);
                }
            } else {
                let before = (
                    wanted
                        .get(&(s.player_id()))
                        .expect("admitted wanted state")
                        .heat,
                    wanted
                        .get(&(s.player_id()))
                        .expect("admitted wanted state")
                        .pursuit,
                );
                ingest_crimes(
                    id.0,
                    clock.0,
                    &crimes,
                    wanted.get_mut(&id.0).expect("wanted component"),
                );
                let p = player
                    .vehicle_id
                    .map(|id| cars[&id.0].state.motor.position.0.map(|v| v.0))
                    .unwrap_or_else(|| player.motor.position.0.map(|v| v.0));
                let witnesses = Witnesses {
                    cars: &cars,
                    units: &units,
                    officers: &officers,
                    collision: &collision.0,
                };
                let seen = witnesses.sees(s.phase, clock.0, p);
                let arrest = witnesses.can_arrest(id.0, s.phase, player, p);
                if advance_pursuit(
                    wanted.get_mut(&id.0).expect("wanted component"),
                    clock.0,
                    player,
                    &witnesses,
                    seen.then_some(p),
                    arrest,
                ) {
                    arrests.write(ArrestRequested {
                        peer,
                        player: id,
                        entity: identities[&id.0],
                        tick: clock.0,
                    });
                }
                s.status_dirty |= before
                    != (
                        wanted
                            .get(&(s.player_id()))
                            .expect("admitted wanted state")
                            .heat,
                        wanted
                            .get(&(s.player_id()))
                            .expect("admitted wanted state")
                            .pursuit,
                    );
            }
        }
        sessions.peers.insert(peer, s);
    }
}

// Borrowed query views scoped to one perception invocation; no persistent snapshots.
struct Witnesses<'a, 'b> {
    cars: &'a BTreeMap<u32, &'b Vehicle>,
    units: &'a BTreeMap<u32, &'b mut Unit>,
    officers: &'a BTreeMap<u32, &'b Officer>,
    collision: &'a CollisionWorld,
}
fn ingest_crimes(
    player: u32,
    tick: u32,
    crimes: &BTreeMap<(u32, CrimeId), CrimeReport>,
    wanted: &mut Wanted,
) {
    for (&(actor, cause), &report) in crimes {
        if actor == player {
            crate::systems::police::state::observe(&mut *wanted, cause, tick, report);
        }
    }
}
impl Witnesses<'_, '_> {
    fn sees(&self, phase: SessionPhase, tick: u32, p: [f32; 3]) -> bool {
        let Self {
            cars,
            units,
            officers,
            collision,
        } = self;
        phase == SessionPhase::Playing
            && tick.is_multiple_of(6)
            && (officers.values().any(|officer| {
                !officer.mounted
                    && officer.state.health.0 > 0
                    && sight(
                        collision,
                        officer.state.motor.position.0.map(|v| v.0),
                        p,
                        45.0,
                    )
            }) || units.keys().any(|unit| {
                let car = &cars[unit];
                car.state.health.0 > 0
                    && sight(collision, car.state.motor.position.0.map(|v| v.0), p, 70.0)
            }))
    }
    fn can_arrest(
        &self,
        player_id: u32,
        phase: SessionPhase,
        player: &CharacterState,
        p: [f32; 3],
    ) -> bool {
        let Self {
            cars,
            units,
            officers,
            collision,
        } = self;
        let speed = player
            .vehicle_id
            .map(|id| cars[&id.0].state.motor.speed.0.abs())
            .unwrap_or_else(|| {
                player
                    .motor
                    .velocity
                    .0
                    .iter()
                    .map(|v| v.0 * v.0)
                    .sum::<f32>()
                    .sqrt()
            });
        phase == SessionPhase::Playing
            && speed < 0.5
            && (officers.values().any(|officer| {
                units
                    .get(&officer.unit)
                    .is_some_and(|unit| unit.target == Some(player_id))
                    && !officer.mounted
                    && officer.state.health.0 > 0
                    && !officer
                        .state
                        .equipment
                        .as_ref()
                        .is_some_and(|weapon| weapon.aiming)
                    && sight(
                        collision,
                        officer.state.motor.position.0.map(|v| v.0),
                        p,
                        3.0,
                    )
            }) || units.iter().any(|(unit, u)| {
                let car = &cars[unit];
                let front = sim_core::anchor(
                    &car.state.motor,
                    [0.0, 0.0, -car.template.dimensions[2] / 2.0],
                );
                u.target == Some(player_id)
                    && car.state.health.0 > 0
                    && distance(front, p) <= 3.0
                    && sight(collision, front, p, 3.0)
            }))
    }
}

// Observation refresh precedes pursuit/arrest timer advancement, as in the tick contract.
fn advance_pursuit(
    wanted: &mut Wanted,
    tick: u32,
    player: &CharacterState,
    witnesses: &Witnesses,
    seen: Option<[f32; 3]>,
    arrest: bool,
) -> bool {
    if let Some(position) = seen {
        wanted.vehicle_sighting = player.vehicle_id.map(|id| {
            let motor = &witnesses.cars[&id.0].state.motor;
            VehicleSighting {
                id: id.0,
                tick,
                position,
                yaw: motor.yaw.0,
                speed: motor.speed.0,
            }
        });
    }
    crate::systems::police::state::advance(&mut *wanted, tick, seen, arrest)
}
