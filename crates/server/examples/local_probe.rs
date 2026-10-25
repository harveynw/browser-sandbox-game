//! Local launch acceptance probe. A normal guest through the real frontend proxy and WS/RTC.
//! Compiled as an operator example; no probe-only gameplay hooks or transport replacement.
#[path = "../tests/support/rtc.rs"]
mod rtc;
use protocol::*;
use rtc::{Peer, Result};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, timeout},
};

struct Http {
    code: u16,
    body: Value,
    cookie: Option<String>,
}
async fn http(port: u16, path: &str, body: Option<Value>, cookie: Option<&str>) -> Result<Http> {
    timeout(Duration::from_secs(3),async {
        let mut stream=TcpStream::connect(("127.0.0.1",port)).await?;
        let data=body.as_ref().map(Value::to_string).unwrap_or_default();
        let request=format!("{} {path} HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}\r\n{data}",if body.is_some(){"POST"}else{"GET"},data.len(),cookie.map(|c|format!("Cookie: {c}\r\n")).unwrap_or_default());
        stream.write_all(request.as_bytes()).await?;
        let mut bytes=Vec::new();stream.take(512*1024).read_to_end(&mut bytes).await?;
        let split=bytes.windows(4).position(|w|w==b"\r\n\r\n").ok_or("HTTP header missing")?;
        let header=std::str::from_utf8(&bytes[..split])?;
        let code=header.lines().next().and_then(|s|s.split_whitespace().nth(1)).ok_or("HTTP status missing")?.parse()?;
        let cookie=header.lines().find(|s|s.to_ascii_lowercase().starts_with("set-cookie:")).and_then(|s|s.split_once(':')).map(|(_,s)|s.trim().split(';').next().unwrap_or("").to_owned());
        let raw=&bytes[split+4..];
        let payload=if header.to_ascii_lowercase().contains("transfer-encoding: chunked") {unchunk(raw)?}else{raw.to_vec()};
        Ok(Http{code,cookie,body:serde_json::from_slice(&payload)?})
    }).await?
}
fn unchunk(mut bytes: &[u8]) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    loop {
        let line = bytes
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("chunk size missing")?;
        let size = usize::from_str_radix(
            std::str::from_utf8(&bytes[..line])?
                .split(';')
                .next()
                .unwrap_or(""),
            16,
        )?;
        bytes = &bytes[line + 2..];
        if size == 0 {
            return Ok(result);
        }
        if size > bytes.len().saturating_sub(2) || &bytes[size..size + 2] != b"\r\n" {
            return Err("truncated HTTP chunk".into());
        }
        result.extend_from_slice(&bytes[..size]);
        bytes = &bytes[size + 2..];
    }
}
fn save_cookie(path: &Path, cookie: &str) -> Result {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(cookie.as_bytes())?;
    Ok(())
}
async fn send(
    peer: &mut Peer,
    world: &Option<OpaqueId>,
    session: &Option<OpaqueId>,
    message: ClientMessage,
    reliable: bool,
) -> Result {
    peer.send(
        reliable,
        encode_client(&ClientEnvelope {
            v: ProtocolVersion(3),
            world_id: world.clone(),
            session_id: session.clone(),
            message,
        })?,
    )
    .await
}
async fn run(port: u16, cookie_path: &Path) -> Result {
    let mut cookie = std::fs::read_to_string(cookie_path).ok();
    loop {
        let response = http(port, "/api/session", Some(json!({})), cookie.as_deref()).await?;
        if let Some(secret) = response.cookie {
            save_cookie(cookie_path, &secret)?;
            cookie = Some(secret);
        }
        match response.code {
            200 => break,
            202 => tokio::time::sleep(Duration::from_millis(100)).await,
            _ => return Err(format!("guest admission HTTP {}", response.code).into()),
        }
    }
    let ticket = http(port, "/api/join-ticket", Some(json!({})), cookie.as_deref()).await?;
    if ticket.code != 200 {
        return Err(format!("join ticket HTTP {}", ticket.code).into());
    }
    if ticket.body["ws_url"] != "ws://127.0.0.1:3000" {
        return Err("unexpected non-loopback game endpoint".into());
    }
    let ticket = ticket.body["join_ticket"]
        .as_str()
        .ok_or("join ticket missing")?
        .to_owned();
    let mut peer = Peer::connect("127.0.0.1:3000".parse()?).await?;
    let mut world = None;
    let mut session = None;
    let mut baseline = 0;
    let mut own = 0;
    let mut visible = BTreeSet::new();
    let mut ws = false;
    let mut rtc = false;
    let mut playing = false;
    let started = Instant::now();
    while !ws || !rtc {
        let (reliable, bytes) = peer.receive_any().await?;
        let envelope = decode_server(
            &bytes,
            if reliable {
                Delivery::Reliable
            } else {
                Delivery::Unreliable
            },
        )?;
        match envelope.message {
            ServerMessage::ServerHello(hello) => {
                world = envelope.world_id;
                send(
                    &mut peer,
                    &world,
                    &None,
                    ClientMessage::ClientHello(ClientHello {
                        protocol_version: hello.protocol_version,
                        build_id: hello.build_id,
                        content_hash: hello.content_hash,
                        sim_config_hash: hello.sim_config_hash,
                        join_ticket: OpaqueId(ticket.clone()),
                        display_name: DisplayName("Local Startup Probe".into()),
                    }),
                    true,
                )
                .await?;
            }
            ServerMessage::Welcome(w) => {
                session = envelope.session_id;
                own = w.player_id.0;
            }
            ServerMessage::BaselineBegin(b) => {
                baseline = b.baseline_id.0;
                visible.clear();
            }
            ServerMessage::EntityShow(show) => {
                visible.insert(show.descriptor.id.0);
            }
            ServerMessage::EntityHide(hide) => {
                visible.remove(&hide.entity_id.0);
            }
            ServerMessage::BaselineEnd(end) => {
                if end.baseline_id.0 != baseline
                    || end.entity_count.0 as usize != visible.len()
                    || !visible.contains(&own)
                {
                    return Err("incomplete startup baseline".into());
                }
                send(
                    &mut peer,
                    &world,
                    &session,
                    ClientMessage::ClientReady(ClientReady {
                        baseline_id: end.baseline_id,
                    }),
                    true,
                )
                .await?;
            }
            ServerMessage::StartPlay(_) => {
                playing = true;
                for reliable in [true, false] {
                    send(
                        &mut peer,
                        &world,
                        &session,
                        ClientMessage::Ping(Ping {
                            nonce: if reliable { 1 } else { 0x80000001 },
                            client_time_ms: Milliseconds(started.elapsed().as_secs_f64() * 1000.0),
                        }),
                        reliable,
                    )
                    .await?;
                }
            }
            ServerMessage::Pong(p) if playing => {
                if reliable && p.nonce == 1 {
                    ws = true;
                }
                if !reliable && p.nonce == 0x80000001 {
                    rtc = true;
                }
            }
            ServerMessage::Reject(r) => {
                return Err(format!("startup guest rejected: {:?}", r.code).into());
            }
            ServerMessage::Disconnect(d) => {
                return Err(format!("startup guest disconnected: {}", d.detail.0).into());
            }
            _ => {}
        }
    }
    let report = http(
        port,
        "/api/transport-probe",
        Some(json!({"session_id":session.as_ref().ok_or("session missing")?.0,"ws":ws,"rtc":rtc})),
        cookie.as_deref(),
    )
    .await?;
    if report.code != 200 {
        return Err(format!("transport receipt HTTP {}", report.code).into());
    }
    let ready = http(port, "/health/ready", None, None).await?;
    if ready.code != 200 {
        return Err(format!("readiness HTTP {}", ready.code).into());
    }
    send(
        &mut peer,
        &world,
        &session,
        ClientMessage::Leave(Empty {}),
        true,
    )
    .await?;
    peer.close().await?;
    println!(
        "PASS: complete baseline, StartPlay, nonce-matched WS + RTC receipts, proxy /health/ready 200; probe left."
    );
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: local_probe FRONTEND_PORT COOKIE_PATH".into());
    }
    let port = args[0].parse::<u16>()?;
    if ![5173, 4173].contains(&port) {
        return Err("only local dev/preview ports are supported".into());
    }
    timeout(Duration::from_secs(15), run(port, Path::new(&args[1]))).await?
}
#[test]
fn proxy_chunks_are_decoded_without_accepting_truncated_bodies() {
    assert_eq!(unchunk(b"2\r\n{}\r\n0\r\n\r\n").unwrap(), b"{}");
    assert!(unchunk(b"4\r\n{}\r\n").is_err());
}
