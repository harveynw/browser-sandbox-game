//! Chat validation, bounded history, reliable broadcasts and join replay.
use crate::{
    replication::Replication,
    sessions::{Session, SessionPhase, Sessions},
    transport::PeerKey,
    world::World,
};
use protocol::{ChatLine, ChatText, DetailText, DisplayName, EventId, ServerMessage};
use std::collections::{BTreeMap, VecDeque};
#[derive(Default)]
pub struct ChatSystem {
    /// The latest 32 accepted messages, replayed to newly ready players.
    messages: VecDeque<ChatLine>,
    /// Last accepted send time per connected player, removed on disconnect.
    last_sent: BTreeMap<u32, u64>,
    /// Monotonic IDs let clients deduplicate history and live messages.
    next_id: u32,
    announced: BTreeMap<u32, DisplayName>,
}
impl ChatSystem {
    /// The sender is temporarily detached from Sessions while its message is handled.
    pub(crate) fn handle_message(
        &mut self,
        peer: PeerKey,
        sender: &Session,
        text: ChatText,
        now: u64,
        world: &mut World,
        sessions: &mut Sessions,
    ) -> Result<(), &'static str> {
        if sender.phase != SessionPhase::Playing {
            return Err("chat player not ready");
        }
        let player = world
            .players
            .get(&sender.player_id())
            .ok_or("chat player missing")?;
        match self.send(
            player.id.0,
            player.display_name.clone(),
            text,
            world.tick,
            now,
        ) {
            Ok(line) => {
                let recipients: Vec<_> = sessions
                    .peers
                    .iter()
                    .filter(|(_, session)| session.player.is_some())
                    .map(|(&peer, session)| (peer, session.id.clone()))
                    .chain(std::iter::once((peer, sender.id.clone())))
                    .collect();
                for (target, sid) in recipients {
                    Replication::reliable(
                        target,
                        sid.as_deref(),
                        ServerMessage::Chat(line.clone()),
                        world,
                        sessions,
                    );
                }
            }
            Err(reason) => Replication::reliable(
                peer,
                sender.id.as_deref(),
                ServerMessage::ChatError(DetailText(reason.into())),
                world,
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
        world: &mut World,
        sessions: &mut Sessions,
    ) {
        if session.baseline != 1 {
            return;
        }
        for line in &self.messages {
            Replication::reliable(
                peer,
                session.id.as_deref(),
                ServerMessage::Chat(line.clone()),
                world,
                sessions,
            );
        }
        let id = session.player_id();
        if let Some(player) = world.players.get(&id) {
            let name = player.display_name.clone();
            if self.announced.insert(id, name.clone()).is_none() {
                self.presence(
                    name,
                    true,
                    Some((peer, session.id.clone())),
                    world,
                    sessions,
                );
            }
        }
    }
    fn presence(
        &mut self,
        username: DisplayName,
        joined: bool,
        extra: Option<(PeerKey, Option<String>)>,
        world: &mut World,
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
            world,
            sessions,
        );
    }
    pub(crate) fn elimination(
        &mut self,
        killer: u32,
        victim: u32,
        extra: (PeerKey, Option<String>),
        world: &mut World,
        sessions: &mut Sessions,
    ) {
        let username = world.players[&killer].display_name.clone();
        let text = ChatText(format!(
            "{} eliminated {}",
            username.0, world.players[&victim].display_name.0
        ));
        self.system_message(username, text, Some(extra), world, sessions);
    }
    fn system_message(
        &mut self,
        username: DisplayName,
        text: ChatText,
        extra: Option<(PeerKey, Option<String>)>,
        world: &mut World,
        sessions: &mut Sessions,
    ) {
        let Some(id) = self.next_id.checked_add(1) else {
            return;
        };
        self.next_id = id;
        let line = ChatLine {
            system: Some(true),
            id: EventId(id),
            username: username.clone(),
            text,
            server_tick: world.tick,
        };
        if self.messages.len() == 32 {
            self.messages.pop_front();
        }
        self.messages.push_back(line.clone());
        let recipients: Vec<_> = sessions
            .peers
            .iter()
            .filter(|(_, s)| s.player.is_some())
            .map(|(&p, s)| (p, s.id.clone()))
            .chain(extra)
            .collect();
        for (peer, sid) in recipients {
            Replication::reliable(
                peer,
                sid.as_deref(),
                ServerMessage::Chat(line.clone()),
                world,
                sessions,
            );
        }
    }
    pub(crate) fn on_left(&mut self, player: u32, world: &mut World, sessions: &mut Sessions) {
        if let Some(name) = self.announced.remove(&player) {
            let name = world
                .players
                .get(&player)
                .map_or(name, |p| p.display_name.clone());
            self.presence(name, false, None, world, sessions);
        }
        self.disconnect(player);
    }
    fn send(
        &mut self,
        player: u32,
        username: DisplayName,
        text: ChatText,
        tick: u32,
        now: u64,
    ) -> Result<ChatLine, &'static str> {
        if !protocol::valid_text("ChatText", &text.0) {
            return Err("Chat must contain 1–256 characters and at most 512 bytes.");
        }
        if self
            .last_sent
            .get(&player)
            .is_some_and(|last| now.saturating_sub(*last) < 1000)
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
        self.last_sent.insert(player, now);
        if self.messages.len() == 32 {
            self.messages.pop_front();
        }
        self.messages.push_back(line.clone());
        Ok(line)
    }
    pub(crate) fn disconnect(&mut self, player: u32) {
        self.last_sent.remove(&player);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_is_bounded_rate_limited_and_cleans_disconnected_players() {
        let mut chat = ChatSystem::default();
        for n in 0..40 {
            chat.send(
                1,
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
                1,
                DisplayName("Guest One".into()),
                ChatText("Too soon".into()),
                40,
                39001
            )
            .is_err()
        );
        assert_eq!(chat.messages.len(), 32);
        chat.disconnect(1);
        assert!(chat.last_sent.is_empty());
    }
}
