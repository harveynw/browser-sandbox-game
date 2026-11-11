use crate::components::{ChatParticipant, Player as PlayerComponent};
#[cfg(test)]
use crate::world::World;
use crate::{
    replication::Replication,
    sessions::{Session, SessionPhase, Sessions},
    transport::PeerKey,
};
use bevy_ecs::prelude::{MessageReader, Query, Res, ResMut};
use protocol::{ChatLine, ChatText, DetailText, DisplayName, EventId, ServerMessage};
use std::collections::VecDeque;
#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct ChatSystem {
    /// The latest 32 accepted messages, replayed to newly ready players.
    messages: VecDeque<ChatLine>,
    /// Monotonic IDs let clients deduplicate history and live messages.
    next_id: u32,
}
impl ChatSystem {
    /// The sender is temporarily detached from Sessions while its message is handled.
    pub(crate) fn handle_message(
        &mut self,
        peer: PeerKey,
        sender: &Session,
        text: ChatText,
        now: u64,
        player: Option<(&PlayerComponent, &mut ChatParticipant)>,
        tick: u32,
        world_id: &str,
        sessions: &mut Sessions,
    ) -> Result<(), &'static str> {
        if sender.phase != SessionPhase::Playing {
            return Err("chat player not ready");
        }
        let (player, participant) = player.ok_or("chat player missing")?;
        match self.send(participant, player.display_name.clone(), text, tick, now) {
            Ok(line) => {
                let recipients: Vec<_> = sessions
                    .peers
                    .iter()
                    .filter(|(_, session)| session.player.is_some())
                    .map(|(&peer, session)| (peer, session.id.clone()))
                    .chain(std::iter::once((peer, sender.id.clone())))
                    .collect();
                for (target, sid) in recipients {
                    Replication::queue(
                        target,
                        sid.as_deref(),
                        protocol::Delivery::Reliable,
                        ServerMessage::Chat(line.clone()),
                        world_id,
                        sessions,
                    );
                }
            }
            Err(reason) => Replication::queue(
                peer,
                sender.id.as_deref(),
                protocol::Delivery::Reliable,
                ServerMessage::ChatError(DetailText(reason.into())),
                world_id,
                sessions,
            ),
        }
        Ok(())
    }
    /// Replay recent chat once on initial readiness; resyncs already receive live messages.
    pub(crate) fn on_ready(
        &mut self,
        peer: PeerKey,
        session: &Session,
        player: Option<(&PlayerComponent, &mut ChatParticipant)>,
        tick: u32,
        world_id: &str,
        sessions: &mut Sessions,
    ) {
        let Some((player, participant)) = player else {
            return;
        };
        if session.baseline != 1 || participant.announced_name.is_some() {
            return;
        }
        for line in &self.messages {
            Replication::queue(
                peer,
                session.id.as_deref(),
                protocol::Delivery::Reliable,
                ServerMessage::Chat(line.clone()),
                world_id,
                sessions,
            );
        }
        let name = player.display_name.clone();
        participant.announced_name = Some(name.clone());
        self.presence(
            name,
            true,
            Some((peer, session.id.clone())),
            tick,
            world_id,
            sessions,
        );
    }
    fn presence(
        &mut self,
        username: DisplayName,
        joined: bool,
        extra: Option<(PeerKey, Option<String>)>,
        tick: u32,
        world_id: &str,
        sessions: &mut Sessions,
    ) {
        self.system_message(
            username.clone(),
            ChatText(format!(
                "{} has {} the game",
                username.0,
                if joined { "joined" } else { "left" }
            )),
            extra,
            tick,
            world_id,
            sessions,
        );
    }
    fn system_message(
        &mut self,
        username: DisplayName,
        text: ChatText,
        extra: Option<(PeerKey, Option<String>)>,
        tick: u32,
        world_id: &str,
        sessions: &mut Sessions,
    ) {
        let Some(line) = self.record_system_message(username, text, tick) else {
            return;
        };
        let recipients: Vec<_> = sessions
            .peers
            .iter()
            .filter(|(_, s)| s.player.is_some())
            .map(|(&p, s)| (p, s.id.clone()))
            .chain(extra)
            .collect();
        for (peer, sid) in recipients {
            Replication::queue(
                peer,
                sid.as_deref(),
                protocol::Delivery::Reliable,
                ServerMessage::Chat(line.clone()),
                world_id,
                sessions,
            );
        }
    }
    fn record_system_message(
        &mut self,
        username: DisplayName,
        text: ChatText,
        tick: u32,
    ) -> Option<ChatLine> {
        let id = self.next_id.checked_add(1)?;
        self.next_id = id;
        let line = ChatLine {
            system: Some(true),
            id: EventId(id),
            username: username.clone(),
            text,
            server_tick: tick,
        };
        if self.messages.len() == 32 {
            self.messages.pop_front();
        }
        self.messages.push_back(line.clone());
        Some(line)
    }
    #[cfg(test)]
    pub(crate) fn on_left(&mut self, player: u32, world: &mut World, sessions: &mut Sessions) {
        let name = world
            .entities
            .get::<PlayerComponent>(protocol::EntityId(player))
            .map(|p| p.display_name.clone());
        if let Some(participant) = world.entities.get_mut_by_id::<ChatParticipant>(&player) {
            self.left(participant, name, world.tick, &world.id, sessions);
        }
    }
    pub(crate) fn left(
        &mut self,
        participant: &mut ChatParticipant,
        current_name: Option<DisplayName>,
        tick: u32,
        world_id: &str,
        sessions: &mut Sessions,
    ) {
        if let Some(name) = participant.announced_name.take() {
            self.presence(
                current_name.unwrap_or(name),
                false,
                None,
                tick,
                world_id,
                sessions,
            );
        }
    }
    fn send(
        &mut self,
        participant: &mut ChatParticipant,
        username: DisplayName,
        text: ChatText,
        tick: u32,
        now: u64,
    ) -> Result<ChatLine, &'static str> {
        if !protocol::valid_text("ChatText", &text.0) {
            return Err("Chat must contain 1–256 characters and at most 512 bytes.");
        }
        if participant
            .last_sent
            .is_some_and(|last| now.saturating_sub(last) < 1000)
        {
            return Err("Please wait a second between messages.");
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Chat unavailable: message IDs exhausted.")?;
        let line = ChatLine {
            system: None,
            id: EventId(self.next_id),
            username,
            text,
            server_tick: tick,
        };
        participant.last_sent = Some(now);
        if self.messages.len() == 32 {
            self.messages.pop_front();
        }
        self.messages.push_back(line.clone());
        Ok(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_is_bounded_and_rate_limits_belong_to_each_participant() {
        let mut chat = ChatSystem::default();
        let mut participant = ChatParticipant::default();
        for n in 0..40 {
            chat.send(
                &mut participant,
                DisplayName("Guest One".into()),
                ChatText(format!("Message {n}")),
                n,
                n as u64 * 1000,
            )
            .unwrap();
        }
        assert_eq!(chat.messages.len(), 32);
        assert_eq!(chat.messages.front().unwrap().id.0, 9);
        assert!(
            chat.send(
                &mut participant,
                DisplayName("Guest One".into()),
                ChatText("Too soon".into()),
                40,
                39001
            )
            .is_err()
        );
        assert_eq!(chat.messages.len(), 32);
        let mut fresh = ChatParticipant::default();
        assert!(
            chat.send(
                &mut fresh,
                DisplayName("New player".into()),
                ChatText("Hello".into()),
                40,
                39001
            )
            .is_ok()
        );
        assert_eq!(participant.last_sent, Some(39000));
        assert_eq!(fresh.last_sent, Some(39001));
    }
}

pub(crate) fn announce_eliminations(
    mut messages: MessageReader<crate::gameplay_messages::ActorEliminated>,
    mut chat: ResMut<ChatSystem>,
    mut sessions: ResMut<Sessions>,
    world: Res<crate::ecs::state::WorldIdentity>,
) {
    for message in messages.read() {
        // IDs remain available to independent future mission/statistics readers.
        let _ = (message.killer, message.victim);
        let text = ChatText(format!(
            "{} eliminated {}",
            message.killer_name.0, message.victim_name.0
        ));
        let Some(line) =
            chat.record_system_message(message.killer_name.clone(), text, message.tick)
        else {
            continue;
        };
        // Producers temporarily remove the shooter session while handling actions.
        // At this checkpoint it is restored, so broadcast once without an extra sender.
        let recipients: Vec<_> = sessions
            .peers
            .iter()
            .filter(|(_, s)| s.player.is_some())
            .map(|(&peer, s)| (peer, s.id.clone()))
            .collect();
        for (peer, sid) in recipients {
            Replication::queue(
                peer,
                sid.as_deref(),
                protocol::Delivery::Reliable,
                ServerMessage::Chat(line.clone()),
                &world.0,
                &mut sessions,
            );
        }
    }
}

#[cfg(test)]
mod message_tests {
    use super::*;
    use crate::gameplay_messages::{ActorEliminated, CrimeReported, NoiseEmitted};
    use bevy_ecs::prelude::*;
    #[derive(Resource, Default)]
    struct Readers {
        first: Vec<(Option<protocol::EntityId>, protocol::EntityId)>,
        second: usize,
    }
    fn first(mut messages: MessageReader<ActorEliminated>, mut readers: ResMut<Readers>) {
        for message in messages.read() {
            readers.first.push((message.killer, message.victim));
        }
    }
    fn second(mut messages: MessageReader<ActorEliminated>, mut readers: ResMut<Readers>) {
        readers.second += messages.read().count();
    }
    fn emit(mut messages: MessageWriter<ActorEliminated>) {
        messages.write(ActorEliminated {
            killer: Some(protocol::EntityId(1)),
            killer_name: DisplayName("Alice".into()),
            victim: protocol::EntityId(2),
            victim_name: DisplayName("Bob".into()),
            tick: 7,
        });
    }
    fn retire(mut messages: ResMut<Messages<ActorEliminated>>) {
        messages.clear();
    }
    fn fixture() -> bevy_ecs::world::World {
        let mut world = bevy_ecs::world::World::new();
        crate::ecs::messages::register(&mut world);
        world.init_resource::<ChatSystem>();
        world.init_resource::<Sessions>();
        world.init_resource::<Readers>();
        world.init_resource::<crate::ecs::TickOutcome>();
        world.insert_resource(crate::ecs::state::WorldIdentity("test".into()));
        world
    }
    #[test]
    fn independent_readers_and_chat_receive_each_fact_once_across_checkpoints() {
        let mut world = fixture();
        let mut schedule = Schedule::default();
        schedule.add_systems(
            (
                emit,
                announce_eliminations,
                first,
                second,
                retire,
                announce_eliminations,
                first,
                second,
                retire,
            )
                .chain(),
        );
        for _ in 0..3 {
            schedule.run(&mut world);
        }
        assert_eq!(
            world.resource::<Readers>().first,
            [(Some(protocol::EntityId(1)), protocol::EntityId(2)); 3]
        );
        assert_eq!(world.resource::<Readers>().second, 3);
        let history = &world.resource::<ChatSystem>().messages;
        assert_eq!(history.len(), 3);
        assert!(
            history
                .iter()
                .all(|line| line.text.0 == "Alice eliminated Bob" && line.server_tick == 7)
        );
        // No actor entities exist: snapshots still produce valid historical announcements.
        assert_eq!(world.query::<&PlayerComponent>().iter(&world).count(), 0);
        assert!(world.resource::<Messages<ActorEliminated>>().is_empty());
    }
    #[test]
    fn failed_checkpoint_discards_pending_facts_instead_of_replaying_them() {
        let mut world = fixture();
        // Empty police/population managers are built from validated content for real consumers.
        let content = crate::content_loader::WorldContent::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../apps/client/public/content"),
        )
        .unwrap();
        let mut core = crate::world::World::new(content).unwrap();
        let systems = crate::systems::Systems::new(&mut core, &mut Sessions::default()).unwrap();
        crate::ecs::state::install_systems(&mut world, systems);
        world
            .resource_mut::<Messages<NoiseEmitted>>()
            .write(NoiseEmitted {
                position: [0.0; 3],
                tick: 7,
            });
        let mut emit_schedule = Schedule::default();
        emit_schedule.add_systems(emit);
        emit_schedule.run(&mut world);
        world
            .resource_mut::<crate::ecs::TickOutcome>()
            .fail("injected producer failure".into());
        let mut schedule = Schedule::default();
        schedule.add_systems(crate::ecs::checkpoints::reactions());
        schedule.run(&mut world);
        assert!(world.resource::<Messages<ActorEliminated>>().is_empty());
        assert!(world.resource::<Messages<NoiseEmitted>>().is_empty());
        assert!(world.resource::<Messages<CrimeReported>>().is_empty());
        assert!(world.resource::<ChatSystem>().messages.is_empty());
        assert!(
            world
                .resource_mut::<crate::ecs::TickOutcome>()
                .finish()
                .is_err()
        );
        schedule.run(&mut world);
        assert!(world.resource::<ChatSystem>().messages.is_empty());
    }
}

