//! Run explicitly: cargo test -p server --test actual_transport -- --ignored --nocapture
//! Real UDP impairment requires loopback listeners, so it is separate from sandboxed checks.
#[path = "support/rtc.rs"]
mod rtc;
use protocol::*;
use rtc::{Peer, Result};
use server::transport::*;
use std::{
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
fn ping(nonce: u32) -> Vec<u8> {
    encode_client(&ClientEnvelope {
        v: ProtocolVersion(2),
        world_id: Some(OpaqueId("diagnostic".into())),
        session_id: Some(OpaqueId("session".into())),
        message: ClientMessage::Ping(Ping {
            nonce,
            client_time_ms: Milliseconds(0.0),
        }),
    })
    .unwrap()
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
async fn real_rtc_datagram_loss_recovery_and_rapid_kill_reconnect() -> Result {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(listener);
    let mut transport = NetCrateTransport::start(&address.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let (tx, rx) = std::sync::mpsc::sync_channel(16);
    let (commands, command_rx) =
        std::sync::mpsc::sync_channel::<(PeerKey, std::sync::mpsc::SyncSender<SendOutcome>)>(4);
    let worker = thread::spawn(move || {
        while !stopping.load(Ordering::Relaxed) {
            while let Ok((old, ack)) = command_rx.try_recv() {
                let message = ServerEnvelope {
                    v: ProtocolVersion(2),
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
                            let reply = ServerEnvelope {
                                v: ProtocolVersion(2),
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
                            transport.send(peer, Delivery::Unreliable, reply);
                            if p.nonce == 999 {
                                transport.disconnect(peer, "test kill");
                            }
                        }
                    }
                    _ => {}
                }
            }
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
            client.relay.drop_every.store(7, Ordering::Relaxed);
            let mut rtc_receipts = 0;
            for nonce in 10..60 {
                client.send(false, ping(nonce)).await?;
                if tokio::time::timeout(Duration::from_millis(75), client.receive(false))
                    .await
                    .is_ok_and(|r| r.is_ok())
                {
                    rtc_receipts += 1;
                }
            }
            println!("T19 loss progress receipts={rtc_receipts}");
            assert!(rtc_receipts > 15, "RTC made no progress under packet loss");
            assert!(client.relay.dropped.load(Ordering::Relaxed) > 0);
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
            client.relay.drop_every.store(0, Ordering::Relaxed);
            client.send(false, ping(200)).await?;
            receipt(&mut client, false, 200).await?;
            println!(
                "T19 actual RTC: receipts={rtc_receipts}/50 under every-seventh-datagram loss; dropped={}, forwarded={}; UDP blackout retained WS and RTC recovered",
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
        println!("T19 actual kill/reconnect cycle={cycle}, peer={key:?}");
    }
    Ok(())
}
