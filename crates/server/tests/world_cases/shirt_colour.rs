use super::*;
use server::entities::player::Player as PlayerComponent;
fn join_with_colour(
    w: &mut World,
    a: &mut Profiles,
    t: &mut FakeTransport,
    p: PeerKey,
    cookie: Option<&str>,
    colour: u32,
) -> String {
    let (_, new) = a.session(cookie).unwrap();
    let cookie = new.or_else(|| cookie.map(str::to_owned)).unwrap();
    let ticket = a.issue(Some(&cookie), u64::from(w.tick) * 34).unwrap();
    t.events.push_back(TransportEvent::Opened(p));
    step(w, a, t);
    let build = w.build();
    send(
        w,
        t,
        p,
        ClientMessage::ClientHello(ClientHello {
            tshirt_colour: TshirtColour(colour),
            protocol_version: ProtocolVersion(4),
            build_id: build.build_id,
            content_hash: build.content_hash,
            sim_config_hash: build.sim_config_hash,
            join_ticket: OpaqueId(ticket),
            display_name: DisplayName("Test Guest".into()),
        }),
    );
    step(w, a, t);
    cookie
}
// Committed appearance is shared by the profile, entity and every observer's baseline.
#[test]
fn tshirt_colour_is_committed_and_replicated_to_other_guests_and_resyncs() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    let q = peer(2);
    let cookie = join_with_colour(&mut w, &mut a, &mut t, p, None, 0x1256ab);
    ready(&mut w, &mut a, &mut t, p);
    let id = w.sessions.peers[&p].player_id();
    assert_eq!(
        a.profile(Some(&cookie)).unwrap().tshirt_colour,
        TshirtColour(0x1256ab)
    );
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&id].tshirt_colour,
        TshirtColour(0x1256ab)
    );
    t.sent.clear();
    join_with_colour(&mut w, &mut a, &mut t, q, None, 0xff0044);
    ready(&mut w, &mut a, &mut t, q);
    assert!(t.sent.iter().any(|(peer,delivery,m)| *peer==q && *delivery==Delivery::Reliable && matches!(&m.message,ServerMessage::EntityShow(v) if v.descriptor.id.0==id && v.descriptor.tshirt_colour==Some(TshirtColour(0x1256ab)))));
    assert!(t.sent.iter().any(|(peer,_,m)| *peer==p && matches!(&m.message,ServerMessage::EntityShow(v) if v.descriptor.tshirt_colour==Some(TshirtColour(0xff0044)))));
    t.sent.clear();
    send(
        &w,
        &mut t,
        q,
        ClientMessage::ResyncRequest(ResyncRequest {
            reason: ResyncReason::Stale,
        }),
    );
    step(&mut w, &mut a, &mut t);
    assert!(t.sent.iter().any(|(peer,_,m)| *peer==q && matches!(&m.message,ServerMessage::EntityShow(v) if v.descriptor.id.0==id && v.descriptor.tshirt_colour==Some(TshirtColour(0x1256ab)))));
    w.disconnect(p, &mut a);
    assert_eq!(
        a.profile(Some(&cookie)).unwrap().tshirt_colour,
        TshirtColour(0x1256ab)
    );
}

// A menu edit changes owner and observer descriptors without a control transition.
#[test]
fn live_tshirt_change_refreshes_every_observer_without_losing_inputs_or_epoch() {
    let (mut w, mut a, mut t) = setup();
    let p = peer(1);
    let q = peer(2);
    let cookie = join_with_colour(&mut w, &mut a, &mut t, p, None, 0x1256ab);
    ready(&mut w, &mut a, &mut t, p);
    join_with_colour(&mut w, &mut a, &mut t, q, None, 0xff0044);
    ready(&mut w, &mut a, &mut t, q);
    let id = w.sessions.peers[&p].player_id();
    let epoch = w.sessions.peers[&p].epoch;
    let before_p = w.sessions.peers[&p].visible[&id];
    let before_q = w.sessions.peers[&q].visible[&id];
    let future = w.tick + 2;
    batch(&w, &mut t, p, future, 0, 0, epoch);
    a.update_tshirt_colour(Some(&cookie), TshirtColour(0x22cc66))
        .unwrap()
        .unwrap();
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&id].tshirt_colour,
        TshirtColour(0x1256ab)
    );
    t.sent.clear();
    step(&mut w, &mut a, &mut t);
    assert_eq!(
        w.core.entities.view::<PlayerComponent>()[&id].tshirt_colour,
        TshirtColour(0x22cc66)
    );
    assert_eq!(w.sessions.peers[&p].epoch, epoch);
    assert!(w.sessions.peers[&p].inputs.contains_key(&future));
    assert!(w.sessions.peers[&p].visible[&id] > before_p);
    assert!(w.sessions.peers[&q].visible[&id] > before_q);
    for target in [p, q] {
        assert!(t.sent.iter().any(|(peer,delivery,m)| *peer==target && *delivery==Delivery::Reliable && matches!(&m.message,ServerMessage::EntityShow(v) if v.descriptor.id.0==id && v.descriptor.tshirt_colour==Some(TshirtColour(0x22cc66)) && v.baseline_id.is_none())));
    }
    assert!(
        !t.sent
            .iter()
            .any(|(_, _, m)| matches!(m.message, ServerMessage::ControlChanged(_)))
    );
    assert_eq!(
        w.sessions.peers[&p].profile.as_ref().unwrap().tshirt_colour,
        TshirtColour(0x22cc66)
    );
    assert_eq!(
        w.sessions.peers[&q].profile.as_ref().unwrap().tshirt_colour,
        TshirtColour(0xff0044)
    );
}
