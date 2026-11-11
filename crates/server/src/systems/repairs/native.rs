//! Native repair validation and durable transaction progression.
use super::*;
use crate::{
    components::PlayerMission,
    ecs::state::{Clock, Content},
    ecs::{GameId, TickOutcome},
};
use bevy_ecs::prelude::*;
use std::result::Result;
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct RepairActors<'w, 's> {
    players: Query<
        'w,
        's,
        (
            &'static GameId,
            &'static PlayerComponent,
            &'static WantedComponent,
            &'static PlayerMission,
        ),
    >,
    cars: Query<'w, 's, (&'static GameId, &'static mut VehicleComponent)>,
}
fn eligible(
    s: &Session,
    car: u32,
    zone: &str,
    epoch: u32,
    players: &BTreeMap<u32, &PlayerComponent>,
    cars: &BTreeMap<u32, &mut VehicleComponent>,
    wanted: &BTreeMap<u32, &WantedComponent>,
    manifest: &content::WorldManifest,
) -> bool {
    let Some(id) = s.player else {
        return false;
    };
    let Some(player) = players.get(&id.0) else {
        return false;
    };
    let Some(v) = cars.get(&car) else {
        return false;
    };
    s.phase == SessionPhase::Playing
        && s.epoch == epoch
        && player.state.health.0 > 0
        && player.state.mode == CharacterMode::Driving
        && player.state.vehicle_id == Some(EntityId(car))
        && v.state.driver_id == Some(id)
        && v.state.health.0 > 0
        && v.state.health.0 < v.template.health
        && !v.state.disabled
        && wanted
            .get(&(s.player_id()))
            .expect("admitted wanted state")
            .heat
            == 0
        && wanted
            .get(&(s.player_id()))
            .expect("admitted wanted state")
            .pursuit
            == Pursuit::Clear
        && v.state.motor.speed.0.abs() < 0.5
        && manifest.zones.iter().any(|z| {
            z.id == zone
                && z.kind == content::ZoneKind::Repair
                && distance(
                    v.state.motor.position.0.map(|v| v.0),
                    z.center.map(|v| v as f32),
                ) <= z.radius as f32
        })
}
pub(crate) fn request(
    repairs: &mut RepairSystem,
    s: &mut Session,
    request: &Repair,
    request_id: u32,
    _tick: u32,
    manifest: &content::WorldManifest,
    actors: &mut RepairActors,
) -> ActionCode {
    let players: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(id, p, _, _)| (id.0.0, p))
        .collect();
    let wanted: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(id, _, w, _)| (id.0.0, w))
        .collect();
    let cars: BTreeMap<_, _> = actors
        .cars
        .iter_mut()
        .map(|(id, c)| (id.0.0, c.into_inner()))
        .collect();
    let missions: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(id, _, _, m)| (id.0.0, &m.0))
        .collect();

    let Some(profile) = &s.profile else {
        return ActionCode::NotReady;
    };
    let profile = profile.profile_id.0.clone();
    if repairs.pending.len() >= 32 {
        return ActionCode::Unavailable;
    }
    if repairs.pending.contains_key(&profile)
        || missions
            .get(&(s.player_id()))
            .expect("admitted mission state")
            .as_ref()
            .is_some_and(|m| m.phase == crate::systems::missions::Phase::RewardPending)
    {
        return ActionCode::AlreadyActive;
    }
    let id = s.player.expect("repair player");
    let Some(car) = players[&id.0].state.vehicle_id.map(|id| id.0) else {
        return ActionCode::InvalidTarget;
    };
    if repairs.pending.values().any(|r| r.car == car) {
        return ActionCode::AlreadyActive;
    }
    let Some(zone) = manifest
        .zones
        .iter()
        .find(|z| z.id == request.zone_id.0 && z.kind == content::ZoneKind::Repair)
    else {
        return ActionCode::InvalidTarget;
    };
    if distance(
        cars[&car].state.motor.position.0.map(|v| v.0),
        zone.center.map(|v| v as f32),
    ) > zone.radius as f32
    {
        return ActionCode::OutOfRange;
    }
    if cars[&car].state.motor.speed.0.abs() >= 0.5 {
        return ActionCode::TooFast;
    }
    if wanted
        .get(&(s.player_id()))
        .expect("admitted wanted state")
        .heat
        > 0
        || wanted
            .get(&(s.player_id()))
            .expect("admitted wanted state")
            .pursuit
            != Pursuit::Clear
    {
        return ActionCode::PursuitActive;
    }
    if !eligible(
        s,
        car,
        &request.zone_id.0,
        s.epoch,
        &players,
        &cars,
        &wanted,
        manifest,
    ) {
        return ActionCode::Unavailable;
    }
    if s.profile.as_ref().expect("repair profile").credits.0 < 50 {
        return ActionCode::InsufficientFunds;
    }
    let sid = s.id.clone().expect("repair session");
    // Original player/session/epoch identity is rechecked on worker completion.
    let pending = PendingRepair {
        session: sid.clone(),
        player: id.0,
        epoch: s.epoch,
        car,
        zone: request.zone_id.0.clone(),
        event: format!("repair:{sid}:{request_id}"),
        phase: RepairPhase::Dwell,
        dwell: 0,
        invalidated: false,
        queued: false,
        code: ActionCode::Ok,
    };
    s.repair_view = Some(pending.view());
    s.repair_clear = None;
    s.status_dirty = true;
    repairs.pending.insert(profile, pending);
    ActionCode::Ok
}
pub(crate) fn update_actors(
    repairs: &mut RepairSystem,
    profiles: &mut Profiles,
    tick: u32,
    manifest: &content::WorldManifest,
    sessions: &mut Sessions,
    actors: &mut RepairActors,
) -> Result<(), String> {
    let players: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(id, p, _, _)| (id.0.0, p))
        .collect();
    let wanted: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(id, _, w, _)| (id.0.0, w))
        .collect();
    let mut cars: BTreeMap<_, _> = actors
        .cars
        .iter_mut()
        .map(|(id, c)| (id.0.0, c.into_inner()))
        .collect();

    let mut results = super::transactions::collect_results(profiles, tick);
    super::transactions::expire_status(sessions, tick);
    for profile in repairs.pending.keys().cloned().collect::<Vec<_>>() {
        let mut r = repairs.pending.remove(&profile).expect("pending repair");
        let eligible = sessions
            .peers
            .values()
            .find(|s| s.id.as_deref() == Some(&r.session) && s.player == Some(EntityId(r.player)))
            .is_some_and(|s| {
                eligible(
                    s, r.car, &r.zone, r.epoch, &players, &cars, &wanted, manifest,
                )
            });
        super::transactions::advance(
            &mut r,
            &profile,
            eligible,
            &mut results,
            profiles,
            &mut cars,
        );
        let pending = matches!(
            r.phase,
            RepairPhase::Dwell | RepairPhase::DebitPending | RepairPhase::RefundPending
        );
        super::transactions::submit(&mut r, &profile, profiles, tick);
        super::transactions::publish(&r, &profile, pending, profiles, sessions, tick);
        if pending {
            repairs.pending.insert(profile, r);
        }
    }
    Ok(())
}
pub(crate) fn update(
    mut repairs: ResMut<RepairSystem>,
    mut actors: RepairActors,
    mut io: NonSendMut<crate::ecs::runtime::TickIo>,
    clock: Res<Clock>,
    content: Res<Content>,
    mut sessions: ResMut<Sessions>,
    mut outcome: ResMut<TickOutcome>,
) {
    if let Err(e) = update_actors(
        &mut repairs,
        &mut io.profiles,
        clock.0,
        &content.0.manifest,
        &mut sessions,
        &mut actors,
    ) {
        outcome.fail(e);
    }
}
