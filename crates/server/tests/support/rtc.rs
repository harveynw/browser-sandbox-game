//! Test-only real WebSocket/RTC peer and datagram impairment relay.
//! Uses the same pinned WebRTC implementation as net; no release listener hooks.
use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use std::{
    future::pending,
    net::{SocketAddr, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};
use tokio::{net::TcpStream, sync::mpsc, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use webrtc::{
    data_channel::{DataChannel, DataChannelEvent},
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceCandidateInit,
        RTCPeerConnectionIceEvent, RTCSessionDescription,
    },
    runtime::TokioRuntime,
};
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const DEADLINE: Duration = Duration::from_secs(10);
enum Signal {
    Candidate(RTCIceCandidateInit),
    Channel(Arc<dyn DataChannel>),
}
struct Handler(mpsc::Sender<Signal>);
#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_candidate(&self, e: RTCPeerConnectionIceEvent) {
        if let Ok(c) = e.candidate.to_json() {
            let _ = self.0.try_send(Signal::Candidate(c));
        }
    }
    async fn on_data_channel(&self, c: Arc<dyn DataChannel>) {
        let _ = self.0.try_send(Signal::Channel(c));
    }
}
#[derive(Default)]
struct Route {
    client: Option<SocketAddr>,
    server: Option<SocketAddr>,
}
pub struct Relay {
    address: SocketAddr,
    route: Arc<Mutex<Route>>,
    stop: Arc<AtomicBool>,
    pub drop_every: Arc<AtomicUsize>,
    pub blocked: Arc<AtomicBool>,
    pub dropped: Arc<AtomicUsize>,
    pub forwarded: Arc<AtomicUsize>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Relay {
    pub fn new() -> Result<Self> {
        let socket = UdpSocket::bind("127.0.0.1:0")?;
        socket.set_read_timeout(Some(Duration::from_millis(10)))?;
        let address = socket.local_addr()?;
        let route = Arc::new(Mutex::new(Route::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let blocked = Arc::new(AtomicBool::new(false));
        let drop_every = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let forwarded = Arc::new(AtomicUsize::new(0));
        let (r, s, b, n, d, f) = (
            route.clone(),
            stop.clone(),
            blocked.clone(),
            drop_every.clone(),
            dropped.clone(),
            forwarded.clone(),
        );
        let worker = thread::spawn(move || {
            let mut bytes = [0u8; 65536];
            let mut count = 0;
            while !s.load(Ordering::Relaxed) {
                if let Ok((size, from)) = socket.recv_from(&mut bytes) {
                    let target = {
                        let r = r.lock().unwrap();
                        if Some(from) == r.server {
                            r.client
                        } else if Some(from) == r.client {
                            r.server
                        } else {
                            None
                        }
                    };
                    if let Some(target) = target {
                        count += 1;
                        let every = n.load(Ordering::Relaxed);
                        if b.load(Ordering::Relaxed) || (every > 0 && count % every == 0) {
                            d.fetch_add(1, Ordering::Relaxed);
                        } else if socket.send_to(&bytes[..size], target).is_ok() {
                            f.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        });
        Ok(Self {
            address,
            route,
            stop,
            drop_every,
            blocked,
            dropped,
            forwarded,
            worker: Some(worker),
        })
    }
    // Both peers advertise this relay; actual DTLS/SCTP datagrams cross the socket.
    fn candidate(&self, candidate: &str, server: bool) -> String {
        let mut parts: Vec<String> = candidate.split_whitespace().map(str::to_owned).collect();
        if parts.len() >= 6 && parts[2].eq_ignore_ascii_case("udp") {
            if let Ok(address) = format!("{}:{}", parts[4], parts[5]).parse::<SocketAddr>() {
                let mut route = self.route.lock().unwrap();
                if server {
                    route.server = Some(address)
                } else {
                    route.client = Some(address)
                }
            }
            parts[4] = self.address.ip().to_string();
            parts[5] = self.address.port().to_string();
            parts.join(" ")
        } else {
            candidate.to_owned()
        }
    }
    fn sdp(&self, sdp: &str, server: bool) -> String {
        sdp.split_inclusive('\n')
            .map(|line| {
                if let Some(c) = line.strip_prefix("a=candidate:") {
                    format!("a=candidate:{}\r\n", self.candidate(c.trim(), server))
                } else {
                    line.to_owned()
                }
            })
            .collect()
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

pub struct Peer {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    rtc: Arc<dyn PeerConnection>,
    signals: mpsc::Receiver<Signal>,
    channel: Option<Arc<dyn DataChannel>>,
    pub relay: Relay,
    pub open: bool,
}
impl Peer {
    pub async fn connect(address: SocketAddr) -> Result<Self> {
        let (socket, _) = connect_async(format!("ws://{address}")).await?;
        let (tx, signals) = mpsc::channel(16);
        let rtc = PeerConnectionBuilder::new()
            .with_udp_addrs(vec!["127.0.0.1:0".parse::<SocketAddr>()?])
            .with_data_channel_send_buffer_limit(65536)
            .with_runtime(Arc::new(TokioRuntime))
            .with_handler(Arc::new(Handler(tx)))
            .build()
            .await?;
        let mut p = Self {
            socket,
            rtc: Arc::new(rtc),
            signals,
            channel: None,
            relay: Relay::new()?,
            open: false,
        };
        timeout(DEADLINE, async {
            while !p.open {
                p.poll().await?;
            }
            Result::Ok(())
        })
        .await??;
        assert!(!p.channel.as_ref().unwrap().ordered().await?);
        Ok(p)
    }
    pub async fn send(&mut self, reliable: bool, bytes: Vec<u8>) -> Result {
        if reliable {
            self.socket.send(Message::Binary(bytes.into())).await?;
        } else {
            self.channel
                .as_ref()
                .ok_or("no RTC channel")?
                .send(BytesMut::from(bytes.as_slice()))
                .await?;
        }
        Ok(())
    }
    pub async fn receive(&mut self, reliable: bool) -> Result<Vec<u8>> {
        timeout(DEADLINE, async {
            loop {
                if let Some((r, b)) = self.poll().await?
                    && r == reliable
                {
                    return Ok(b);
                }
            }
        })
        .await?
    }
    async fn poll(&mut self) -> Result<Option<(bool, Vec<u8>)>> {
        let channel = self.channel.clone();
        tokio::select! {
        ws=self.socket.next()=>{match ws.ok_or("WS ended")??{
        Message::Binary(b)=>return Ok(Some((true,b.to_vec()))),
        Message::Text(text)=>{let signal:serde_json::Value=serde_json::from_str(&text)?;
        if let Some(sdp)=signal["sdp"].as_str(){self.rtc.set_remote_description(RTCSessionDescription::offer(self.relay.sdp(sdp,true))?).await?;let answer=self.rtc.create_answer(None).await?;self.rtc.set_local_description(answer.clone()).await?;self.socket.send(Message::text(serde_json::json!({"type":"answer","sdp":self.relay.sdp(&answer.sdp,false)}).to_string())).await?;}
        else if signal["candidate"].is_object(){let mut c:RTCIceCandidateInit=serde_json::from_value(signal["candidate"].clone())?;c.candidate=self.relay.candidate(&c.candidate,true);self.rtc.add_ice_candidate(c).await?;}
        },Message::Close(_)=>return Err("WS closed".into()),_=>{}}},
        signal=self.signals.recv()=>{match signal.ok_or("signals closed")?{Signal::Channel(c)=>{assert_eq!(c.label().await?,"game");self.channel=Some(c);},Signal::Candidate(mut c)=>{c.candidate=self.relay.candidate(&c.candidate,false);self.socket.send(Message::text(serde_json::json!({"type":"ice","candidate":c}).to_string())).await?;}}},
        event=async move{match channel{Some(c)=>c.poll().await,None=>pending().await}}=>{match event.ok_or("RTC ended")?{DataChannelEvent::OnOpen=>self.open=true,DataChannelEvent::OnMessage(m)=>{assert!(!m.is_string);return Ok(Some((false,m.data.to_vec())));},DataChannelEvent::OnClose=>return Err("RTC closed".into()),_=>{}}}
        }
        Ok(None)
    }
    pub async fn check_backpressure(&mut self, packet: &[u8]) -> Result {
        self.relay.blocked.store(true, Ordering::Relaxed);
        let c = self.channel.as_ref().ok_or("no RTC channel")?;
        let mut rejected = false;
        let mut accepted = 0;
        for _ in 0..1000 {
            match c.try_send(BytesMut::from(packet)).await {
                Ok(()) => accepted += 1,
                Err(webrtc::error::Error::ErrSendBufferFull) => {
                    rejected = true;
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }
        assert!(rejected, "RTC outstanding send buffer did not backpressure");
        assert!(c.outstanding_bytes().await? <= 65536);
        println!(
            "T19 actual RTC backpressure: {accepted} queued packets, outstanding={} bytes; next send rejected",
            c.outstanding_bytes().await?
        );
        self.relay.blocked.store(false, Ordering::Relaxed);
        Ok(())
    }
    pub async fn close(mut self) -> Result {
        let ws = self.socket.close(None).await;
        let rtc = self.rtc.close().await;
        rtc?;
        ws?;
        Ok(())
    }
}
