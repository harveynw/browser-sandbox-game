//! Ordered mission objectives, recovery retirement and reward requests.
use super::*;
pub(crate) fn progress(
    mut actors: JobActors,
    mut sessions: ResMut<Sessions>,
    clock: Res<Clock>,
    scratch: Res<crate::ecs::runtime::Scratch>,
    mut rewards: MessageWriter<RewardRequested>,
    mut outcome: ResMut<TickOutcome>,
) {
    let tick = clock.0;
    let Some(frame) = scratch.frame.as_ref() else {
        outcome.fail("mission movement frame missing".into());
        return;
    };
    let previous = &frame.previous_cars;
    let result = (|| -> Result<(), String> {
        // Pending guests have no player or mission state; admitted players also update during resync.
        let peers: Vec<_> = sessions
            .peers
            .iter()
            .filter_map(|(&peer, session)| session.player.map(|_| peer))
            .collect();
        for peer in peers {
            let mut s = sessions.peers.remove(&peer).expect("mission session");
            let mut changed = s.status_dirty;
            if let Some(mut m) = actors.mission_mut(s.player_id()).0.take() {
                let before = (m.phase, m.checkpoint_index);
                advance_objective(&actors, &s, &mut m, tick, previous)?;
                finish_recovery(&mut actors, peer, &mut s, &mut m, tick)?;
                request_reward(&actors, peer, &s, &m, tick, &mut rewards)?;
                let released = retire_failed(&mut actors, peer, &mut s, &mut m, tick)?;
                changed |= before != (m.phase, m.checkpoint_index);
                if released && m.clear_at.is_some_and(|end| tick >= end) {
                    changed = true;
                } else {
                    actors.mission_mut(s.player_id()).0 = Some(m);
                }
            }
            s.status_dirty = changed;
            sessions.peers.insert(peer, s);
        }
        Ok(())
    })();
    if let Err(e) = result {
        outcome.fail(e);
    }
}

fn advance_objective(
    actors: &JobActors,
    s: &Session,
    m: &mut Mission,
    tick: u32,
    previous: &BTreeMap<u32, VehicleMotor>,
) -> Result<(), String> {
    let id = s.player.ok_or("mission owner missing")?;
    let player = actors.player(id.0);
    let car = player.state.vehicle_id.map(|id| id.0);
    let motor = car
        .map(|id| sim_core::presentation_motor(&actors.car(id).state.motor))
        .unwrap_or_else(|| player.state.motor.clone());
    let alive = player.state.health.0 > 0
        && matches!(
            player.state.mode,
            CharacterMode::OnFoot | CharacterMode::Driving
        )
        && car.is_none_or(|id| actors.car(id).state.health.0 > 0)
        && m.reserved_vehicle
            .is_none_or(|id| actors.car_opt(id).is_some_and(|v| v.state.health.0 > 0));
    if m.phase == Phase::Active
        && let Objective::Recovery { garage } = m.objective
    {
        if car == m.reserved_vehicle {
            m.target = garage;
            if m.checkpoint_index != 2
                || distance(motor.position.0.map(|v| v.0), garage) > m.radius
                || motor
                    .velocity
                    .0
                    .iter()
                    .map(|v| v.0 * v.0)
                    .sum::<f32>()
                    .sqrt()
                    >= m.max_speed
            {
                m.checkpoint_index = 1;
            }
        } else {
            m.checkpoint_index = 0;
            if let Some(v) = m.reserved_vehicle.and_then(|id| actors.car_opt(id)) {
                m.target = v.state.motor.position.0.map(|v| v.0);
            }
        }
    }
    m.advance(
        tick,
        crate::systems::missions::Motion {
            alive,
            playing: s.phase == SessionPhase::Playing,
            position: motor.position.0.map(|v| v.0),
            speed: motor
                .velocity
                .0
                .iter()
                .map(|v| v.0 * v.0)
                .sum::<f32>()
                .sqrt(),
            vehicle: car,
            previous: car
                .and_then(|id| previous.get(&id))
                .map(|m| m.position.0.map(|v| v.0)),
        },
    );
    Ok(())
}
// Delivery cannot become reward-eligible until the driver can leave safely.
fn finish_recovery(
    actors: &mut JobActors,
    peer: crate::transport::PeerKey,
    s: &mut Session,
    m: &mut Mission,
    tick: u32,
) -> Result<(), String> {
    if m.phase == Phase::RewardPending
        && let Some(car) = m.reserved_vehicle
        && actors.reserved(car)
        && !actors.retire(peer, s, car, tick)?
    {
        m.phase = Phase::Active;
        m.checkpoint_index = 2;
        m.dwell = m.required_dwell;
    }
    Ok(())
}
fn request_reward(
    actors: &JobActors,
    peer: crate::transport::PeerKey,
    s: &Session,
    m: &Mission,
    tick: u32,
    rewards: &mut MessageWriter<RewardRequested>,
) -> Result<(), String> {
    let id = s.player.ok_or("mission owner missing")?;
    if m.phase == Phase::RewardPending {
        let profile = s.profile.as_ref().ok_or("mission profile missing")?;
        rewards.write(RewardRequested {
            peer,
            player: id,
            entity: actors.index.0[&id.0],
            session: s.id.clone().ok_or("mission session missing")?,
            profile: profile.profile_id.0.clone(),
            mission: m.id.clone(),
            amount: m.reward,
            tick,
        });
    }
    Ok(())
}
fn retire_failed(
    actors: &mut JobActors,
    peer: crate::transport::PeerKey,
    s: &mut Session,
    m: &mut Mission,
    tick: u32,
) -> Result<bool, String> {
    let mut released = true;
    if m.phase == Phase::Failed
        && let Some(car) = m.reserved_vehicle
    {
        released = actors.retire(peer, s, car, tick)?;
        m.checkpoint_index = if released { 0 } else { 2 };
    }
    Ok(released)
}
