use super::*;
use crate::{
    components::{GameId, Player},
    ecs::{checkpoints, runtime::TickIo, state::Clock},
    gameplay_messages::{PlayerBecameReady, PlayerLeaving},
    sessions::native::{DisconnectRequested, finish_leaving},
};
use bevy_ecs::prelude::*;

fn host(ready_players: bool) -> (bevy_ecs::world::World, [PeerKey; 2]) {
    let (mut w, mut profiles, mut transport) = setup();
    let peers = [peer(0), peer(1)];
    for p in peers {
        join(&mut w, &mut profiles, &mut transport, p, None);
        if ready_players {
            ready(&mut w, &mut profiles, &mut transport, p);
        }
    }
    let Fixture {
        core,
        mut sessions,
        systems,
        ..
    } = w.fixture.take().unwrap();
    sessions.output.clear();
    let mut host = crate::ecs::state::install(core);
    crate::ecs::state::install_systems(&mut host, systems);
    host.insert_resource(sessions);
    host.insert_non_send(TickIo {
        profiles,
        now: 0,
        allow_joins: true,
    });
    (host, peers)
}

#[derive(Resource, Default)]
struct Departures(Vec<EntityId>);
fn inspect_before_despawn(
    mut messages: MessageReader<PlayerLeaving>,
    players: Query<&GameId, With<Player>>,
    sessions: Res<Sessions>,
    mut departures: ResMut<Departures>,
) {
    for message in messages.read() {
        assert!(!sessions.peers.contains_key(&message.peer));
        assert_eq!(
            players.get(message.entity.unwrap()).unwrap().0,
            message.player
        );
        assert!(message.name.is_some());
        departures.0.push(message.player);
    }
}

#[test]
fn departure_subscribers_run_before_despawn_and_duplicate_requests_do_not_replay() {
    let (mut host, [p, q]) = host(true);
    let id = host.resource::<Sessions>().peers[&p].player.unwrap();
    let entity = host.resource::<crate::ecs::EntityIndex>().0[&id.0];
    let tick = host.resource::<Clock>().0;
    host.init_resource::<Departures>();
    let mut schedule = Schedule::default();
    schedule.add_systems(checkpoints::disconnects());
    schedule.add_systems(
        inspect_before_despawn
            .after(crate::systems::chat::player_leaving)
            .before(finish_leaving),
    );
    for _ in 0..2 {
        host.write_message(DisconnectRequested {
            peer: p,
            reason: "test",
            tick,
        });
    }
    schedule.run(&mut host);
    assert_eq!(host.resource::<Departures>().0, [id]);
    assert!(host.get::<Player>(entity).is_none());
    assert!(
        !host
            .resource::<crate::ecs::EntityIndex>()
            .0
            .contains_key(&id.0)
    );
    assert!(host.resource::<Messages<PlayerLeaving>>().is_empty());
    let departures = |host: &bevy_ecs::world::World| {
        host.resource::<Sessions>().output.iter()
        .filter(|(peer, _, m)| *peer == q && matches!(&m.message, ServerMessage::Chat(line) if line.text.0.ends_with("has left the game"))).count()
    };
    assert_eq!(departures(&host), 1);
    schedule.run(&mut host);
    assert_eq!(host.resource::<Departures>().0, [id]);
    assert_eq!(departures(&host), 1);
}

#[test]
fn readiness_subscriber_rejects_stale_identity_and_resync_and_retires_messages() {
    let (mut host, [p, _]) = host(false);
    let tick = host.resource::<Clock>().0;
    let (player, session) = {
        let mut sessions = host.resource_mut::<Sessions>();
        let s = sessions.peers.get_mut(&p).unwrap();
        s.phase = SessionPhase::Playing;
        (s.player.unwrap(), s.id.clone().unwrap())
    };
    let entity = host.resource::<crate::ecs::EntityIndex>().0[&player.0];
    let message = PlayerBecameReady {
        peer: p,
        player,
        entity,
        session,
        tick,
    };
    let mut schedule = Schedule::default();
    schedule.add_systems(checkpoints::ready());
    let mut stale = message.clone();
    stale.session.push_str("-old");
    host.write_message(stale);
    schedule.run(&mut host);
    assert!(host.resource::<Sessions>().output.is_empty());
    host.resource_mut::<Sessions>()
        .peers
        .get_mut(&p)
        .unwrap()
        .baseline = 2;
    host.write_message(message.clone());
    schedule.run(&mut host);
    assert!(host.resource::<Sessions>().output.is_empty());
    host.resource_mut::<Sessions>()
        .peers
        .get_mut(&p)
        .unwrap()
        .baseline = 1;
    host.write_message(message.clone());
    schedule.run(&mut host);
    assert!(!host.resource::<Sessions>().output.is_empty());
    assert!(host.resource::<Messages<PlayerBecameReady>>().is_empty());
    host.resource_mut::<Sessions>().output.clear();
    host.write_message(message);
    schedule.run(&mut host);
    assert!(host.resource::<Sessions>().output.is_empty());
}
