//! Stateless coherence and context admission, separate from authoritative gameplay.
use crate::{Result, invalid, limits::*, types::*};
use std::collections::BTreeSet;

pub fn validate_client(envelope: &ClientEnvelope) -> Result<()> {
    scope(
        envelope.world_id.as_ref(),
        envelope.session_id.as_ref(),
        matches!(envelope.message, ClientMessage::ClientHello(_)),
        matches!(envelope.message, ClientMessage::Leave(_)),
    )?;
    if let ClientMessage::InputBatch(batch) = &envelope.message
        && (batch.frames.is_empty()
            || batch.frames.len() > MAX_INPUT_FRAMES
            || batch
                .frames
                .windows(2)
                .any(|pair| pair[0].tick >= pair[1].tick))
    {
        return Err(invalid(
            "input frames must contain 1..3 unique ascending ticks",
        ));
    }
    Ok(())
}

pub fn validate_server(envelope: &ServerEnvelope, delivery: Delivery) -> Result<()> {
    scope(
        envelope.world_id.as_ref(),
        envelope.session_id.as_ref(),
        matches!(
            envelope.message,
            ServerMessage::ServerHello(_) | ServerMessage::Reject(_)
        ),
        matches!(envelope.message, ServerMessage::Disconnect(_)),
    )?;
    if delivery == Delivery::Unreliable
        && !matches!(
            envelope.message,
            ServerMessage::Snapshot(_)
                | ServerMessage::Pong(_)
                | ServerMessage::OwnerState(_)
                | ServerMessage::WorldEvent(WorldEvent::Impact(_))
        )
    {
        return Err(invalid("message requires reliable delivery"));
    }
    match &envelope.message {
        ServerMessage::EntityShow(show) => {
            state(&show.state)?;
            let valid = matches!(
                (&show.descriptor.entity_kind, &show.state),
                (
                    EntityKind::Player | EntityKind::Pedestrian,
                    EntityState::Character(_)
                ) | (EntityKind::Vehicle, EntityState::Vehicle(_))
                    | (EntityKind::Police, _)
            );
            if !valid {
                return Err(invalid("descriptor/state kind mismatch"));
            }
        }
        ServerMessage::Snapshot(snapshot) => {
            if snapshot.records.is_empty() || snapshot.records.len() > MAX_VISIBLE_ENTITIES {
                return Err(invalid("snapshot record count"));
            }
            let mut ids = BTreeSet::new();
            for record in &snapshot.records {
                if !ids.insert(record.entity_id.0) {
                    return Err(invalid("duplicate snapshot entity"));
                }
                state(&record.state)?;
            }
        }
        ServerMessage::OwnerState(value) | ServerMessage::ControlChanged(value) => owner(value)?,
        ServerMessage::StartPlay(value) => owner(&value.owner)?,
        ServerMessage::ActionResult(value) if value.ok != (value.code == ActionCode::Ok) => {
            return Err(invalid("action result code/ok mismatch"));
        }
        ServerMessage::PlayerStatus(value) => {
            if value
                .repair
                .as_ref()
                .is_some_and(|r| r.price.0 != 50 || r.dwell_ticks > 60)
            {
                return Err(invalid("invalid repair price or dwell"));
            }
            if (value.heat.0 == 0) != (value.pursuit == Pursuit::Clear) {
                return Err(invalid("heat/pursuit mismatch"));
            }
        }
        _ => {}
    }
    Ok(())
}

fn scope(
    world: Option<&OpaqueId>,
    session: Option<&OpaqueId>,
    pre_auth: bool,
    either: bool,
) -> Result<()> {
    if world.is_none() || (!either && pre_auth != session.is_none()) {
        return Err(invalid("invalid envelope scope"));
    }
    Ok(())
}

fn character(value: &CharacterState) -> Result<()> {
    if (value.mode == CharacterMode::Driving) != value.vehicle_id.is_some() {
        return Err(invalid("character mode/vehicle mismatch"));
    }
    Ok(())
}

fn state(value: &EntityState) -> Result<()> {
    match value {
        EntityState::Character(value) => character(value),
        EntityState::Vehicle(value) if value.disabled && value.driver_id.is_some() => {
            Err(invalid("disabled vehicle has a driver"))
        }
        _ => Ok(()),
    }
}

fn owner(value: &OwnerState) -> Result<()> {
    if value.server_tick != value.consumed_through_tick {
        return Err(invalid("owner input checkpoint mismatch"));
    }
    character(&value.player_state)?;
    state(&value.state)?;
    match &value.state {
        EntityState::Character(character)
            if value.controlled_entity_id == value.player_id
                && value.player_state == *character
                && character.mode != CharacterMode::Driving =>
        {
            Ok(())
        }
        EntityState::Vehicle(vehicle)
            if value.controlled_entity_id != value.player_id
                && value.player_state.mode == CharacterMode::Driving
                && value.player_state.vehicle_id == Some(value.controlled_entity_id)
                && vehicle.driver_id == Some(value.player_id) =>
        {
            Ok(())
        }
        _ => Err(invalid("owner control/occupancy mismatch")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[ts(rename_all = "snake_case")]
pub enum Phase {
    AwaitHello,
    Loading,
    Playing,
    Closing,
}

#[derive(ts_rs::TS)]
pub struct ClientContext<'a> {
    pub world_id: &'a OpaqueId,
    pub session_id: Option<&'a OpaqueId>,
    pub phase: Phase,
    pub control_epoch: Option<ControlEpoch>,
    pub next_tick: Tick,
    pub baseline_id: Option<BaselineId>,
}

/// Must be called again at application time; does not check range/ownership,
/// cooldowns, tickets or money, which require the authoritative world in T07+.
pub fn validate_client_context(value: &ClientEnvelope, context: &ClientContext<'_>) -> Result<()> {
    validate_client(value)?;
    if value.world_id.as_ref() != Some(context.world_id)
        || value.session_id.as_ref() != context.session_id
        || context.phase == Phase::Closing
    {
        return Err(invalid("stale world/session or closed phase"));
    }
    let allowed = match &value.message {
        ClientMessage::ClientHello(_) => context.phase == Phase::AwaitHello,
        ClientMessage::ClientReady(ready) => {
            context.phase == Phase::Loading && Some(ready.baseline_id) == context.baseline_id
        }
        ClientMessage::InputBatch(batch) => {
            context.phase == Phase::Playing
                && Some(batch.control_epoch) == context.control_epoch
                && batch.frames.iter().all(|frame| {
                    frame.tick >= context.next_tick
                        && frame.tick <= context.next_tick.saturating_add(8)
                })
        }
        ClientMessage::Action(action) => {
            context.phase == Phase::Playing && Some(action.control_epoch) == context.control_epoch
        }
        ClientMessage::ResyncRequest(_) => context.phase == Phase::Playing,
        ClientMessage::Ping(_) => matches!(context.phase, Phase::Loading | Phase::Playing),
        ClientMessage::Leave(_) => true,
    };
    if !allowed {
        return Err(invalid("message rejected by session context"));
    }
    Ok(())
}