/// Lifecycle reactions run at explicit admission/disconnect checkpoints.
pub(crate) fn player_ready(
    mut messages: MessageReader<crate::gameplay_messages::PlayerBecameReady>,
    mut players: Query<(
        &crate::components::GameId,
        &PlayerComponent,
        &mut ChatParticipant,
    )>,
    mut chat: ResMut<ChatSystem>,
    mut sessions: ResMut<Sessions>,
    clock: Res<crate::ecs::state::Clock>,
    identity: Res<crate::ecs::state::WorldIdentity>,
) {
    for message in messages.read().filter(|m| m.tick == clock.0) {
        let Ok((id, player, mut participant)) = players.get_mut(message.entity) else {
            continue;
        };
        if id.0 != message.player {
            continue;
        }
        let valid = sessions.peers.get(&message.peer).is_some_and(|s| {
            s.player == Some(message.player)
                && s.id.as_deref() == Some(&message.session)
                && s.phase == SessionPhase::Playing
                && s.baseline == 1
        });
        if !valid {
            continue;
        }
        // Existing chat replay/presence routines expect the sender detached.
        let s = sessions.peers.remove(&message.peer).unwrap();
        chat.on_ready(
            message.peer,
            &s,
            Some((player, &mut participant)),
            clock.0,
            &identity.0,
            &mut sessions,
        );
        sessions.peers.insert(message.peer, s);
    }
}
pub(crate) fn player_leaving(
    mut messages: MessageReader<crate::gameplay_messages::PlayerLeaving>,
    mut players: Query<(&crate::components::GameId, &mut ChatParticipant)>,
    mut chat: ResMut<ChatSystem>,
    mut sessions: ResMut<Sessions>,
    clock: Res<crate::ecs::state::Clock>,
    identity: Res<crate::ecs::state::WorldIdentity>,
) {
    for message in messages.read().filter(|m| m.tick == clock.0) {
        if sessions.peers.contains_key(&message.peer) {
            continue;
        }
        let Some(entity) = message.entity else {
            continue;
        };
        let Ok((id, mut participant)) = players.get_mut(entity) else {
            continue;
        };
        if id.0 != message.player {
            continue;
        }
        chat.left(
            &mut participant,
            message.name.clone(),
            clock.0,
            &identity.0,
            &mut sessions,
        );
    }
}
