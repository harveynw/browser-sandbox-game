//! Packet-ordered protocol adapter. Actor effects commit between FIFO packets;
//! admissions and baselines are messages consumed after deferred lifecycle work.
use super::*;
use crate::sessions::native::{DisconnectRequested, LifecycleActors, PossessionRequested};
use crate::{
    ecs::runtime::TickIo,
    ecs::state::{Clock, Content, EntityAllocator, WorldIdentity},
    ecs::{EntityIndex, TickOutcome},
    replication::{
        Replication,
        native::{Actors, Projection},
    },
    systems::{
        chat::ChatSystem,
        missions::native::JobActionRequested,
        weapons::{WeaponActionRequested, WeaponTriggerRequested},
    },
};
use bevy_ecs::{prelude::*, system::SystemParam};
use std::collections::BTreeSet;
use std::result::Result;
#[derive(Message, Clone)]
pub(crate) struct AdmissionRequested {
    peer: PeerKey,
    profile: ProfileView,
    tick: u32,
}
#[derive(Message, Clone)]
pub(crate) struct BaselineRequested {
    peer: PeerKey,
    tick: u32,
}
#[derive(Resource, Default)]
pub(crate) struct Ingress {
    pub completed_profile: bool,
    events: Vec<TransportEvent>,
    closed: BTreeSet<PeerKey>,
    pub packets: VecDeque<(PeerKey, ClientEnvelope)>,
}
#[derive(SystemParam)]
pub(crate) struct Protocol<'w, 's> {
    sessions: ResMut<'w, Sessions>,
    io: NonSendMut<'w, TickIo>,
    clock: Res<'w, Clock>,
    identity: Res<'w, WorldIdentity>,
    content: Res<'w, Content>,
    metrics: ResMut<'w, crate::world::Metrics>,
    actors: ParamSet<'w, 's, (LifecycleActors<'w, 's>, Actors<'w, 's>)>,
    index: Res<'w, EntityIndex>,
    chat: ResMut<'w, ChatSystem>,
    participants: Query<'w, 's, &'static mut crate::components::ChatParticipant>,
    admissions: MessageWriter<'w, AdmissionRequested>,
    baselines: MessageWriter<'w, BaselineRequested>,
    disconnects: MessageWriter<'w, DisconnectRequested>,
    triggers: MessageWriter<'w, WeaponTriggerRequested>,
    weapons: MessageWriter<'w, WeaponActionRequested>,
    jobs: MessageWriter<'w, JobActionRequested>,
    possession: MessageWriter<'w, PossessionRequested>,
    ready: MessageWriter<'w, crate::gameplay_messages::PlayerBecameReady>,
}
impl Protocol<'_, '_> {
    fn send(
        &mut self,
        peer: PeerKey,
        sid: Option<&str>,
        delivery: Delivery,
        message: ServerMessage,
    ) {
        Replication::queue(
            peer,
            sid,
            delivery,
            message,
            &self.identity.0,
            &mut self.sessions,
        );
    }
    fn reliable(&mut self, peer: PeerKey, sid: Option<&str>, message: ServerMessage) {
        self.send(peer, sid, Delivery::Reliable, message);
    }
    fn owner(&mut self, session: &Session) -> Result<OwnerState, String> {
        Projection::owner(
            session,
            &self.actors.p1().view(self.clock.0, &self.identity.0),
        )
    }
    fn status(&mut self, session: &Session) -> Result<PlayerStatus, String> {
        Projection::status(
            self.clock.0,
            session,
            &self.actors.p1().view(self.clock.0, &self.identity.0),
        )
    }
    fn player_ready(&mut self, peer: PeerKey, session: &Session) {
        if session.baseline != 1 {
            return;
        }
        if let (Some(player), Some(sid)) = (session.player, session.id.as_ref())
            && let Some(&entity) = self.index.0.get(&player.0)
        {
            self.ready
                .write(crate::gameplay_messages::PlayerBecameReady {
                    peer,
                    player,
                    entity,
                    session: sid.clone(),
                    tick: self.clock.0,
                });
        }
    }
    fn chat_message(
        &mut self,
        peer: PeerKey,
        session: &Session,
        text: ChatText,
        now: u64,
    ) -> Result<(), &'static str> {
        let actors = self.actors.p1();
        let participant = session
            .player
            .and_then(|id| self.index.0.get(&id.0))
            .and_then(|&entity| self.participants.get_mut(entity).ok());
        let player = actors.player(session.player).zip(participant);
        self.chat.handle_message(
            peer,
            session,
            text,
            now,
            player.map(|(player, participant)| (player, participant.into_inner())),
            self.clock.0,
            &self.identity.0,
            &mut self.sessions,
        )
    }
}
use crate::world::build;
pub(crate) fn receive_packet(mut ctx: Protocol, mut ingress: ResMut<Ingress>) {
    let Some((peer, message)) = ingress.packets.pop_front() else {
        return;
    };
    let Some(mut s) = ctx.sessions.peers.remove(&peer) else {
        return;
    };
    let now = ctx.io.now;
    let allow_joins = ctx.io.allow_joins;
    let result = handle(&mut ctx, peer, &mut s, message, now, allow_joins);
    ctx.sessions.peers.insert(peer, s);
    if let Err(reason) = result {
        let tick = ctx.clock.0;
        ctx.disconnects
            .write(DisconnectRequested { peer, reason, tick });
    }
}
fn handle(
    ctx: &mut Protocol,
    peer: PeerKey,
    s: &mut Session,
    message: ClientEnvelope,
    now: u64,
    allow_joins: bool,
) -> Result<(), &'static str> {
    if protocol::validate::validate_client(&message).is_err()
        || message.world_id.as_ref().map(|v| v.0.as_str()) != Some(ctx.identity.0.as_str())
        || message.session_id.as_ref().map(|v| v.0.as_str()) != s.id.as_deref()
    {
        return Err("invalid session scope");
    }
    let bytes = encode_client(&message)
        .map_err(|_| "invalid input encoding")?
        .len();
    if !s.ingress_budget.allow(bytes, now, 0) {
        return Err("ingress byte budget exceeded");
    }
    ctx.metrics.ingress_bytes += bytes as u64;
    s.traffic.up_bytes += bytes as u64;
    s.last_heard = now;
    match message.message {
        ClientMessage::ClientHello(hello)
            if s.phase == SessionPhase::AwaitHello && s.pending_admission.is_none() =>
        {
            let build = build(&ctx.content.0);
            let reject = if hello.build_id != build.build_id {
                Some(RejectCode::VersionMismatch)
            } else if hello.content_hash != build.content_hash
                || hello.sim_config_hash != build.sim_config_hash
            {
                Some(RejectCode::ContentMismatch)
            } else if !allow_joins {
                Some(RejectCode::Unavailable)
            } else {
                None
            };
            if let Some(code) = reject {
                ctx.reliable(
                    peer,
                    None,
                    ServerMessage::Reject(Reject {
                        code,
                        detail: DetailText("local build unavailable or mismatched".into()),
                    }),
                );
                return Err("join rejected");
            }
            let admission = match ctx.io.profiles.begin_admission(
                &hello.join_ticket.0,
                now,
                hello.display_name,
                hello.tshirt_colour,
            ) {
                Ok(admission) => admission,
                Err(code) => {
                    ctx.reliable(
                        peer,
                        None,
                        ServerMessage::Reject(Reject {
                            code,
                            detail: DetailText(
                                "ticket invalid or profile storage unavailable".into(),
                            ),
                        }),
                    );
                    return Err("ticket or profile unavailable");
                }
            };
            match admission {
                crate::admission::Admission::Ready(profile) => {
                    ctx.admissions.write(AdmissionRequested {
                        peer,
                        profile,
                        tick: ctx.clock.0,
                    });
                }
                crate::admission::Admission::Pending(id) => s.pending_admission = Some(id),
            }
        }
        ClientMessage::ClientReady(ready) if s.phase == SessionPhase::Loading => {
            if ready.baseline_id.0 != s.baseline {
                return Ok(());
            }
            if s.baseline == 1 {
                native::protect_on_ready(
                    s.player.ok_or("ready owner missing")?,
                    ctx.clock.0,
                    &mut ctx.actors.p0(),
                )?;
            }
            s.phase = SessionPhase::Playing;
            ctx.io.profiles.activate(
                &s.profile.as_ref().ok_or("profile missing")?.profile_id.0,
                s.id.as_deref().ok_or("session missing")?,
            );
            let owner = ctx.owner(s).map_err(|_| "owner missing")?;
            ctx.reliable(
                peer,
                s.id.as_deref(),
                ServerMessage::StartPlay(StartPlay {
                    baseline_id: ready.baseline_id,
                    owner,
                }),
            );
            let status = ctx.status(s).map_err(|_| "status unavailable")?;
            ctx.reliable(peer, s.id.as_deref(), ServerMessage::PlayerStatus(status));
            ctx.player_ready(peer, s);
        }
        ClientMessage::Chat(text) => ctx.chat_message(peer, s, text, now)?,
        ClientMessage::InputBatch(batch) => {
            if !s.input_rate.allow(now) {
                // RTC congestion can release more than a second of legitimate
                // 30 Hz input at once. Keep the strict processing cap, discard
                // excess stale work, and let fresh input recover the session.
                ctx.metrics.rate_limited_inputs += 1;
                return Ok(());
            }
            if s.phase != SessionPhase::Playing {
                return Ok(());
            }
            if batch.control_epoch.0 != s.epoch {
                ctx.metrics.stale_epochs += 1;
                return Ok(());
            }
            let next = ctx.clock.0 + 1;
            for frame in batch.frames {
                if frame.tick < next {
                    ctx.metrics.late_inputs += 1;
                    continue;
                }
                if frame.tick > next.saturating_add(8) || frame.tick == u32::MAX {
                    ctx.metrics.future_inputs += 1;
                    continue;
                }
                if let Some(previous) = s.inputs.get(&frame.tick) {
                    if *previous != frame {
                        ctx.metrics.conflicting_inputs += 1;
                    }
                    continue;
                }
                s.inputs.insert(frame.tick, frame);
            }
        }
        ClientMessage::Ping(ping) if s.phase != SessionPhase::AwaitHello => {
            // The crate lacks ingress delivery metadata; this existing nonce convention
            // selects only the requested response channel, never asserts ingress type.
            let channel = if ping.nonce & 0x8000_0000 != 0 {
                Delivery::Unreliable
            } else {
                Delivery::Reliable
            };
            ctx.send(
                peer,
                s.id.as_deref(),
                channel,
                ServerMessage::Pong(Pong {
                    nonce: ping.nonce,
                    client_time_ms: ping.client_time_ms,
                    server_tick: ctx.clock.0,
                    server_time_ms: Milliseconds(now as f64),
                }),
            );
        }
        ClientMessage::ResyncRequest(_) if s.phase == SessionPhase::Playing => {
            if s.last_resync
                .is_some_and(|last| now.saturating_sub(last) < 5000)
            {
                return Ok(());
            }
            s.last_resync = Some(now);
            s.epoch = s.epoch.checked_add(1).ok_or("epoch exhausted")?;
            if let Some(id) = s.player {
                native::freeze_on_resync(id, &mut ctx.actors.p0())?;
            }
            ctx.baselines.write(BaselineRequested {
                peer,
                tick: ctx.clock.0,
            });
        }
        ClientMessage::Trigger(intent) => {
            if !s.trigger_rate.allow(1, now, 0) {
                return Err("trigger rate exceeded");
            }
            ctx.triggers
                .write(crate::systems::weapons::WeaponTriggerRequested {
                    peer,
                    intent,
                    tick: ctx.clock.0,
                    entity: s.player.and_then(|id| ctx.index.0.get(&id.0).copied()),
                });
        }
        ClientMessage::Action(request) => {
            if !s.action_rate.allow(1, now, 0) {
                return Err("action rate exceeded");
            }
            let result = if let Some(cached) = s.actions.get(&request.request_id.0) {
                cached.clone()
            } else {
                let code = if s.phase != SessionPhase::Playing {
                    ActionCode::NotReady
                } else if request.control_epoch.0 != s.epoch {
                    ActionCode::StaleEpoch
                } else if request.request_id.0 <= s.last_request {
                    ActionCode::StaleRequest
                } else {
                    if matches!(request.action, Action::AcceptMission(_) | Action::Repair(_))
                        && s.profile
                            .as_ref()
                            .is_some_and(|p| ctx.io.profiles.economic_pending(&p.profile_id.0))
                    {
                        ActionCode::Unavailable
                    } else {
                        if matches!(
                            request.action,
                            Action::AcceptMission(_) | Action::CancelMission(_) | Action::Repair(_)
                        ) {
                            ctx.jobs
                                .write(crate::systems::missions::native::JobActionRequested {
                                    peer,
                                    request: request.clone(),
                                    tick: ctx.clock.0,
                                    entity: s.player.and_then(|id| ctx.index.0.get(&id.0).copied()),
                                });
                            return Ok(());
                        }
                        if matches!(
                            request.action,
                            Action::Fire(_) | Action::Reload(_) | Action::EquipWeapon(_)
                        ) {
                            ctx.weapons
                                .write(crate::systems::weapons::WeaponActionRequested {
                                    peer,
                                    request: request.clone(),
                                    tick: ctx.clock.0,
                                    entity: s.player.and_then(|id| ctx.index.0.get(&id.0).copied()),
                                });
                            return Ok(());
                        }
                        ctx.possession.write(native::PossessionRequested {
                            peer,
                            request: request.clone(),
                            entity: s.player.and_then(|id| ctx.index.0.get(&id.0).copied()),
                            tick: ctx.clock.0,
                        });
                        return Ok(());
                    }
                };
                let result = ActionResult {
                    request_id: request.request_id,
                    applied_tick: ctx.clock.0,
                    ok: code == ActionCode::Ok,
                    code,
                    affected_entity_id: None,
                };
                if request.request_id.0 > s.last_request {
                    s.last_request = request.request_id.0;
                    s.actions.insert(request.request_id.0, result.clone());
                    while s.actions.len() > 32 {
                        s.actions.pop_first();
                    }
                }
                result
            };
            ctx.reliable(peer, s.id.as_deref(), ServerMessage::ActionResult(result));
        }
        ClientMessage::Leave(_) => return Err("left"),
        ClientMessage::ClientReady(_) => {} // A delayed duplicate readiness cannot start twice.
        _ => return Err("invalid phase"),
    }
    Ok(())
}
pub(crate) fn drain(
    mut network: NonSendMut<Network>,
    mut io: NonSendMut<TickIo>,
    clock: Res<Clock>,
    sessions: Res<Sessions>,
    mut ingress: ResMut<Ingress>,
    mut requests: MessageWriter<DisconnectRequested>,
) {
    io.profiles.poll_store(clock.0);
    ingress.events = network.drain(128);
    ingress.closed = ingress
        .events
        .iter()
        .filter_map(|e| match e {
            TransportEvent::Closed { peer, .. } => Some(*peer),
            _ => None,
        })
        .collect();
    for &peer in &ingress.closed {
        requests.write(DisconnectRequested {
            peer,
            reason: "transport closed",
            tick: clock.0,
        });
    }
    for (&peer, s) in &sessions.peers {
        if if s.phase == SessionPhase::Playing {
            io.now.saturating_sub(s.last_heard) >= 10_000
        } else {
            io.now >= s.deadline
        } {
            requests.write(DisconnectRequested {
                peer,
                reason: "session timeout",
                tick: clock.0,
            });
        }
    }
}
pub(crate) fn open(
    mut ingress: ResMut<Ingress>,
    mut sessions: ResMut<Sessions>,
    io: NonSend<TickIo>,
    identity: Res<WorldIdentity>,
    content: Res<Content>,
) {
    let now = io.now;
    let mut messages = Vec::new();
    for event in std::mem::take(&mut ingress.events) {
        match event {
            TransportEvent::Opened(peer)
                if !ingress.closed.contains(&peer) && !sessions.peers.contains_key(&peer) =>
            {
                if sessions.peers.len() >= MAX_PLAYERS {
                    sessions.closing.insert(peer, "connection capacity");
                    continue;
                }
                sessions.peers.insert(peer, new_session(now));
                Replication::queue(
                    peer,
                    None,
                    Delivery::Reliable,
                    ServerMessage::ServerHello(build(&content.0)),
                    &identity.0,
                    &mut sessions,
                );
            }
            TransportEvent::Message { peer, message } if !ingress.closed.contains(&peer) => {
                messages.push((peer, message))
            }
            _ => {}
        }
    }
    messages.sort_by_key(|(peer, _)| *peer);
    ingress.packets = messages.into();
}
fn new_session(now: u64) -> Session {
    Session {
        traffic: Default::default(),
        id: None,
        profile: None,
        player: None,
        pending_admission: None,
        phase: SessionPhase::AwaitHello,
        epoch: 1,
        baseline: 0,
        visibility_counter: 0,
        visible: BTreeMap::new(),
        inputs: BTreeMap::new(),
        actions: BTreeMap::new(),
        last_request: 0,
        repair_view: None,
        repair_clear: None,
        status_revision: 1,
        status_dirty: false,
        last_heard: now,
        deadline: now.saturating_add(5000),
        last_resync: None,
        ingress_budget: Budget::new(30 * 1024, 60 * 1024),
        egress_budget: Budget::new(60 * 1024, 120 * 1024),
        input_rate: InputRate::default(),
        action_rate: Budget::new(10, 20),
        trigger_rate: Budget::new(12, 24),
        last_replicated: BTreeMap::new(),
    }
}
pub(crate) fn pending_profiles(
    mut ingress: ResMut<Ingress>,
    mut sessions: ResMut<Sessions>,
    mut io: NonSendMut<TickIo>,
    clock: Res<Clock>,
    identity: Res<WorldIdentity>,
    mut admissions: MessageWriter<AdmissionRequested>,
    mut disconnects: MessageWriter<DisconnectRequested>,
) {
    ingress.completed_profile = false;
    let mut rejected = Vec::new();
    for (&peer, s) in &mut sessions.peers {
        let result = s
            .pending_admission
            .as_deref()
            .and_then(|id| io.profiles.admission_result(id));
        if let Some(result) = result {
            ingress.completed_profile = true;
            s.pending_admission = None;
            match result {
                Ok(profile) if io.allow_joins => {
                    admissions.write(AdmissionRequested {
                        peer,
                        profile,
                        tick: clock.0,
                    });
                }
                _ => {
                    rejected.push(peer);
                }
            }
            break;
        }
    }
    for peer in rejected {
        Replication::queue(
            peer,
            None,
            Delivery::Reliable,
            ServerMessage::Reject(Reject {
                code: RejectCode::Unavailable,
                detail: DetailText("profile storage unavailable".into()),
            }),
            &identity.0,
            &mut sessions,
        );
        disconnects.write(DisconnectRequested {
            peer,
            reason: "profile storage unavailable",
            tick: clock.0,
        });
    }
}
// Two readers intentionally observe the same admission fact. Replacement cleanup
// commits before the second reader allocates a slot and entity for the new guest.
pub(crate) fn replace_guests(
    mut admissions: MessageReader<AdmissionRequested>,
    mut sessions: ResMut<Sessions>,
    identity: Res<WorldIdentity>,
    clock: Res<Clock>,
    mut disconnects: MessageWriter<DisconnectRequested>,
) {
    for request in admissions.read().filter(|r| r.tick == clock.0) {
        if let Some((&old, s)) = sessions.peers.iter().find(|(peer, s)| {
            **peer != request.peer
                && s.profile
                    .as_ref()
                    .is_some_and(|p| p.profile_id == request.profile.profile_id)
        }) {
            let sid = s.id.clone();
            // The disconnected guest is retired before the new guest gets a slot.
            disconnects.write(DisconnectRequested {
                peer: old,
                reason: "guest replaced",
                tick: clock.0,
            });
            Replication::queue(
                old,
                sid.as_deref(),
                Delivery::Reliable,
                ServerMessage::Disconnect(Disconnect {
                    code: TemplateId("replaced".into()),
                    detail: DetailText("guest joined again".into()),
                }),
                &identity.0,
                &mut sessions,
            );
        }
    }
}
pub(crate) fn admit(
    mut admissions: MessageReader<AdmissionRequested>,
    mut sessions: ResMut<Sessions>,
    mut actors: LifecycleActors,
    mut allocator: ResMut<EntityAllocator>,
    content: Res<Content>,
    clock: Res<Clock>,
    identity: Res<WorldIdentity>,
    mut baselines: MessageWriter<BaselineRequested>,
    mut disconnects: MessageWriter<DisconnectRequested>,
) {
    for request in admissions.read().filter(|r| r.tick == clock.0) {
        let Some(mut s) = sessions.peers.remove(&request.peer) else {
            continue;
        };
        let result =
            native::create_player(&request.profile, &content.0, &mut allocator.0, &mut actors)
                .and_then(|id| {
                    s.player = Some(id);
                    s.profile = Some(request.profile.clone());
                    s.id = Some(
                        crate::admission::random_id("session-", 16)
                            .map_err(|_| "randomness failed")?,
                    );
                    Replication::queue(
                        request.peer,
                        s.id.as_deref(),
                        Delivery::Reliable,
                        ServerMessage::Welcome(Welcome {
                            player_id: id,
                            server_tick: clock.0,
                            input_lead_ticks: InputLead(3),
                            profile: request.profile.clone(),
                        }),
                        &identity.0,
                        &mut sessions,
                    );
                    baselines.write(BaselineRequested {
                        peer: request.peer,
                        tick: clock.0,
                    });
                    Ok(())
                });
        sessions.peers.insert(request.peer, s);
        if let Err(reason) = result {
            if reason == "shard full" {
                Replication::queue(
                    request.peer,
                    None,
                    Delivery::Reliable,
                    ServerMessage::Reject(Reject {
                        code: RejectCode::ServerFull,
                        detail: DetailText("shard full".into()),
                    }),
                    &identity.0,
                    &mut sessions,
                );
            }
            disconnects.write(DisconnectRequested {
                peer: request.peer,
                reason,
                tick: clock.0,
            });
        }
    }
}
pub(crate) fn baselines(
    mut requests: MessageReader<BaselineRequested>,
    actors: Actors,
    mut sessions: ResMut<Sessions>,
    io: NonSend<TickIo>,
    clock: Res<Clock>,
    identity: Res<WorldIdentity>,
    mut disconnects: MessageWriter<DisconnectRequested>,
) {
    let view = actors.view(clock.0, &identity.0);
    for request in requests.read().filter(|r| r.tick == clock.0) {
        let Some(mut s) = sessions.peers.remove(&request.peer) else {
            continue;
        };
        let result = Projection::baseline(request.peer, &mut s, io.now, &view, &mut sessions);
        sessions.peers.insert(request.peer, s);
        if result.is_err() {
            disconnects.write(DisconnectRequested {
                peer: request.peer,
                reason: "baseline unavailable",
                tick: clock.0,
            });
        }
    }
}
pub(crate) fn appearance(
    mut actors: ParamSet<(LifecycleActors, Actors)>,
    mut sessions: ResMut<Sessions>,
    io: NonSend<TickIo>,
    clock: Res<Clock>,
    identity: Res<WorldIdentity>,
    mut outcome: ResMut<TickOutcome>,
) {
    let changed = native::apply_appearance(&mut sessions, &io.profiles, &mut actors.p0());
    if !changed.is_empty()
        && let Err(error) = Projection::refresh_appearance(
            &changed,
            &actors.p1().view(clock.0, &identity.0),
            &mut sessions,
        )
    {
        outcome.fail(error);
    }
}
pub(crate) fn retire(
    mut admissions: ResMut<Messages<AdmissionRequested>>,
    mut baselines: ResMut<Messages<BaselineRequested>>,
) {
    admissions.clear();
    baselines.clear();
}
