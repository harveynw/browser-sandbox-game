//! Test-only real WebSocket/RTC peer and datagram impairment relay.
//! Uses the same pinned WebRTC implementation as net; no release listener hooks.
#![allow(dead_code)] // Helpers are shared by transport and capacity integration binaries.
use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::VecDeque,
    future::pending,
    net::{SocketAddr, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
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
/// Forwards WebRTC connection events to the test client.
struct Handler(
    /// Queue used to forward connection events to the test client.
    mpsc::Sender<Signal>,
);
#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    /// Forwards a generated ICE candidate to the test client's signalling queue.
    async fn on_ice_candidate(&self, e: RTCPeerConnectionIceEvent) {
        if let Ok(c) = e.candidate.to_json() {
            let _ = self.0.try_send(Signal::Candidate(c));
        }
    }
    /// Forwards a new WebRTC data channel to the test client.
    async fn on_data_channel(&self, c: Arc<dyn DataChannel>) {
        let _ = self.0.try_send(Signal::Channel(c));
    }
}
/// Independent streams make the loss profile reproducible without coupling
/// its direction to the SCTP DATA/SACK packet cadence.
struct DatagramLoss(
    /// Separate random generator states for forward and reverse packet loss.
    [u32; 2],
);
impl Default for DatagramLoss {
    fn default() -> Self {
        Self([0x19c0_ff01, 0x19c0_ff02])
    }
}
impl DatagramLoss {
    fn drop(&mut self, direction: usize, one_in: usize) -> bool {
        if one_in == 0 {
            return false;
        }
        let state = &mut self.0[direction];
        *state ^= *state << 13;
        *state ^= *state >> 17;
        *state ^= *state << 5;
        (*state as usize).is_multiple_of(one_in)
    }
}
/// Verifies that independent loss is repeatable and not coupled to reverse traffic.
#[test]
fn independent_loss_is_repeatable_and_not_coupled_to_reverse_traffic() {
    let mut a = DatagramLoss::default();
    let mut b = DatagramLoss::default();
    let mut forward = 0;
    let mut reverse = 0;
    for _ in 0..1400 {
        let drop = a.drop(0, 7);
        assert_eq!(drop, b.drop(0, 7));
        forward += usize::from(drop);
        // Only one schedule sees this extra reverse traffic.
        reverse += usize::from(a.drop(1, 7));
        assert!(!a.drop(0, 0));
    }
    assert!((150..250).contains(&forward));
    assert!((150..250).contains(&reverse));
}

