use super::*;
use server::components::Player as PlayerComponent;
use server::components::Vehicle as VehicleComponent;
#[test]
fn recycled_generations_ignore_old_data_close_and_outputs_across_twelve_rejoins() {
    let (mut w, mut a, mut t) = setup();
    let mut cookie = None;
    let mut old = None;
    for generation in 1..=12 {
        let p = PeerKey { id: 0, generation };
        let saved = join(&mut w, &mut a, &mut t, p, cookie.as_deref());
        ready(&mut w, &mut a, &mut t, p);
        let player = w.sessions.peers[&p].player.unwrap();
        if let Some(previous) = old {
            t.events.push_back(TransportEvent::Closed {
                peer: previous,
                reason: "late old close",
            });
            t.events.push_back(TransportEvent::Message {
                peer: previous,
                message: ClientEnvelope {
                    v: ProtocolVersion(4),
                    world_id: Some(OpaqueId(w.id.clone())),
                    session_id: Some(OpaqueId("old-session".into())),
                    message: ClientMessage::Leave(Empty {}),
                },
            });
            step(&mut w, &mut a, &mut t);
            assert_eq!(w.sessions.peers[&p].player, Some(player));
            assert_eq!(w.entities.view::<PlayerComponent>().len(), 1);
        }
        let origin = w.entities.view::<PlayerComponent>()[&player.0]
            .state
            .motor
            .clone();
        // An old control epoch after a baseline can neither move nor jump the new owner.
        send(
            &w,
            &mut t,
            p,
            ClientMessage::ResyncRequest(ResyncRequest {
                reason: ResyncReason::Stale,
            }),
        );
        step(&mut w, &mut a, &mut t);
        ready(&mut w, &mut a, &mut t, p);
        for _ in 0..3 {
            batch(&w, &mut t, p, w.tick + 1, 32767, 32767, 1);
            step(&mut w, &mut a, &mut t);
        }
        for axis in 0..3 {
            assert!(
                (w.entities.view::<PlayerComponent>()[&player.0]
                    .state
                    .motor
                    .position
                    .0[axis]
                    .0
                    - origin.position.0[axis].0)
                    .abs()
                    < 0.0001
            );
        }
        assert_eq!(w.sessions.peers[&p].epoch, 2);
        assert!(w.sessions.peers[&p].inputs.is_empty());
        send(&w, &mut t, p, ClientMessage::Leave(Empty {}));
        step(&mut w, &mut a, &mut t);
        assert!(w.entities.view::<PlayerComponent>().is_empty());
        assert!(w.sessions.peers.is_empty());
        assert!(
            w.entities
                .view::<VehicleComponent>()
                .values()
                .all(|v| v.state.driver_id.is_none())
        );
        t.sent.clear();
        old = Some(p);
        cookie = Some(saved);
    }
}

#[test]
fn reliable_budget_failure_retires_actor_and_hides_it_before_flush_finishes() {
    let (mut w, mut profiles, mut t) = setup();
    let failed = peer(0);
    let observer = peer(1);
    for p in [failed, observer] {
        join(&mut w, &mut profiles, &mut t, p, None);
        ready(&mut w, &mut profiles, &mut t, p);
    }
    let player = w.sessions.peers[&failed].player.unwrap();
    assert!(w.sessions.peers[&observer].visible.contains_key(&player.0));
    t.sent.clear();
    w.sessions.peers.get_mut(&failed).unwrap().egress_budget =
        server::replication::Budget::new(0, 0);
    for nonce in [10, 11] {
        send(
            &w,
            &mut t,
            failed,
            ClientMessage::Ping(Ping {
                nonce,
                client_time_ms: Milliseconds(0.0),
            }),
        );
    }
    step(&mut w, &mut profiles, &mut t);
    assert!(!w.sessions.peers.contains_key(&failed));
    assert!(w.entities.get::<PlayerComponent>(player).is_none());
    assert!(!w.sessions.peers[&observer].visible.contains_key(&player.0));
    assert!(t.closed.contains(&failed));
    assert!(
        !t.sent.iter().any(|(peer, _, _)| *peer == failed),
        "do not send the tail after a reliable failure"
    );
    assert!(t.sent.iter().any(|(peer, _, envelope)| *peer == observer && matches!(&envelope.message, ServerMessage::EntityHide(hide) if hide.entity_id == player && hide.reason == HideReason::Disconnected)));
    assert!(w.sessions.output.is_empty());
}
