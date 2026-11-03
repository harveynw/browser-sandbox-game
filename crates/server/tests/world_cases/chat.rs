use super::*;
#[test]
fn chat_is_reliable_authoritative_and_replayed_to_new_players() {
    let (mut w, mut profiles, mut t) = setup();
    let p = peer(1);
    let q = peer(2);
    join(&mut w, &mut profiles, &mut t, p, None);
    ready(&mut w, &mut profiles, &mut t, p);
    join(&mut w, &mut profiles, &mut t, q, None);
    ready(&mut w, &mut profiles, &mut t, q);
    let id = w.sessions.peers[&p].player_id();
    w.core.players.get_mut(&id).unwrap().display_name = DisplayName("Alice".into());
    t.sent.clear();
    send(
        &w,
        &mut t,
        p,
        ClientMessage::Chat(ChatText("Hello *world*!".into())),
    );
    step(&mut w, &mut profiles, &mut t);
    for target in [p, q] {
        assert!(t.sent.iter().any(|(peer,delivery,m)|*peer==target&&*delivery==Delivery::Reliable&&matches!(&m.message,ServerMessage::Chat(line) if line.username.0=="Alice"&&line.text.0=="Hello *world*!")));
    }
    t.sent.clear();
    send(&w, &mut t, p, ClientMessage::Chat(ChatText("Spam".into())));
    step(&mut w, &mut profiles, &mut t);
    assert!(
        !t.sent
            .iter()
            .any(|(_, _, message)| matches!(message.message, ServerMessage::Chat(_)))
    );
    assert!(
        t.sent
            .iter()
            .any(|(peer, _, m)| *peer == p && matches!(m.message, ServerMessage::ChatError(_)))
    );
    let r = peer(3);
    join(&mut w, &mut profiles, &mut t, r, None);
    t.sent.clear();
    ready(&mut w, &mut profiles, &mut t, r);
    assert!(t.sent.iter().any(|(peer, delivery, m)| *peer == r
        && *delivery == Delivery::Reliable
        && matches!(&m.message,ServerMessage::Chat(line) if line.username.0=="Alice" && line.text.0=="Hello *world*!")));
    send(
        &w,
        &mut t,
        q,
        ClientMessage::ResyncRequest(ResyncRequest {
            reason: ResyncReason::Stale,
        }),
    );
    step(&mut w, &mut profiles, &mut t);
    assert_eq!(w.sessions.peers[&q].phase, SessionPhase::Loading);
    t.sent.clear();
    send(
        &w,
        &mut t,
        p,
        ClientMessage::Chat(ChatText("During resync".into())),
    );
    t.step(&mut w, &mut profiles, 2000);
    assert!(t.sent.iter().any(|(peer, delivery, m)| *peer == q
        && *delivery == Delivery::Reliable
        && matches!(&m.message,ServerMessage::Chat(line) if line.text.0=="During resync")));
    w.disconnect(p, &mut profiles);
}

#[test]
fn presence_is_broadcast_once_on_ready_and_leave_but_not_loading_or_resync() {
    let (mut w, mut profiles, mut t) = setup();
    let p = peer(1);
    let q = peer(2);
    let loading = peer(3);
    join(&mut w, &mut profiles, &mut t, p, None);
    let id = w.sessions.peers[&p].player_id();
    w.core.players.get_mut(&id).unwrap().display_name = DisplayName("Alice".into());
    ready(&mut w, &mut profiles, &mut t, p);
    join(&mut w, &mut profiles, &mut t, q, None);
    t.sent.clear();
    ready(&mut w, &mut profiles, &mut t, q);
    for target in [p, q] {
        assert_eq!(t.sent.iter().filter(|(peer, delivery, m)| *peer == target && *delivery == Delivery::Reliable && matches!(&m.message, ServerMessage::Chat(line) if line.system == Some(true) && line.text.0 == "Test Guest has joined the game")).count(), 1);
    }
    // A failed/loading guest never appeared in gameplay and must not announce a departure.
    join(&mut w, &mut profiles, &mut t, loading, None);
    t.sent.clear();
    w.disconnect(loading, &mut profiles);
    step(&mut w, &mut profiles, &mut t);
    assert!(
        !t.sent
            .iter()
            .any(|(_, _, m)| matches!(&m.message, ServerMessage::Chat(_)))
    );
    send(
        &w,
        &mut t,
        p,
        ClientMessage::ResyncRequest(ResyncRequest {
            reason: ResyncReason::Stale,
        }),
    );
    step(&mut w, &mut profiles, &mut t);
    t.sent.clear();
    ready(&mut w, &mut profiles, &mut t, p);
    assert!(
        !t.sent
            .iter()
            .any(|(_, _, m)| matches!(&m.message, ServerMessage::Chat(_)))
    );
    t.sent.clear();
    w.disconnect(p, &mut profiles);
    w.disconnect(p, &mut profiles);
    step(&mut w, &mut profiles, &mut t);
    assert_eq!(t.sent.iter().filter(|(peer,delivery,m)| *peer == q && *delivery == Delivery::Reliable && matches!(&m.message, ServerMessage::Chat(line) if line.system == Some(true) && line.text.0 == "Alice has left the game")).count(), 1);
}