/// Stores the client and server addresses joined by the test relay.
#[derive(Default)]
struct Route {
    /// Address of the client side of the test relay.
    client: Option<SocketAddr>,
    /// Live network server, absent in socket-free tests.
    server: Option<SocketAddr>,
}
/// Counts packets, bytes, and impairments observed by the test relay.
#[derive(Debug, Default, Clone, Copy)]
pub struct RelayStats {
    // Index 0 is client -> server, index 1 is server -> client.
    received: [usize; 2],
    /// Packet counts dropped by relay direction.
    dropped: [usize; 2],
    /// Packet counts forwarded by relay direction.
    forwarded: [usize; 2],
    /// Number of packets from an unrecognised relay address.
    unknown: usize,
    /// UDP payload bytes received by relay direction.
    received_bytes: [usize; 2],
    /// UDP payload bytes forwarded by relay direction.
    forwarded_bytes: [usize; 2],
    /// Number of packets deliberately delivered out of order.
    reordered: usize,
    /// Number of packets deliberately delivered more than once.
    duplicated: usize,
    /// Number of packets dropped because the impairment queue was full.
    overflow: usize,
    /// Largest number of datagrams waiting in the impairment queue.
    peak_queue: usize,
}
impl RelayStats {
    /// Returns collected measurements as a JSON report.
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"received_packets":self.received,"dropped_packets":self.dropped,"forwarded_packets":self.forwarded,"received_udp_payload_bytes":self.received_bytes,"forwarded_udp_payload_bytes":self.forwarded_bytes,"reordered":self.reordered,"duplicated":self.duplicated,"overflow":self.overflow,"peak_queue":self.peak_queue})
    }
    /// Returns relay counter changes since the supplied earlier snapshot.
    pub fn since(&self, before: Self) -> Self {
        Self {
            received: std::array::from_fn(|i| self.received[i] - before.received[i]),
            dropped: std::array::from_fn(|i| self.dropped[i] - before.dropped[i]),
            forwarded: std::array::from_fn(|i| self.forwarded[i] - before.forwarded[i]),
            unknown: self.unknown - before.unknown,
            received_bytes: std::array::from_fn(|i| {
                self.received_bytes[i] - before.received_bytes[i]
            }),
            forwarded_bytes: std::array::from_fn(|i| {
                self.forwarded_bytes[i] - before.forwarded_bytes[i]
            }),
            reordered: self.reordered - before.reordered,
            duplicated: self.duplicated - before.duplicated,
            overflow: self.overflow - before.overflow,
            peak_queue: self.peak_queue,
        }
    }
}
/// Forwards test datagrams with controlled loss, delay, and reordering.
pub struct Relay {
    /// Shared packet and byte counters for the test relay.
    pub stats: Arc<Mutex<RelayStats>>,
    /// Local socket address exposed by the relay.
    address: SocketAddr,
    /// Navigation nodes remaining on the actor's route.
    route: Arc<Mutex<Route>>,
    /// Shared signal that asks the worker to stop.
    stop: Arc<AtomicBool>,
    /// Whether the relay applies the target network impairment profile.
    pub target_profile: Arc<AtomicBool>,
    /// Denominator of the configured random packet-loss probability.
    pub loss_one_in: Arc<AtomicUsize>,
    /// Whether the relay applies periodic packet loss.
    pub periodic_loss: Arc<AtomicBool>,
    /// Whether an obstacle currently prevents forward movement.
    pub blocked: Arc<AtomicBool>,
    /// Packet counts dropped by relay direction.
    pub dropped: Arc<AtomicUsize>,
    /// Packet counts forwarded by relay direction.
    pub forwarded: Arc<AtomicUsize>,
    /// Background worker owned by this handle.
    worker: Option<thread::JoinHandle<()>>,
}
impl Relay {
    /// Starts a local UDP relay with configurable network impairments.
    pub fn new() -> Result<Self> {
        let socket = UdpSocket::bind("127.0.0.1:0")?;
        socket.set_read_timeout(Some(Duration::from_millis(10)))?;
        let address = socket.local_addr()?;
        let route = Arc::new(Mutex::new(Route::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let blocked = Arc::new(AtomicBool::new(false));
        let loss_one_in = Arc::new(AtomicUsize::new(0));
        let periodic_loss = Arc::new(AtomicBool::new(false));
        let periodic = periodic_loss.clone();
        let target_profile = Arc::new(AtomicBool::new(false));
        let target_mode = target_profile.clone();
        let dropped = Arc::new(AtomicUsize::new(0));
        let forwarded = Arc::new(AtomicUsize::new(0));
        let (r, s, b, n, d, f) = (
            route.clone(),
            stop.clone(),
            blocked.clone(),
            loss_one_in.clone(),
            dropped.clone(),
            forwarded.clone(),
        );
        let stats = Arc::new(Mutex::new(RelayStats::default()));
        let counters = stats.clone();
        let worker = thread::spawn(move || {
            let mut bytes = [0u8; 65536];
            // Separate seeded streams avoid phase-locking loss to a protocol's
            // DATA/SACK cadence or concentrating every global seventh packet on one direction.
            let mut random = DatagramLoss::default();
            let mut count = 0;
            let mut impairment = DatagramLoss::default();
            let mut queue: Vec<(Instant, SocketAddr, usize, Vec<u8>)> = Vec::new();
            while !s.load(Ordering::Relaxed) {
                let target_enabled = target_mode.load(Ordering::Relaxed);
                let _ = socket.set_read_timeout(Some(Duration::from_millis(if target_enabled {
                    1
                } else {
                    10
                })));
                let now = Instant::now();
                let mut i = 0;
                while i < queue.len() {
                    if queue[i].0 <= now {
                        let (_, to, dir, packet) = queue.swap_remove(i);
                        if socket.send_to(&packet, to).is_ok() {
                            f.fetch_add(1, Ordering::Relaxed);
                            let mut st = counters.lock().unwrap();
                            st.forwarded[dir] += 1;
                            st.forwarded_bytes[dir] += packet.len();
                        }
                    } else {
                        i += 1;
                    }
                }
                if let Ok((size, from)) = socket.recv_from(&mut bytes) {
                    let target = {
                        let r = r.lock().unwrap();
                        if Some(from) == r.server {
                            r.client.map(|to| (1, to))
                        } else if Some(from) == r.client {
                            r.server.map(|to| (0, to))
                        } else {
                            None
                        }
                    };
                    if let Some((direction, target)) = target {
                        {
                            let mut st = counters.lock().unwrap();
                            st.received[direction] += 1;
                            st.received_bytes[direction] += size;
                        }
                        let every = n.load(Ordering::Relaxed);
                        count += 1;
                        let drop = if periodic.load(Ordering::Relaxed) {
                            every > 0 && count % every == 0
                        } else {
                            random.drop(direction, every)
                        };
                        if b.load(Ordering::Relaxed) || drop {
                            d.fetch_add(1, Ordering::Relaxed);
                            counters.lock().unwrap().dropped[direction] += 1;
                        } else if target_enabled {
                            // 50 +/-10ms each direction; independent 5% +40ms reorder and 1% duplicate.
                            let reorder = impairment.drop(direction, 20);
                            let duplicate = impairment.drop(direction, 100);
                            impairment.drop(direction, 2);
                            let jitter = (impairment.0[direction] % 21) as u64;
                            let due = Instant::now()
                                + Duration::from_millis(40 + jitter + if reorder { 40 } else { 0 });
                            let mut st = counters.lock().unwrap();
                            if queue.len() + 1 + usize::from(duplicate) > 512 {
                                st.overflow += 1;
                                st.dropped[direction] += 1;
                                d.fetch_add(1, Ordering::Relaxed);
                            } else {
                                queue.push((due, target, direction, bytes[..size].to_vec()));
                                if reorder {
                                    st.reordered += 1;
                                }
                                if duplicate {
                                    st.duplicated += 1;
                                    queue.push((
                                        due + Duration::from_millis(2),
                                        target,
                                        direction,
                                        bytes[..size].to_vec(),
                                    ));
                                }
                                st.peak_queue = st.peak_queue.max(queue.len());
                            }
                        } else if socket.send_to(&bytes[..size], target).is_ok() {
                            f.fetch_add(1, Ordering::Relaxed);
                            let mut st = counters.lock().unwrap();
                            st.forwarded[direction] += 1;
                            st.forwarded_bytes[direction] += size;
                        }
                    } else {
                        counters.lock().unwrap().unknown += 1;
                    }
                }
            }
        });
        Ok(Self {
            stats,
            address,
            route,
            stop,
            target_profile,
            loss_one_in,
            periodic_loss,
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
    /// Rewrites SDP candidate addresses to pass through the test relay.
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

/// Buffers bounded input and output for one connection incarnation.
pub struct Peer {
    /// WebSocket used for signalling and reliable gameplay messages.
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    /// Whether the WebRTC probe succeeded.
    rtc: Arc<dyn PeerConnection>,
    /// WebRTC events waiting for the test client to handle them.
    signals: mpsc::Receiver<Signal>,
    /// Unreliable WebRTC data channel, once established.
    channel: Option<Arc<dyn DataChannel>>,
    /// Test relay used to measure or impair network delivery.
    pub relay: Relay,
    /// Whether the WebRTC data channel has opened.
    pub open: bool,
    /// Gameplay packets received before the test data channel opened.
    pending: VecDeque<(bool, Vec<u8>)>,
}
impl Peer {
    /// Opens signalling and negotiates a test WebRTC data channel through the relay.
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
            pending: VecDeque::new(),
        };
        timeout(DEADLINE, async {
            while !p.open {
                if let Some(packet) = p.poll().await? {
                    if p.pending.len() >= 128 {
                        return Err("pre-open application queue full".into());
                    }
                    p.pending.push_back(packet);
                }
            }
            Result::Ok(())
        })
        .await??;
        assert!(!p.channel.as_ref().unwrap().ordered().await?);
        Ok(p)
    }
    /// Queues or submits a message using the required delivery channel.
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
    /// Returns the next gameplay packet from either transport within the test deadline.
    pub async fn receive_any(&mut self) -> Result<(bool, Vec<u8>)> {
        if let Some(packet) = self.pending.pop_front() {
            return Ok(packet);
        }
        timeout(DEADLINE, async {
            loop {
                if let Some(packet) = self.poll().await? {
                    return Ok(packet);
                }
            }
        })
        .await?
    }
    /// Validates incoming transport data and queues bounded decoded messages.
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
    /// Collects completed work or incoming events without blocking.
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
    /// Returns the data channel's currently buffered byte count.
    pub async fn outstanding(&self) -> Result<usize> {
        Ok(self
            .channel
            .as_ref()
            .ok_or("no RTC channel")?
            .outstanding_bytes()
            .await?)
    }
    /// Rejects sends when the test data channel exceeds its buffer limit.
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
            "Actual RTC backpressure: {accepted} queued packets, outstanding={} bytes; next send rejected",
            c.outstanding_bytes().await?
        );
        self.relay.blocked.store(false, Ordering::Relaxed);
        Ok(())
    }
    /// Closes the test client's WebRTC and signalling connections.
    pub async fn close(mut self) -> Result {
        let ws = self.socket.close(None).await;
        let rtc = self.rtc.close().await;
        rtc?;
        ws?;
        Ok(())
    }
}
