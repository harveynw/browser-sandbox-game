use protocol::*;
use server::replication::*;
use std::collections::{BTreeMap, BTreeSet};
fn record(id: u32, visibility: u32) -> StateRecord {
    StateRecord {
        entity_id: EntityId(id),
        visibility_id: VisibilityId(visibility),
        state: EntityState::Character(CharacterState {
            motor: sim_core::spawn_character([id as f32, 0.0, 0.0], 0.0).unwrap(),
            health: CharacterHealth(100),
            mode: CharacterMode::OnFoot,
            vehicle_id: None,
        }),
    }
}
#[test]
fn actual_size_splits_are_independent_and_per_entity_coalescing_does_not_lose_other_packets() {
    let world = "w".repeat(64);
    let session = "s".repeat(64);
    let initial = packets(&world, &session, 10, (1..=96).map(|id| record(id, 1))).unwrap();
    assert!(initial.len() > 1);
    let mut queue = SnapshotBuffer::default();
    for p in &initial {
        assert!(encode_server(p, Delivery::Unreliable).unwrap().len() <= 1100);
        queue.push(p).unwrap();
    }
    queue
        .push(&snapshot(&world, &session, 13, vec![record(1, 1)]))
        .unwrap();
    queue.push(&initial[0]).unwrap();
    queue.hide(2, 1);
    let encoded = queue.drain().unwrap();
    let mut records = BTreeMap::new();
    for bytes in encoded {
        assert!(bytes.len() <= 1100);
        let message = decode_server(&bytes, Delivery::Unreliable).unwrap();
        let ServerMessage::Snapshot(s) = message.message else {
            panic!()
        };
        for r in s.records {
            assert!(records.insert(r.entity_id.0, s.server_tick).is_none());
        }
    }
    assert_eq!(records.len(), 95);
    assert!(!records.contains_key(&2));
    assert_eq!(records[&1], 13);
    assert_eq!(records[&96], 10);
    assert!(queue.drain().unwrap().is_empty());
    queue
        .push(&snapshot(&world, &session, 20, vec![record(1, 3)]))
        .unwrap();
    queue
        .push(&snapshot(&world, &session, 22, vec![record(1, 2)]))
        .unwrap();
    queue
        .push(&snapshot(&world, "new-session", 1, vec![record(3, 1)]))
        .unwrap();
    let out = queue.drain().unwrap();
    let decoded = decode_server(&out[0], Delivery::Unreliable).unwrap();
    assert_eq!(decoded.session_id.unwrap().0, "new-session");
    let ServerMessage::Snapshot(s) = decoded.message else {
        panic!()
    };
    assert_eq!(s.records.len(), 1);
    assert_eq!(s.records[0].entity_id.0, 3);
}
#[test]
fn spatial_hysteresis_self_priority_and_visibility_cap_are_deterministic() {
    let index =
        SpatialIndex::new([(1, [0.0, 0.0]), (2, [170.0, 0.0]), (3, [193.0, 0.0])].into_iter());
    assert_eq!(
        index.relevant([0.0, 0.0], 1, &BTreeMap::new()),
        BTreeSet::from([1])
    );
    assert_eq!(
        index.relevant([0.0, 0.0], 1, &BTreeMap::from([(2, 9), (3, 10)])),
        BTreeSet::from([1, 2])
    );
    let crowd = SpatialIndex::new((1..=120).map(|id| (id, [id as f32, 0.0])));
    let set = crowd.relevant([120.0, 0.0], 120, &BTreeMap::new());
    assert_eq!(set.len(), 96);
    assert!(set.contains(&120));
    assert!(!set.contains(&1));
}
#[test]
fn byte_budget_enforces_burst_refill_and_owner_reserve() {
    let mut budget = Budget::new(60 * 1024, 120 * 1024);
    assert!(budget.allow(120 * 1024 - 1100, 0, 1100));
    assert!(!budget.allow(1, 0, 1100));
    assert!(budget.allow(1100, 0, 0));
    assert!(!budget.allow(1, 0, 0));
    assert!(budget.allow(60 * 1024, 1000, 0));
    assert!(!budget.allow(1, 1000, 0));
    assert!(budget.allow(120 * 1024, 100_000, 0));
    assert!(!budget.allow(1, 100_000, 0));
}

#[test]
fn input_rate_is_a_strict_rolling_second_not_a_refilling_burst() {
    let mut rate = InputRate::default();
    for i in 0..60 {
        assert!(rate.allow(i * 10));
    }
    assert!(!rate.allow(600));
    assert!(!rate.allow(999));
    assert!(rate.allow(1000));
    assert!(!rate.allow(1001));
    assert!(rate.allow(1010));
}
