//! Replication reads authoritative components through typed queries. The ordered
//! view below borrows components for one call; it is never retained as a resource.
use super::*;
use crate::{
    components::{PlayerMission, SecuredVehicle},
    ecs::state::{Clock, WorldIdentity},
    ecs::{GameId, TickOutcome},
};
use bevy_ecs::{prelude::*, system::SystemParam};
use std::result::Result;
#[derive(SystemParam)]
pub(crate) struct OwnerActors<'w, 's> {
    players: Query<'w, 's, (&'static GameId, &'static PlayerComponent)>,
    cars: Query<'w, 's, (&'static GameId, &'static VehicleComponent)>,
}
#[derive(SystemParam)]
pub(crate) struct PublicActors<'w, 's> {
    owners: OwnerActors<'w, 's>,
    units: Query<'w, 's, &'static GameId, With<PoliceUnitComponent>>,
    officers: Query<'w, 's, (&'static GameId, &'static OfficerComponent)>,
    peds: Query<'w, 's, (&'static GameId, &'static PedestrianComponent)>,
    secured: Query<'w, 's, &'static GameId, With<SecuredVehicle>>,
}
#[derive(SystemParam)]
pub(crate) struct StatusActors<'w, 's> {
    players: Query<'w, 's, (&'static GameId, &'static PlayerComponent)>,
    wanted: Query<'w, 's, (&'static GameId, &'static WantedComponent)>,
    missions: Query<'w, 's, (&'static GameId, &'static PlayerMission)>,
}
#[derive(SystemParam)]
pub(crate) struct Actors<'w, 's> {
    public: PublicActors<'w, 's>,
    wanted: Query<'w, 's, (&'static GameId, &'static WantedComponent)>,
    missions: Query<'w, 's, (&'static GameId, &'static PlayerMission)>,
}
#[derive(Default)]
pub(crate) struct View<'a> {
    tick: u32,
    id: &'a str,
    players: BTreeMap<u32, &'a PlayerComponent>,
    cars: BTreeMap<u32, &'a VehicleComponent>,
    units: BTreeMap<u32, ()>,
    officers: BTreeMap<u32, &'a OfficerComponent>,
    peds: BTreeMap<u32, &'a PedestrianComponent>,
    wanted: BTreeMap<u32, &'a WantedComponent>,
    missions: BTreeMap<u32, &'a PlayerMission>,
    secured: BTreeMap<u32, ()>,
}
impl OwnerActors<'_, '_> {
    pub(super) fn view<'a>(&'a self, tick: u32, id: &'a str) -> View<'a> {
        View {
            tick,
            id,
            players: self.players.iter().map(|(id, p)| (id.0.0, p)).collect(),
            cars: self.cars.iter().map(|(id, c)| (id.0.0, c)).collect(),
            ..Default::default()
        }
    }
}
impl PublicActors<'_, '_> {
    pub(super) fn view<'a>(&'a self, tick: u32, id: &'a str) -> View<'a> {
        View {
            units: self.units.iter().map(|id| (id.0.0, ())).collect(),
            officers: self.officers.iter().map(|(id, v)| (id.0.0, v)).collect(),
            peds: self.peds.iter().map(|(id, v)| (id.0.0, v)).collect(),
            secured: self.secured.iter().map(|id| (id.0.0, ())).collect(),
            ..self.owners.view(tick, id)
        }
    }
}
impl StatusActors<'_, '_> {
    pub(super) fn view<'a>(&'a self, tick: u32, id: &'a str) -> View<'a> {
        View {
            tick,
            id,
            players: self.players.iter().map(|(id, p)| (id.0.0, p)).collect(),
            wanted: self.wanted.iter().map(|(id, v)| (id.0.0, v)).collect(),
            missions: self.missions.iter().map(|(id, v)| (id.0.0, v)).collect(),
            ..Default::default()
        }
    }
}
impl Actors<'_, '_> {
    pub(crate) fn player(&self, id: Option<EntityId>) -> Option<&PlayerComponent> {
        let id = id?;
        self.public
            .owners
            .players
            .iter()
            .find(|(actual, _)| actual.0 == id)
            .map(|(_, p)| p)
    }
    pub(crate) fn view<'a>(&'a self, tick: u32, id: &'a str) -> View<'a> {
        View {
            wanted: self.wanted.iter().map(|(id, v)| (id.0.0, v)).collect(),
            missions: self.missions.iter().map(|(id, v)| (id.0.0, v)).collect(),
            ..self.public.view(tick, id)
        }
    }
}
pub(super) type PeerPackets = BTreeMap<PeerKey, Vec<(Delivery, ServerEnvelope)>>;

