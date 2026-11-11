//! Explicit planning checkpoints. Observations are tick-local; actor state stays in components.
use crate::{
    components::{Pedestrian, Player, Vehicle},
    components::{PlayerMission, PoolMembership, VehicleHome},
    ecs::runtime::Scratch,
    ecs::state::{Clock, Collision, Content},
    ecs::{EntityIndex, GameId, TickOutcome, TickPhase},
    performance::WorldTimings,
    sessions::{SessionPhase, Sessions},
    systems::{
        movement::MovementFrame,
        police::Wanted,
        population::{Observer, VehiclePool},
    },
};
use bevy_ecs::prelude::*;
use protocol::*;
use std::{collections::BTreeMap, time::Instant};
#[derive(Resource, Default)]
pub(crate) struct Planning {
    pub tick: u32,
    pub observers: Vec<Observer>,
    started: Option<Instant>,
}
pub(crate) fn begin(
    mut planning: ResMut<Planning>,
    clock: Res<Clock>,
    mut sessions: ResMut<Sessions>,
    mut players: Query<(&GameId, &Player, &mut Wanted, &PlayerMission)>,
    index: Res<EntityIndex>,
    mut scratch: ResMut<Scratch>,
    mut outcome: ResMut<TickOutcome>,
) {
    if !scratch.enter(TickPhase::Planning, &mut outcome) {
        return;
    }
    planning.started = Some(Instant::now());
    planning.tick = clock.0 + 1;
    let owners: BTreeMap<_, _> = sessions
        .peers
        .iter()
        .filter(|(_, s)| s.phase == SessionPhase::Playing)
        .filter_map(|(p, s)| s.player.map(|id| (id.0, *p)))
        .collect();
    let mut inputs = BTreeMap::new();
    for (&id, &peer) in &owners {
        let Some(entity) = index.0.get(&id) else {
            outcome.fail("missing planning actor".into());
            return;
        };
        let Ok((_, player, mut wanted, mission)) = players.get_mut(*entity) else {
            outcome.fail("missing planning components".into());
            return;
        };
        let session = sessions.peers.get_mut(&peer).unwrap();
        let input = session
            .inputs
            .remove(&planning.tick)
            .unwrap_or_else(|| sim_core::neutral_input(planning.tick, player.state.motor.yaw));
        wanted.surrendering = player.state.mode == CharacterMode::OnFoot
            && input.buttons.0 & 4 != 0
            && input.buttons.0 & 10 == 0
            && input.move_x.0 == 0
            && input.move_z.0 == 0;
        let retiring = mission.0.as_ref().is_some_and(|m| {
            m.phase == crate::systems::missions::Phase::Failed
                && m.reserved_vehicle == player.state.vehicle_id.map(|v| v.0)
                && m.reserved_vehicle.is_some()
        });
        let input = if wanted.arrest_pending || retiring {
            let mut v = sim_core::neutral_input(planning.tick, player.state.motor.yaw);
            v.buttons = Buttons(4);
            v
        } else {
            input
        };
        session.inputs.retain(|tick, _| *tick > planning.tick);
        inputs.insert(player.state.vehicle_id.map_or(id, |v| v.0), input);
    }
    planning.observers = sessions
        .peers
        .values()
        .filter(|s| s.phase == SessionPhase::Playing)
        .filter_map(|s| s.player)
        .filter_map(|id| index.0.get(&id.0))
        .filter_map(|e| players.get(*e).ok())
        .map(|(_, p, _, _)| Observer {
            position: p.state.motor.position.0.map(|v| v.0),
            yaw: p.state.motor.yaw.0,
        })
        .collect();
    scratch.frame = Some(MovementFrame {
        next: planning.tick,
        failed: Vec::new(),
        owners,
        actor_inputs: inputs,
        previous_cars: BTreeMap::new(),
        previous_peds: BTreeMap::new(),
    });
}
pub(crate) fn prepare_geometry(
    cars: Query<(
        Entity,
        &GameId,
        &Vehicle,
        Option<&PoolMembership>,
        Option<&VehicleHome>,
    )>,
    peds: Query<(&GameId, &Pedestrian)>,
    content: Res<Content>,
    mut collision: NonSendMut<Collision>,
    mut commands: Commands,
    mut scratch: ResMut<Scratch>,
    mut outcome: ResMut<TickOutcome>,
) {
    let mut cars: Vec<_> = cars.iter().collect();
    cars.sort_by_key(|(_, id, _, _, _)| id.0.0);
    if let Err(e) = collision.0.set_vehicle_proxies(
        &cars
            .iter()
            .map(|(_, _, c, _, _)| crate::systems::vehicle::proxy(c))
            .collect::<Vec<_>>(),
    ) {
        outcome.fail(e);
        return;
    }
    for &(entity, _, car, pool, home) in &cars {
        if home.is_some() {
            continue;
        }
        let pos = car.state.motor.position.0.map(|v| v.0);
        let home = if pool.is_some_and(|p| p.0 == VehiclePool::Traffic) {
            content
                .0
                .manifest
                .parking
                .iter()
                .filter(|p| !p.mission_reserved && p.template_id == car.template.id)
                .min_by(|a, b| {
                    let d = |p: [f64; 3]| (p[0] as f32 - pos[0]).hypot(p[2] as f32 - pos[2]);
                    d(a.position).total_cmp(&d(b.position))
                })
                .map(|p| (p.position.map(|v| v as f32), p.yaw as f32))
                .unwrap_or((pos, car.state.motor.yaw.0))
        } else {
            (pos, car.state.motor.yaw.0)
        };
        commands.entity(entity).insert(VehicleHome(home));
    }
    if let Some(frame) = scratch.frame.as_mut() {
        frame.previous_cars = cars
            .iter()
            .map(|(_, id, c, _, _)| (id.0.0, c.state.motor.clone()))
            .collect();
        frame.previous_peds = peds
            .iter()
            .map(|(id, p)| (id.0.0, p.state.motor.position.0.map(|v| v.0)))
            .collect();
    }
}
pub(crate) fn finish(mut planning: ResMut<Planning>, mut timings: ResMut<WorldTimings>) {
    if let Some(start) = planning.started.take() {
        timings.planning.observe(start.elapsed());
    }
}
pub(crate) fn spawn(commands: &mut Commands, id: u32, bundle: impl Bundle) {
    commands.queue(move |world: &mut World| {
        assert!(
            !world.resource::<EntityIndex>().0.contains_key(&id),
            "duplicate actor ID"
        );
        let entity = world.spawn((GameId(EntityId(id)), bundle)).id();
        world.resource_mut::<EntityIndex>().0.insert(id, entity);
    });
}
pub(crate) fn despawn(commands: &mut Commands, id: u32, entity: Entity) {
    commands.queue(move |world: &mut World| {
        crate::systems::lifecycle::despawn_indexed(world, entity, EntityId(id))
    });
}

/// A director's local command staging: existing components remain borrowed in
/// place; only newly created actors are owned until deferred spawn is applied.
pub(crate) enum Pending<'a, T> {
    Live(&'a mut T),
    Spawn(T),
}
impl<T> std::ops::Deref for Pending<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        match self {
            Self::Live(v) => v,
            Self::Spawn(v) => v,
        }
    }
}
impl<T> std::ops::DerefMut for Pending<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        match self {
            Self::Live(v) => v,
            Self::Spawn(v) => v,
        }
    }
}
