//! Run explicitly: cargo test -p server --test actual_transport -- --ignored --nocapture --test-threads=1
//! Real UDP impairment requires loopback listeners, so it is separate from sandboxed checks.
#[path = "support/rtc.rs"]
mod rtc;
use protocol::*;
use rtc::{Peer, Result};
use server::transport::*;
use std::{
    collections::BTreeSet,
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};
fn ping(nonce: u32) -> Vec<u8> {
    encode_client(&ClientEnvelope {
        v: ProtocolVersion(3),
        world_id: Some(OpaqueId("diagnostic".into())),
        session_id: Some(OpaqueId("session".into())),
        message: ClientMessage::Ping(Ping {
            nonce,
            client_time_ms: Milliseconds(0.0),
        }),
    })
    .unwrap()
}
// Distinct entities survive the adapter's per-entity snapshot queue. Pong echoes
// are deliberately latest-only and cannot measure individual datagram delivery.
fn probe_snapshot(nonce: u32) -> ServerEnvelope {
    server::replication::snapshot(
        "diagnostic",
        "session",
        0,
        vec![StateRecord {
            entity_id: EntityId(nonce),
            visibility_id: VisibilityId(1),
            state: EntityState::Character(CharacterState {
                motor: sim_core::spawn_character([0.0, 0.0, 0.0], 0.0).unwrap(),
                health: CharacterHealth(100),
                mode: CharacterMode::OnFoot,
                vehicle_id: None,
            }),
        }],
    )
}
async fn receipt(client: &mut Peer, reliable: bool, nonce: u32) -> Result {
    tokio::time::timeout(rtc::DEADLINE, async {
        loop {
            let bytes = client.receive(reliable).await?;
            let envelope = decode_server(
                &bytes,
                if reliable {
                    Delivery::Reliable
                } else {
                    Delivery::Unreliable
                },
            )?;
            if matches!(envelope.message,ServerMessage::Pong(p) if p.nonce==nonce) {
                return Result::Ok(());
            }
        }
    })
    .await?
}
struct Pump {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Drop for Pump {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            w.join().unwrap();
        }
    }
}
#[tokio::test(flavor = "current_thread")]
#[ignore = "binds real loopback TCP/UDP; run explicit actual transport suite"]
async fn real_rtc_backpressure_and_rapid_kill_reconnect() -> Result {
    run_transport(LossProfile::Off).await
}
#[tokio::test(flavor = "current_thread")]
#[ignore = "binds real loopback TCP/UDP; run explicit actual transport suite"]
async fn real_rtc_loss_and_blackout_recovery() -> Result {
    run_transport(LossProfile::Independent).await
}
#[tokio::test(flavor = "current_thread")]
#[ignore = "binds real loopback TCP/UDP; run explicit actual transport suite"]
async fn real_rtc_periodic_loss_recovers_without_unbounded_sends() -> Result {
    run_transport(LossProfile::Periodic).await
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LossProfile {
    Off,
    Independent,
    Periodic,
}
async fn run_transport(profile: LossProfile) -> Result {
    let loss = profile != LossProfile::Off;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(listener);
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let (tx, rx) = std::sync::mpsc::sync_channel(16);
    let (commands, command_rx) =
        std::sync::mpsc::sync_channel::<(PeerKey, std::sync::mpsc::SyncSender<SendOutcome>)>(4);
    let ingress = Arc::new(AtomicUsize::new(0));
    let received = ingress.clone();
    let replaced = Arc::new(AtomicUsize::new(0));
    let coalesced = replaced.clone();
    let worker = thread::spawn(move || {
        let mut transport = Network::start(&address.to_string()).expect("transport start");
        while !stopping.load(Ordering::Relaxed) {
            transport.poll();
            while let Ok((old, ack)) = command_rx.try_recv() {
                let message = ServerEnvelope {
                    v: ProtocolVersion(3),
                    world_id: Some(OpaqueId("diagnostic".into())),
                    session_id: Some(OpaqueId("session".into())),
                    message: ServerMessage::Pong(Pong {
                        nonce: 0,
                        client_time_ms: Milliseconds(0.0),
                        server_tick: 0,
                        server_time_ms: Milliseconds(0.0),
                    }),
                };
                let outcome = transport.send(old, Delivery::Reliable, message);
                transport.disconnect(old, "late old kill");
                ack.send(outcome).unwrap();
            }
            for event in transport.drain(128) {
                match event {
                    TransportEvent::Opened(p) => {
                        tx.try_send(p).unwrap();
                    }
                    TransportEvent::Message { peer, message } => {
                        if let ClientMessage::Ping(p) = message.message {
                            if (10..60).contains(&p.nonce) {
                                received.fetch_add(1, Ordering::Relaxed);
                            }
                            let reply = ServerEnvelope {
                                v: ProtocolVersion(3),
                                world_id: Some(OpaqueId("diagnostic".into())),
                                session_id: Some(OpaqueId("session".into())),
                                message: ServerMessage::Pong(Pong {
                                    nonce: p.nonce,
                                    client_time_ms: p.client_time_ms,
                                    server_tick: 0,
                                    server_time_ms: Milliseconds(0.0),
                                }),
                            };
                            transport.send(peer, Delivery::Reliable, reply.clone());
                            let probe = loss && (10..60).contains(&p.nonce);
                            let unreliable = if probe {
                                probe_snapshot(p.nonce)
                            } else {
                                reply
                            };
                            if transport.send(peer, Delivery::Unreliable, unreliable)
                                == SendOutcome::ReplacedStale
                                && probe
                            {
                                coalesced.fetch_add(1, Ordering::Relaxed);
                            }
                            if p.nonce == 999 {
                                transport.disconnect(peer, "test kill");
                            }
                        }
                    }
                    _ => {}
                }
            }
            transport.flush();
            thread::sleep(Duration::from_millis(2));
        }
    });
    let _pump = Pump {
        stop,
        worker: Some(worker),
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut previous: Option<PeerKey> = None;
    for cycle in 0..6 {
        let mut client = Peer::connect(address).await?;
        let key = rx.recv_timeout(Duration::from_secs(1))?;
        if let Some(old) = previous {
            assert_eq!(old.id, key.id);
            assert!(key.generation > old.generation);
            let (ack, outcome) = std::sync::mpsc::sync_channel(1);
            commands.send((old, ack))?;
            assert_eq!(
                outcome.recv_timeout(Duration::from_secs(1))?,
                SendOutcome::Closed
            );
        }
        client.send(true, ping(1)).await?;
        receipt(&mut client, true, 1).await?;
        client.send(false, ping(2)).await?;
        receipt(&mut client, false, 2).await?;
        if cycle == 5 {
            client.check_backpressure(&ping(220)).await?;
            client.close().await?;
            break;
        }
        if cycle == 0 {
            let mut rtc_receipts = None;
            if loss {
                let relay_before = *client.relay.stats.lock().unwrap();
                client
                    .relay
                    .periodic_loss
                    .store(profile == LossProfile::Periodic, Ordering::Relaxed);
                client.relay.loss_one_in.store(7, Ordering::Relaxed);
                // Measure unique nonce progress with paced requests and a bounded
                // settling interval, rather than counting short receipt windows.
                let mut receipts = BTreeSet::new();
                let mut nonce = 10;
                let mut pace = tokio::time::interval(Duration::from_millis(40));
                let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep_until(deadline) => break,
                        _ = pace.tick(), if nonce < 60 => {
                            client.send(false, ping(nonce)).await?;
                            nonce += 1;
                        }
                        reply = client.receive(false) => {
                            let message = decode_server(&reply?, Delivery::Unreliable)?;
                            if let ServerMessage::Snapshot(snapshot) = message.message {
                                for record in snapshot.records {
                                    assert!((10..60).contains(&record.entity_id.0));
                                    receipts.insert(record.entity_id.0);
                                }
                            }
                        }
                    }
                }
                assert_eq!(nonce, 60, "all impairment probes were sent");
                let count = receipts.len();
                rtc_receipts = Some(count);
                println!(
                    "RTC loss progress unique_receipts={count}/50; server_ingress={}; replaced_probe_records={}; client_outstanding={}; relay={:?}",
                    ingress.load(Ordering::Relaxed),
                    replaced.load(Ordering::Relaxed),
                    client.outstanding().await?,
                    client.relay.stats.lock().unwrap().since(relay_before)
                );
                assert_eq!(
                    replaced.load(Ordering::Relaxed),
                    0,
                    "independent probe records must not coalesce"
                );
                if profile == LossProfile::Independent {
                    assert!(
                        count > 15,
                        "RTC made no progress under independent packet loss"
                    );
                }
                assert!(client.outstanding().await? <= 65536);
                assert!(client.relay.dropped.load(Ordering::Relaxed) > 0);
            }
            client.relay.blocked.store(true, Ordering::Relaxed);
            // WS must survive actual UDP blackout. Polling reliable data continues signalling/I/O.
            client.send(true, ping(100)).await?;
            receipt(&mut client, true, 100).await?;
            let before = client.relay.forwarded.load(Ordering::Relaxed);
            for nonce in 100..110 {
                client.send(false, ping(nonce)).await?;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(client.relay.forwarded.load(Ordering::Relaxed), before);
            client.relay.blocked.store(false, Ordering::Relaxed);
            client.relay.loss_one_in.store(0, Ordering::Relaxed);
            client.send(false, ping(200)).await?;
            receipt(&mut client, false, 200).await?;
            println!(
                "Actual RTC: loss_profile={profile:?}; unique_receipts={rtc_receipts:?}; dropped={}, forwarded={}; UDP blackout retained WS and RTC recovered",
                client.relay.dropped.load(Ordering::Relaxed),
                client.relay.forwarded.load(Ordering::Relaxed)
            );
        }
        client.send(true, ping(999)).await?;
        receipt(&mut client, true, 999).await?;
        // Observe server close before recycling the ID; no fabricated transport lifecycle events.
        assert!(client.receive(true).await.is_err());
        let _ = client.close().await;
        previous = Some(key);
        println!("Actual kill/reconnect cycle={cycle}, peer={key:?}");
    }
    Ok(())
}
