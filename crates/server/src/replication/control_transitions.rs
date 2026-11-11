//! Native message consumer for control epochs and reliable replica refreshes.
use super::Replication;
use crate::{
    ecs::state::{Clock, WorldIdentity},
    ecs::{EntityIndex, GameId, TickOutcome},
    entities::{pedestrian::Pedestrian, player::Player, vehicle::Vehicle},
    gameplay_messages::ControlTransitionRequested,
    metadata::SecuredVehicle,
    sessions::Sessions,
    systems::police::{Officer, Unit},
};
use bevy_ecs::prelude::*;
use protocol::*;
use std::{collections::BTreeMap, result::Result};

#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct Replicas<'w, 's> {
    players: Query<'w, 's, (&'static GameId, &'static mut Player)>,
    vehicles: Query<
        'w,
        's,
        (
            &'static Vehicle,
            Option<&'static Unit>,
            Option<&'static SecuredVehicle>,
        ),
    >,
    pedestrians: Query<'w, 's, &'static Pedestrian>,
    officers: Query<'w, 's, &'static Officer>,
    index: Res<'w, EntityIndex>,
}
impl Replicas<'_, '_> {
    fn snapshot(&self, id: u32) -> Result<(EntityDescriptor, EntityState), String> {
        let entity = *self.index.0.get(&id).ok_or("missing changed entity")?;
        if let Ok((_, player)) = self.players.get(entity) {
            return Ok((
                EntityDescriptor {
                    tshirt_colour: Some(player.tshirt_colour),
                    id: player.id,
                    entity_kind: EntityKind::Player,
                    template_id: TemplateId("player_greybox".into()),
                    display_name: Some(player.display_name.clone()),
                },
                EntityState::Character(player.public_state()),
            ));
        }
        if let Ok((vehicle, police, secured)) = self.vehicles.get(entity) {
            return Ok((
                EntityDescriptor {
                    tshirt_colour: None,
                    id: vehicle.id,
                    entity_kind: if police.is_some() {
                        EntityKind::Police
                    } else {
                        EntityKind::Vehicle
                    },
                    template_id: TemplateId(vehicle.template.id.clone()),
                    display_name: if secured.is_some() {
                        Some(DisplayName(format!("Secured {}", vehicle.template.id)))
                    } else if police.is_some() {
                        Some(DisplayName("PoliceSystem patrol".into()))
                    } else {
                        None
                    },
                },
                EntityState::Vehicle(vehicle.state.clone()),
            ));
        }
        if let Ok(officer) = self.officers.get(entity)
            && !officer.mounted
        {
            return Ok((
                EntityDescriptor {
                    id: EntityId(id),
                    entity_kind: EntityKind::Police,
                    template_id: TemplateId("player_greybox".into()),
                    display_name: None,
                    tshirt_colour: Some(TshirtColour(0x263f68)),
                },
                EntityState::Character(officer.state.clone()),
            ));
        }
        if let Ok(pedestrian) = self.pedestrians.get(entity) {
            return Ok((
                EntityDescriptor {
                    tshirt_colour: None,
                    id: pedestrian.id,
                    entity_kind: EntityKind::Pedestrian,
                    template_id: TemplateId("pedestrian_greybox".into()),
                    display_name: None,
                },
                EntityState::Character(pedestrian.state.clone()),
            ));
        }
        Err("changed entity has no public replica".into())
    }
}

pub(crate) fn apply_control_transitions(
    mut messages: MessageReader<ControlTransitionRequested>,
    mut replicas: Replicas,
    mut sessions: ResMut<Sessions>,
    clock: Res<Clock>,
    identity: Res<WorldIdentity>,
    mut outcome: ResMut<TickOutcome>,
) {
    for message in messages.read() {
        if message.tick != clock.0
            || replicas.index.0.get(&message.player.0) != Some(&message.player_entity)
            || !replicas
                .players
                .get(message.player_entity)
                .is_ok_and(|(id, _)| id.0 == message.player)
            || !sessions
                .peers
                .get(&message.peer)
                .is_some_and(|s| s.player == Some(message.player))
        {
            continue;
        }
        let result = (|| -> Result<(), String> {
            let epoch = sessions.peers[&message.peer]
                .epoch
                .checked_add(1)
                .ok_or("epoch exhausted")?;
            let player = replicas
                .players
                .get(message.player_entity)
                .map_err(|_| "missing player")?
                .1;
            let player_state = player.public_state();
            let controlled_entity_id = player.state.vehicle_id.unwrap_or(message.player);
            let state = if controlled_entity_id == message.player {
                EntityState::Character(player_state.clone())
            } else {
                replicas.snapshot(controlled_entity_id.0)?.1
            };
            // Only requested replicas are materialized; there is no whole-world snapshot or manager bridge.
            let snapshots: BTreeMap<_, _> = message
                .changed
                .iter()
                .map(|id| replicas.snapshot(*id).map(|v| (*id, v)))
                .collect::<Result<_, _>>()?;
            let targets: Vec<_> = std::iter::once(message.peer)
                .chain(
                    sessions
                        .peers
                        .keys()
                        .copied()
                        .filter(|peer| *peer != message.peer),
                )
                .collect();
            // Preflight counters before publishing any part of this transition.
            for peer in &targets {
                let session = &sessions.peers[peer];
                let count = message
                    .changed
                    .iter()
                    .filter(|id| session.visible.contains_key(id))
                    .count() as u32;
                session
                    .visibility_counter
                    .checked_add(count)
                    .ok_or("visibility exhausted")?;
            }
            let (_, mut player) = replicas
                .players
                .get_mut(message.player_entity)
                .map_err(|_| "missing player")?;
            player.weapon.trigger = None;
            let session = sessions
                .peers
                .get_mut(&message.peer)
                .expect("validated session");
            session.epoch = epoch;
            session.inputs.clear();
            let owner_session_id = session.id.clone();
            let mut outgoing = Vec::new();
            for peer in targets {
                let session = sessions.peers.get_mut(&peer).expect("known recipient");
                for id in &message.changed {
                    if !session.visible.contains_key(id) {
                        continue;
                    }
                    session.visibility_counter += 1;
                    session.visible.insert(*id, session.visibility_counter);
                    let (descriptor, state) = snapshots[id].clone();
                    outgoing.push((
                        peer,
                        session.id.clone(),
                        ServerMessage::EntityShow(EntityShow {
                            baseline_id: None,
                            visibility_id: VisibilityId(session.visibility_counter),
                            descriptor,
                            state,
                            state_tick: clock.0,
                        }),
                    ));
                }
            }
            for (peer, sid, payload) in outgoing {
                Replication::queue(
                    peer,
                    sid.as_deref(),
                    Delivery::Reliable,
                    payload,
                    &identity.0,
                    &mut sessions,
                );
            }
            Replication::queue(
                message.peer,
                owner_session_id.as_deref(),
                Delivery::Reliable,
                ServerMessage::ControlChanged(OwnerState {
                    server_tick: clock.0,
                    consumed_through_tick: clock.0,
                    control_epoch: ControlEpoch(epoch),
                    player_id: message.player,
                    controlled_entity_id,
                    state,
                    player_state,
                }),
                &identity.0,
                &mut sessions,
            );
            Ok(())
        })();
        if let Err(error) = result {
            outcome.fail(error);
            break;
        }
    }
}

/// Each schedule checkpoint has its own reader; retire requests after delivery
/// so the later respawn checkpoint cannot replay a movement/crash transition.
pub(crate) fn retire_control_transitions(
    mut messages: ResMut<Messages<ControlTransitionRequested>>,
) {
    messages.clear();
}
