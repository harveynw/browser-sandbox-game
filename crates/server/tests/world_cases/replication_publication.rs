use super::*;
use crate::{ecs::TickOutcome, ecs::runtime::Scratch, ecs::state::Clock, replication::publication};
use bevy_ecs::prelude::Schedule;

fn publication_fixture() -> (bevy_ecs::world::World, Schedule) {
    let (mut w, mut profiles, mut transport) = setup();
    for p in [peer(0), peer(1)] {
        join(&mut w, &mut profiles, &mut transport, p, None);
        ready(&mut w, &mut profiles, &mut transport, p);
    }
    let Fixture {
        core, mut sessions, ..
    } = w.fixture.take().unwrap();
    sessions.output.clear();
    let mut host = crate::ecs::state::install(core);
    host.insert_resource(sessions);
    host.init_resource::<Scratch>();
    host.init_resource::<TickOutcome>();
    host.init_resource::<publication::Publication>();
    let mut schedule = Schedule::default();
    schedule.add_systems(crate::ecs::checkpoints::publication());
    (host, schedule)
}
#[test]
fn publication_preserves_cadence_visibility_order_and_per_peer_batch_order() {
    let (mut host, mut schedule) = publication_fixture();
    for tick in [1, 3, 30] {
        host.resource_mut::<Clock>().0 = tick;
        {
            let mut sessions = host.resource_mut::<Sessions>();
            sessions.output.clear();
            // Force a fresh show in every run so ordering is observable at all cadences.
            let ids: Vec<_> = sessions
                .peers
                .values()
                .map(|s| s.player.unwrap().0)
                .collect();
            for session in sessions.peers.values_mut() {
                for id in &ids {
                    session.visible.remove(id);
                }
            }
        }
        schedule.run(&mut host);
        host.resource_mut::<TickOutcome>().finish().unwrap();
        let sessions = host.resource::<Sessions>();
        for p in [peer(0), peer(1)] {
            let output: Vec<_> = sessions
                .output
                .iter()
                .filter(|(peer, _, _)| *peer == p)
                .collect();
            let show = output
                .iter()
                .position(|(_, _, e)| matches!(e.message, ServerMessage::EntityShow(_)))
                .unwrap();
            let owner = output
                .iter()
                .position(|(_, _, e)| matches!(e.message, ServerMessage::OwnerState(_)));
            let snapshot = output
                .iter()
                .position(|(_, _, e)| matches!(e.message, ServerMessage::Snapshot(_)));
            let map = output
                .iter()
                .position(|(_, _, e)| matches!(e.message, ServerMessage::PlayerMap(_)));
            let status = output
                .iter()
                .position(|(_, _, e)| matches!(e.message, ServerMessage::PlayerStatus(_)));
            assert_eq!(owner.is_some(), tick % 3 == 0);
            assert_eq!(snapshot.is_some(), tick % 3 == 0);
            assert_eq!(map.is_some(), tick % 30 == 0);
            assert_eq!(status.is_some(), tick % 30 == 0);
            if let Some(owner) = owner {
                assert!(show < owner && owner < snapshot.unwrap());
            }
            if let Some(status) = status {
                assert!(map.unwrap() < show && snapshot.unwrap() < status);
            }
            for (_, delivery, envelope) in output {
                match &envelope.message {
                    ServerMessage::Snapshot(s) => {
                        assert_eq!(*delivery, Delivery::Unreliable);
                        assert_eq!(s.server_tick, tick);
                        assert!(
                            s.records
                                .iter()
                                .all(|r| sessions.peers[&p].visible.get(&r.entity_id.0)
                                    == Some(&r.visibility_id.0))
                        );
                    }
                    ServerMessage::OwnerState(_) => assert_eq!(*delivery, Delivery::Unreliable),
                    _ => assert_eq!(*delivery, Delivery::Reliable),
                }
            }
        }
        let batch_peers: Vec<_> = sessions
            .output
            .iter()
            .filter(|(_, _, e)| {
                matches!(
                    e.message,
                    ServerMessage::Snapshot(_) | ServerMessage::PlayerStatus(_)
                )
            })
            .map(|(peer, _, _)| *peer)
            .collect();
        assert!(batch_peers.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
#[test]
fn failed_status_discards_all_staged_snapshots_and_does_not_replay_next_tick() {
    let (mut host, mut schedule) = publication_fixture();
    host.resource_mut::<Clock>().0 = 30;
    let profile = host
        .resource_mut::<Sessions>()
        .peers
        .get_mut(&peer(1))
        .unwrap()
        .profile
        .take();
    schedule.run(&mut host);
    assert_eq!(
        host.resource_mut::<TickOutcome>().finish(),
        Err("profile missing".into())
    );
    assert!(
        host.resource::<Sessions>()
            .output
            .iter()
            .all(|(_, _, e)| !matches!(
                e.message,
                ServerMessage::Snapshot(_) | ServerMessage::PlayerStatus(_)
            ))
    );
    {
        let mut sessions = host.resource_mut::<Sessions>();
        sessions.peers.get_mut(&peer(1)).unwrap().profile = profile;
        sessions.output.clear();
    }
    host.resource_mut::<Clock>().0 = 31;
    schedule.run(&mut host);
    host.resource_mut::<TickOutcome>().finish().unwrap();
    assert!(
        host.resource::<Sessions>().output.is_empty(),
        "failed publication cannot leak into the next tick"
    );
}