pub(crate) struct Projection;
impl Projection {
    fn send(
        peer: PeerKey,
        sid: Option<&str>,
        delivery: Delivery,
        message: ServerMessage,
        world: &View,
        sessions: &mut Sessions,
    ) {
        Replication::queue(peer, sid, delivery, message, world.id, sessions);
    }
    fn reliable(
        peer: PeerKey,
        sid: Option<&str>,
        message: ServerMessage,
        world: &View,
        sessions: &mut Sessions,
    ) {
        Self::send(peer, sid, Delivery::Reliable, message, world, sessions);
    }
    pub(crate) fn owner(session: &Session, world: &View) -> Result<OwnerState, String> {
        let id = session.player.ok_or("missing owner")?;
        let player = world.players.get(&id.0).ok_or("missing player")?;
        Ok(OwnerState {
            server_tick: world.tick,
            consumed_through_tick: world.tick,
            control_epoch: ControlEpoch(session.epoch),
            player_id: id,
            controlled_entity_id: player.state.vehicle_id.unwrap_or(id),
            state: player
                .state
                .vehicle_id
                .map(|car| EntityState::Vehicle(world.cars[&car.0].state.clone()))
                .unwrap_or_else(|| {
                    EntityState::Character(crate::replication::projection::public_state(player))
                }),
            player_state: crate::replication::projection::public_state(player),
        })
    }
    pub(crate) fn entities(world: &View) -> BTreeMap<u32, (EntityDescriptor, EntityState)> {
        world
            .players
            .iter()
            .map(|(&id, p)| {
                (
                    id,
                    (
                        EntityDescriptor {
                            tshirt_colour: Some(p.tshirt_colour),
                            id: p.id,
                            entity_kind: EntityKind::Player,
                            template_id: TemplateId("player_greybox".into()),
                            display_name: Some(p.display_name.clone()),
                        },
                        EntityState::Character(crate::replication::projection::public_state(p)),
                    ),
                )
            })
            .chain(world.cars.iter().map(|(&id, v)| {
                (
                    id,
                    (
                        EntityDescriptor {
                            tshirt_colour: None,
                            id: v.id,
                            entity_kind: if world.units.contains_key(&id) {
                                EntityKind::Police
                            } else {
                                EntityKind::Vehicle
                            },
                            template_id: TemplateId(v.template.id.clone()),
                            display_name: if world.secured.contains_key(&id) {
                                Some(DisplayName(format!("Secured {}", v.template.id)))
                            } else if world.units.contains_key(&id) {
                                Some(DisplayName("PoliceSystem patrol".into()))
                            } else {
                                None
                            },
                        },
                        EntityState::Vehicle(v.state.clone()),
                    ),
                )
            }))
            .chain(
                world
                    .officers
                    .iter()
                    .filter(|(_, officer)| !officer.mounted)
                    .map(|(&id, officer)| {
                        (
                            id,
                            (
                                EntityDescriptor {
                                    id: EntityId(id),
                                    entity_kind: EntityKind::Police,
                                    template_id: TemplateId("player_greybox".into()),
                                    display_name: None,
                                    tshirt_colour: Some(TshirtColour(0x263f68)),
                                },
                                EntityState::Character(officer.state.clone()),
                            ),
                        )
                    }),
            )
            .chain(world.peds.iter().map(|(&id, p)| {
                (
                    id,
                    (
                        EntityDescriptor {
                            tshirt_colour: None,
                            id: p.id,
                            entity_kind: EntityKind::Pedestrian,
                            template_id: TemplateId("pedestrian_greybox".into()),
                            display_name: None,
                        },
                        EntityState::Character(p.state.clone()),
                    ),
                )
            }))
            .collect()
    }
    pub(crate) fn controlled(s: &Session, world: &View) -> u32 {
        let player = s.player.expect("authenticated player");
        world.players[&player.0]
            .state
            .vehicle_id
            .unwrap_or(player)
            .0
    }
    /// Refresh descriptors reliably without changing control epochs or discarding player inputs.
    pub(crate) fn refresh_appearance(
        ids: &[u32],
        world: &View,
        sessions: &mut Sessions,
    ) -> Result<(), String> {
        let entities = Self::entities(world);
        let mut messages = Vec::new();
        for (&peer, session) in &mut sessions.peers {
            for id in ids {
                if !session.visible.contains_key(id) {
                    continue;
                }
                session.visibility_counter = session
                    .visibility_counter
                    .checked_add(1)
                    .ok_or("visibility exhausted")?;
                session.visible.insert(*id, session.visibility_counter);
                let (descriptor, state) = entities[id].clone();
                messages.push((
                    peer,
                    session.id.clone(),
                    ServerMessage::EntityShow(EntityShow {
                        baseline_id: None,
                        visibility_id: VisibilityId(session.visibility_counter),
                        descriptor,
                        state,
                        state_tick: world.tick,
                    }),
                ));
            }
        }
        for (peer, sid, message) in messages {
            Self::reliable(peer, sid.as_deref(), message, world, sessions);
        }
        Ok(())
    }
    pub(crate) fn status(tick: u32, s: &Session, world: &View) -> Result<PlayerStatus, String> {
        let p = s.profile.as_ref().ok_or("profile missing")?;
        Ok(PlayerStatus {
            equipped_weapon: world.players[&s.player_id()].weapon.equipped,
            inventory: crate::replication::projection::views(&world.players[&s.player_id()].weapon),
            equip_end_tick: world.players[&s.player_id()].weapon.equip_end,
            revision: Revision(s.status_revision),
            server_tick: tick,
            credits: p.credits,
            repair: s.repair_view.clone(),
            completed_jobs: p.completed_jobs,
            heat: Heat(
                world
                    .wanted
                    .get(&(s.player_id()))
                    .expect("admitted wanted state")
                    .heat,
            ),
            pursuit: world
                .wanted
                .get(&(s.player_id()))
                .expect("admitted wanted state")
                .pursuit,
            magazine: Magazine(
                world.players[&s.player_id()].weapon.slots
                    [world.players[&s.player_id()].weapon.equipped.index()]
                .magazine,
            ),
            reload_end_tick: world.players[&s.player_id()].weapon.reload_end,
            mission: world
                .missions
                .get(&(s.player_id()))
                .expect("admitted mission state")
                .0
                .as_ref()
                .map(|m| m.view()),
            respawn_tick: world.players[&s.player_id()].lifecycle.respawn,
            protected_until_tick: world.players[&s.player_id()].lifecycle.protected_until,
        })
    }
    pub(crate) fn baseline(
        peer: PeerKey,
        s: &mut Session,
        now: u64,
        world: &View,
        sessions: &mut Sessions,
    ) -> Result<(), String> {
        s.baseline = s.baseline.checked_add(1).ok_or("baseline exhausted")?;
        s.phase = SessionPhase::Loading;
        s.inputs.clear();
        s.visible.clear();
        s.deadline = now.saturating_add(30_000);
        Projection::reliable(
            peer,
            s.id.as_deref(),
            ServerMessage::BaselineBegin(BaselineBegin {
                baseline_id: BaselineId(s.baseline),
                server_tick: world.tick,
            }),
            world,
            sessions,
        );
        s.last_replicated.clear();
        let entities = Projection::entities(world);
        let spatial = SpatialIndex::new(entities.iter().map(|(&id, (_, state))| {
            let p = entity_position(state);
            (id, [p[0], p[2]])
        }));
        let owned = Projection::controlled(s, world);
        let origin = entity_position(&entities[&owned].1);
        let mut visible = spatial.relevant([origin[0], origin[2]], owned, &s.visible);
        visible.insert(s.player.ok_or("missing player")?.0);
        for id in visible {
            s.visibility_counter = s
                .visibility_counter
                .checked_add(1)
                .ok_or("visibility exhausted")?;
            s.visible.insert(id, s.visibility_counter);
            Projection::reliable(
                peer,
                s.id.as_deref(),
                ServerMessage::EntityShow(EntityShow {
                    baseline_id: Some(BaselineId(s.baseline)),
                    visibility_id: VisibilityId(s.visibility_counter),
                    descriptor: entities[&id].0.clone(),
                    state: entities[&id].1.clone(),
                    state_tick: world.tick,
                }),
                world,
                sessions,
            );
        }
        Projection::reliable(
            peer,
            s.id.as_deref(),
            ServerMessage::BaselineEnd(BaselineEnd {
                baseline_id: BaselineId(s.baseline),
                entity_count: EntityCount(s.visible.len() as u16),
            }),
            world,
            sessions,
        );
        Ok(())
    }
    pub(crate) fn sync_visible(world: &View, sessions: &mut Sessions) -> Result<(), String> {
        let entities = Projection::entities(world);
        let spatial = SpatialIndex::new(entities.iter().map(|(&id, (_, state))| {
            let p = entity_position(state);
            (id, [p[0], p[2]])
        }));
        let mut messages = Vec::new();
        for (&peer, s) in &mut sessions.peers {
            if s.phase == SessionPhase::AwaitHello {
                continue;
            }
            let player = s.player.ok_or("missing observer")?.0;
            let owner = world.players[&player]
                .state
                .vehicle_id
                .map_or(player, |v| v.0);
            let origin = entity_position(&entities[&owner].1);
            let mut present = spatial.relevant([origin[0], origin[2]], owner, &s.visible);
            present.insert(player);
            for id in s.visible.keys().copied().collect::<Vec<_>>() {
                if !present.contains(&id) {
                    let visibility = s.visible.remove(&id).ok_or("visibility missing")?;
                    s.last_replicated.remove(&id);
                    messages.push((
                        peer,
                        s.id.clone(),
                        ServerMessage::EntityHide(EntityHide {
                            entity_id: EntityId(id),
                            visibility_id: VisibilityId(visibility),
                            server_tick: world.tick,
                            reason: if entities.contains_key(&id) {
                                HideReason::OutOfRange
                            } else {
                                HideReason::Disconnected
                            },
                        }),
                    ));
                }
            }
            for id in &present {
                if !s.visible.contains_key(id) {
                    s.visibility_counter = s
                        .visibility_counter
                        .checked_add(1)
                        .ok_or("visibility exhausted")?;
                    s.visible.insert(*id, s.visibility_counter);
                    // Build after releasing the mutable session borrow.
                    messages.push((
                        peer,
                        s.id.clone(),
                        ServerMessage::EntityShow(EntityShow {
                            baseline_id: None,
                            visibility_id: VisibilityId(s.visibility_counter),
                            descriptor: entities[id].0.clone(),
                            state: entities[id].1.clone(),
                            state_tick: world.tick,
                        }),
                    ));
                }
            }
        }
        for (peer, sid, message) in messages {
            Projection::reliable(peer, sid.as_deref(), message, world, sessions);
        }
        Ok(())
    }
    pub(super) fn player_map(world: &View, sessions: &mut Sessions) {
        let players: Vec<_> = sessions
            .peers
            .values()
            .filter(|s| s.phase == SessionPhase::Playing)
            .filter_map(|s| s.player.and_then(|id| world.players.get(&id.0)))
            .map(|p| PlayerMapEntry {
                player_id: p.id,
                position: p
                    .state
                    .vehicle_id
                    .and_then(|id| world.cars.get(&id.0))
                    .map_or_else(
                        || p.state.motor.position.clone(),
                        |v| v.state.motor.position.clone(),
                    ),
            })
            .collect();
        let recipients: Vec<_> = sessions
            .peers
            .iter()
            .filter(|(_, s)| s.phase == SessionPhase::Playing)
            .map(|(&peer, s)| (peer, s.id.clone()))
            .collect();
        for (peer, sid) in recipients {
            Self::reliable(
                peer,
                sid.as_deref(),
                ServerMessage::PlayerMap(players.clone()),
                world,
                sessions,
            );
        }
    }
    pub(super) fn owner_states(world: &View, sessions: &mut Sessions) -> Result<(), String> {
        let updates: Result<Vec<_>, String> = sessions
            .peers
            .iter()
            .filter(|(_, s)| s.phase == SessionPhase::Playing)
            .map(|(peer, s)| Ok((*peer, s.id.clone(), Projection::owner(s, world)?)))
            .collect();
        for (peer, sid, owner) in updates? {
            Projection::send(
                peer,
                sid.as_deref(),
                Delivery::Unreliable,
                ServerMessage::OwnerState(owner),
                world,
                sessions,
            );
        }
        Ok(())
    }
    pub(super) fn snapshots(world: &View, sessions: &Sessions) -> Result<PeerPackets, String> {
        let mut batches = PeerPackets::new();
        let entities = Projection::entities(world);
        for (&peer, session) in &sessions.peers {
            if session.phase != SessionPhase::Playing {
                continue;
            }
            let player = session.player.ok_or("missing owner")?.0;
            let owned = world.players[&player]
                .state
                .vehicle_id
                .map_or(player, |v| v.0);
            let position = entity_position(&entities[&owned].1);
            let mut ids: Vec<_> = session
                .visible
                .keys()
                .copied()
                .filter(|id| *id != owned)
                .filter(|id| {
                    world.cars.get(id).is_none_or(|v| {
                        v.idle_ticks < 30
                            || v.state.driver_id.is_some()
                            || world.tick.is_multiple_of(15)
                    })
                })
                .collect();
            ids.sort_by(|a, b| {
                let age_a = world
                    .tick
                    .saturating_sub(*session.last_replicated.get(a).unwrap_or(&world.tick));
                let age_b = world
                    .tick
                    .saturating_sub(*session.last_replicated.get(b).unwrap_or(&world.tick));
                let dist = |id: &u32| {
                    let p = entity_position(&entities[id].1);
                    (p[0] - position[0]).hypot(p[2] - position[2])
                };
                (age_b >= 9)
                    .cmp(&(age_a >= 9))
                    .then_with(|| {
                        if age_a >= 9 && age_b >= 9 {
                            age_b.cmp(&age_a)
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    })
                    .then(dist(a).total_cmp(&dist(b)))
                    .then(a.cmp(b))
            });
            let records = ids.into_iter().map(|id| StateRecord {
                entity_id: EntityId(id),
                visibility_id: VisibilityId(session.visible[&id]),
                state: entities[&id].1.clone(),
            });
            for envelope in packets(
                world.id,
                session.id.as_deref().ok_or("missing session")?,
                world.tick,
                records,
            )? {
                batches
                    .entry(peer)
                    .or_default()
                    .push((Delivery::Unreliable, envelope));
            }
        }
        Ok(batches)
    }
}
// Scenario diagnostics use the same query projection as the scheduled publisher.
#[cfg(test)]
pub(crate) fn with_view<R>(world: &mut crate::world::World, f: impl FnOnce(&View) -> R) -> R {
    let tick = world.tick;
    let id = world.id.clone();
    let mut state = bevy_ecs::system::SystemState::<Actors>::new(world.entities.ecs_mut());
    let actors = state
        .get_mut(world.entities.ecs_mut())
        .expect("replication queries");
    f(&actors.view(tick, &id))
}
pub(crate) fn sync_visible(
    actors: PublicActors,
    clock: Res<Clock>,
    identity: Res<WorldIdentity>,
    mut sessions: ResMut<Sessions>,
    mut outcome: ResMut<TickOutcome>,
) {
    if let Err(error) = Projection::sync_visible(&actors.view(clock.0, &identity.0), &mut sessions)
    {
        outcome.fail(error);
    }
}
