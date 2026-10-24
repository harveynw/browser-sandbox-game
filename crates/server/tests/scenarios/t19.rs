use super::*;
#[test]
fn recycled_generations_ignore_old_data_close_and_outputs_across_twelve_rejoins() {
    let (mut w, mut a, mut t) = setup();
    let mut cookie = None;
    let mut old = None;
    for generation in 1..=12 {
        let p = PeerKey { id: 0, generation };
        let saved = join(&mut w, &mut a, &mut t, p, cookie.as_deref());
        ready(&mut w, &mut a, &mut t, p);
        let player = w.sessions[&p].player.unwrap();
        if let Some(previous) = old {
            t.events.push_back(TransportEvent::Closed {
                peer: previous,
                reason: "late old close",
            });
            t.events.push_back(TransportEvent::Message {
                peer: previous,
                message: ClientEnvelope {
                    v: ProtocolVersion(2),
                    world_id: Some(OpaqueId(w.id.clone())),
                    session_id: Some(OpaqueId("old-session".into())),
                    message: ClientMessage::Leave(Empty {}),
                },
            });
            step(&mut w, &mut a, &mut t);
            assert_eq!(w.sessions[&p].player, Some(player));
            assert_eq!(w.players.len(), 1);
        }
        let origin = w.players[&player.0].state.motor.clone();
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
                (w.players[&player.0].state.motor.position.0[axis].0 - origin.position.0[axis].0)
                    .abs()
                    < 0.0001
            );
        }
        assert_eq!(w.sessions[&p].epoch, 2);
        assert!(w.sessions[&p].inputs.is_empty());
        send(&w, &mut t, p, ClientMessage::Leave(Empty {}));
        step(&mut w, &mut a, &mut t);
        assert!(w.players.is_empty());
        assert!(w.sessions.is_empty());
        assert!(w.vehicles.values().all(|v| v.state.driver_id.is_none()));
        t.sent.clear();
        old = Some(p);
        cookie = Some(saved);
    }
}
