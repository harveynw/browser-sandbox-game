//! Real clients for a separately launched production server. No world-state mutation hooks.
//! PM_CAPACITY_SECONDS=3600 PM_CAPACITY_OUT=var/capacity.json cargo test -p server --release --test capacity -- --ignored --nocapture
#[path = "support/rtc.rs"]
mod rtc;
use protocol::*;
use rtc::{Peer, Result};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, timeout},
};

struct Response {
    code: u16,
    body: Value,
    cookie: Option<String>,
}
async fn http(path: &str, post: bool, cookie: Option<&str>) -> Result<Response> {
    timeout(Duration::from_secs(8),async{
 let mut stream=TcpStream::connect("127.0.0.1:3001").await?;
 let body=if post{"{}"}else{""};
 let request=format!("{} {path} HTTP/1.1\r\nHost: 127.0.0.1:3001\r\nOrigin: http://localhost:4173\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}\r\n{body}",if post{"POST"}else{"GET"},body.len(),cookie.map(|c|format!("Cookie: {c}\r\n")).unwrap_or_default());
 stream.write_all(request.as_bytes()).await?;
 let mut bytes=Vec::new();stream.take(65536).read_to_end(&mut bytes).await?;
 let split=bytes.windows(4).position(|w|w==b"\r\n\r\n").ok_or("HTTP header")?;
 let headers=std::str::from_utf8(&bytes[..split])?;
 let code=headers.lines().next().and_then(|s|s.split_whitespace().nth(1)).ok_or("HTTP status")?.parse()?;
 let cookie=headers.lines().find(|s|s.to_ascii_lowercase().starts_with("set-cookie:")).and_then(|s|s.split_once(':')).map(|(_,v)|v.trim().split(';').next().unwrap_or("").to_owned());
 Ok(Response{code,body:serde_json::from_slice(&bytes[split+4..])?,cookie})
 }).await?
}
async fn ticket() -> Result<String> {
    let mut cookie = None;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let response = http("/api/session", true, cookie.as_deref()).await?;
        if response.cookie.is_some() {
            cookie = response.cookie;
        }
        if response.code == 200 {
            break;
        }
        if response.code != 202 || Instant::now() > deadline {
            return Err(format!("profile admission status {}", response.code).into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let response = http("/api/join-ticket", true, cookie.as_deref()).await?;
    if response.code != 200 {
        return Err(format!("ticket status {}", response.code).into());
    }
    Ok(response.body["join_ticket"]
        .as_str()
        .ok_or("ticket")?
        .to_owned())
}
#[derive(Default)]
struct BotStats {
    playing: bool,
    up: u64,
    down: u64,
    ws: u64,
    rtc: u64,
    player: u32,
    epoch: u32,
    tick: u32,
    joins_ms: f64,
    frames: u64,
    actions: u64,
    resyncs: u64,
    distance: f64,
    position: [f32; 3],
    visible: usize,
    relay: Value,
    rtt_ms: f64,
    closed: Option<String>,
}
impl BotStats {
    fn report(&self) -> Value {
        json!({"playing":self.playing,"player":self.player,"epoch":self.epoch,"tick":self.tick,"up_game_bytes":self.up,"down_game_bytes":self.down,"ws_packets":self.ws,"rtc_packets":self.rtc,"join_ms":self.joins_ms,"frames":self.frames,"actions":self.actions,"resyncs":self.resyncs,"distance_m":self.distance,"position":self.position,"visible":self.visible,"relay":self.relay,"rtc_rtt_ms":self.rtt_ms,"closed":self.closed})
    }
}
struct Walker {
    graph: Arc<content::RoadGraph>,
    route: VecDeque<u32>,
    cursor: usize,
    targets: Vec<u32>,
    last_plan: u32,
}
impl Walker {
    fn new(graph: Arc<content::RoadGraph>, index: usize) -> Self {
        let mut targets = Vec::new();
        for p in [
            [-240.0, -240.0],
            [240.0, -240.0],
            [240.0, 240.0],
            [-240.0, 240.0],
        ] {
            if let Some(n) = graph.nodes.iter().min_by(|a, b| {
                ((a.position[0] - p[0]).hypot(a.position[2] - p[1]))
                    .total_cmp(&((b.position[0] - p[0]).hypot(b.position[2] - p[1])))
            }) {
                targets.push(n.id);
            }
        }
        Self {
            graph,
            route: VecDeque::new(),
            cursor: index % 4,
            targets,
            last_plan: 0,
        }
    }
    fn controls(&mut self, p: [f32; 3], tick: u32) -> (i16, i16, f32) {
        let node = |id: u32| {
            self.graph
                .nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.position)
        };
        if let Some(next) = self.route.front().and_then(|id| node(*id))
            && ((next[0] - p[0] as f64).hypot(next[2] - p[2] as f64)) < 1.2
        {
            self.route.pop_front();
        }
        if self.route.is_empty() && tick.saturating_sub(self.last_plan) > 30 {
            self.last_plan = tick;
            let nearest = self
                .graph
                .nodes
                .iter()
                .min_by(|a, b| {
                    ((a.position[0] - p[0] as f64).hypot(a.position[2] - p[2] as f64)).total_cmp(
                        &((b.position[0] - p[0] as f64).hypot(b.position[2] - p[2] as f64)),
                    )
                })
                .map(|n| n.id);
            if let Some(start) = nearest {
                let target = self.targets[self.cursor % self.targets.len()];
                self.cursor += 1;
                let mut queue = VecDeque::from([start]);
                let mut from = BTreeMap::from([(start, start)]);
                while let Some(id) = queue.pop_front() {
                    if id == target {
                        break;
                    }
                    for edge in self.graph.edges.iter().filter(|e| e.from == id) {
                        if let std::collections::btree_map::Entry::Vacant(entry) =
                            from.entry(edge.to)
                        {
                            entry.insert(id);
                            queue.push_back(edge.to);
                        }
                    }
                }
                if from.contains_key(&target) {
                    let mut path = VecDeque::from([target]);
                    while *path.front().unwrap() != start {
                        path.push_front(from[path.front().unwrap()]);
                    }
                    self.route = path;
                }
            }
        }
        let Some(target) = self.route.front().and_then(|id| node(*id)) else {
            return (0, 0, 0.0);
        };
        let dx = (target[0] - p[0] as f64) as f32;
        let dz = (target[2] - p[2] as f64) as f32;
        let len = dx.hypot(dz).max(0.01);
        (
            (dx / len * 32767.0) as i16,
            (-dz / len * 32767.0) as i16,
            (-dx).atan2(-dz),
        )
    }
}
async fn send(
    peer: &mut Peer,
    world: &Option<OpaqueId>,
    session: &Option<OpaqueId>,
    message: ClientMessage,
    reliable: bool,
    stats: &Arc<Mutex<BotStats>>,
) -> Result {
    let bytes = encode_client(&ClientEnvelope {
        v: ProtocolVersion(3),
        world_id: world.clone(),
        session_id: session.clone(),
        message,
    })?;
    let len = bytes.len();
    timeout(Duration::from_secs(2), peer.send(reliable, bytes)).await??;
    stats.lock().unwrap().up += len as u64;
    Ok(())
}
async fn bot(
    index: usize,
    graph: Arc<content::RoadGraph>,
    stats: Arc<Mutex<BotStats>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result {
    let impaired = std::env::var("PM_CAPACITY_PROFILE").as_deref() == Ok("target");
    let started = Instant::now();
    let ticket = ticket().await?;
    let mut peer = Peer::connect("127.0.0.1:3000".parse()?).await?;
    let mut world = None;
    let mut session = None;
    let mut epoch = 1;
    let mut player = 0;
    let mut baseline = 0;
    let mut visible = BTreeSet::new();
    let mut playing = false;
    let mut position = [0.0f32; 3];
    let mut mode = CharacterMode::OnFoot;
    let mut magazine = 12;
    let mut anchor = (0u32, Instant::now());
    let mut clock_synced = false;
    let mut owner_tick = 0;
    let mut last_input = 0;
    let mut history = VecDeque::new();
    let mut ping_id = 0;
    let mut request = 0;
    let mut walker = Walker::new(graph, index);
    let mut frame = tokio::time::interval(Duration::from_nanos(1_000_000_000 / 30));
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
    let mut action = tokio::time::interval(Duration::from_secs(3));
    loop {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        tokio::select! {
        packet=peer.receive_any()=>{
        let(reliable,bytes)=packet?;{let mut s=stats.lock().unwrap();s.down+=bytes.len() as u64;if reliable{s.ws+=1}else{s.rtc+=1}}
        let envelope=decode_server(&bytes,if reliable{Delivery::Reliable}else{Delivery::Unreliable})?;
        match envelope.message{
        ServerMessage::ServerHello(info)=>{world=envelope.world_id;send(&mut peer,&world,&None,ClientMessage::ClientHello(ClientHello{protocol_version:ProtocolVersion(3),build_id:info.build_id,content_hash:info.content_hash,sim_config_hash:info.sim_config_hash,join_ticket:OpaqueId(ticket.clone()),display_name:DisplayName(format!("Load Peer {index:02}"))}),true,&stats).await?;},
        ServerMessage::Welcome(w)=>{session=envelope.session_id;player=w.player_id.0;},
        ServerMessage::BaselineBegin(b)=>{baseline=b.baseline_id.0;playing=false;visible.clear();history.clear();},
        ServerMessage::EntityShow(s)=>{visible.insert(s.descriptor.id.0);},
        ServerMessage::EntityHide(h)=>{visible.remove(&h.entity_id.0);},
        ServerMessage::BaselineEnd(b)=>{if b.baseline_id.0!=baseline||visible.len()!=b.entity_count.0 as usize||!visible.contains(&player){return Err("incomplete load baseline".into());}send(&mut peer,&world,&session,ClientMessage::ClientReady(ClientReady{baseline_id:b.baseline_id}),true,&stats).await?;},
        ServerMessage::StartPlay(start)=>{if impaired{peer.relay.loss_one_in.store(50,std::sync::atomic::Ordering::Relaxed);peer.relay.target_profile.store(true,std::sync::atomic::Ordering::Relaxed);}playing=true;let o=start.owner;epoch=o.control_epoch.0;owner_tick=o.server_tick;last_input=owner_tick;anchor=(owner_tick,Instant::now());position=o.player_state.motor.position.0.map(|v|v.0);mode=o.player_state.mode;history.clear();let mut s=stats.lock().unwrap();s.joins_ms=started.elapsed().as_secs_f64()*1000.0;s.playing=true;s.player=player;},
        ServerMessage::OwnerState(o) if o.control_epoch.0==epoch&&o.server_tick>owner_tick=>{let next=o.player_state.motor.position.0.map(|v|v.0);let mut s=stats.lock().unwrap();s.distance+=((next[0]-position[0]).hypot(next[2]-position[2])) as f64;position=next;mode=o.player_state.mode;owner_tick=o.server_tick;if !clock_synced{anchor=(owner_tick,Instant::now());}s.position=position;s.epoch=epoch;s.tick=owner_tick;s.visible=visible.len();},
        ServerMessage::ControlChanged(o)=>{epoch=o.control_epoch.0;owner_tick=o.server_tick;last_input=owner_tick;anchor=(owner_tick,Instant::now());position=o.player_state.motor.position.0.map(|v|v.0);mode=o.player_state.mode;history.clear();},
        ServerMessage::PlayerStatus(p)=>{magazine=p.magazine.0;},
        ServerMessage::Pong(p)=>{let rtt=started.elapsed().as_secs_f64()*1000.0-p.client_time_ms.0;if p.nonce & 0x80000000 !=0{stats.lock().unwrap().rtt_ms=rtt;}else if (0.0..10000.0).contains(&rtt){anchor=(p.server_tick+(rtt*0.015) as u32,Instant::now());clock_synced=true;}},
        ServerMessage::Reject(r)=>return Err(format!("join rejected {:?}",r.code).into()),
        ServerMessage::Disconnect(d)=>return Err(format!("disconnected {}",d.detail.0).into()),_=>{}
        }
        },
        _=frame.tick(),if playing=>{
        let estimated=anchor.0+(anchor.1.elapsed().as_secs_f64()*30.0) as u32;let tick=estimated+if impaired{7}else{3};
        if tick>last_input{last_input=tick;let(x,z,yaw)=if mode==CharacterMode::OnFoot{walker.controls(position,tick)}else{(0,0,0.0)};history.push_back(InputFrame{tick,move_x:InputAxis(x),move_z:InputAxis(z),look_yaw:Yaw(yaw),look_pitch:Pitch(0.0),buttons:Buttons(1)});while history.len()>3{history.pop_front();}send(&mut peer,&world,&session,ClientMessage::InputBatch(InputBatch{control_epoch:ControlEpoch(epoch),frames:history.iter().cloned().collect()}),false,&stats).await?;stats.lock().unwrap().frames+=1;}
        },
        _=heartbeat.tick(),if session.is_some()=>{stats.lock().unwrap().relay=peer.relay.stats.lock().unwrap().report();ping_id+=1;for reliable in [true,false]{send(&mut peer,&world,&session,ClientMessage::Ping(Ping{nonce:if reliable{ping_id}else{ping_id|0x80000000},client_time_ms:Milliseconds(started.elapsed().as_secs_f64()*1000.0)}),reliable,&stats).await?;}},
        _=action.tick(),if playing=>{if mode==CharacterMode::OnFoot{request+=1;let a=if magazine==0{Action::Reload(Empty{})}else{Action::Fire(Fire{aim_yaw:Yaw(0.0),aim_pitch:Pitch(0.0)})};send(&mut peer,&world,&session,ClientMessage::Action(ActionRequest{request_id:RequestId(request),control_epoch:ControlEpoch(epoch),action:a}),true,&stats).await?;stats.lock().unwrap().actions+=1;}},
        }
    }
    send(
        &mut peer,
        &world,
        &session,
        ClientMessage::Leave(Empty {}),
        true,
        &stats,
    )
    .await?;
    peer.close().await?;
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires separate production server and a real browser observer; defaults to a 60-minute soak"]
async fn populated_actual_rtc_capacity() -> Result {
    let seconds = std::env::var("PM_CAPACITY_SECONDS")
        .unwrap_or_else(|_| "3600".into())
        .parse::<u64>()?;
    if !(10..=7200).contains(&seconds) {
        return Err("duration must be 10..7200 seconds".into());
    }
    let out = std::env::var("PM_CAPACITY_OUT").unwrap_or_else(|_| "var/capacity.json".into());
    let content = server::content_loader::WorldContent::load_index(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content"),
        "city.json",
    )?;
    let graph = Arc::new(content.manifest.pedestrian_graph);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut tasks = Vec::new();
    let mut all = Vec::new();
    for index in 0..15 {
        let stats = Arc::new(Mutex::new(BotStats::default()));
        all.push(stats.clone());
        let graph = graph.clone();
        let signal = stop.clone();
        tasks.push(tokio::spawn(async move {
            let result = bot(index, graph, stats.clone(), signal).await;
            if let Err(e) = &result {
                stats.lock().unwrap().closed = Some(e.to_string());
            }
            result
        }));
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    println!(
        "Capacity synthetic RTC peers dispatched; join one browser at http://localhost:4173/ (16 total required)"
    );
    let profile = std::env::var("PM_CAPACITY_PROFILE").unwrap_or_else(|_| "loopback".into());
    if profile != "loopback" && profile != "target" {
        return Err("unknown capacity profile".into());
    }
    let admission_deadline = Instant::now() + Duration::from_secs(300);
    let mut samples = Vec::new();
    loop {
        let m = http("/api/metrics", false, None).await?.body;
        if m["population"]["players"].as_u64() == Some(16)
            && all.iter().all(|s| s.lock().unwrap().playing)
        {
            break;
        }
        if Instant::now() > admission_deadline {
            return Err("16-player admission deadline; no capacity pass".into());
        }
        for s in &all {
            if let Some(e) = &s.lock().unwrap().closed {
                return Err(e.clone().into());
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let started = Instant::now();
    println!("Capacity measurement started: 15 real RTC bots plus browser, duration={seconds}s");
    let result:Result=async {
      loop {
        let response=http("/api/metrics",false,None).await?;
        let elapsed=started.elapsed().as_secs_f64();
        let bots:Vec<_>=all.iter().map(|s|s.lock().unwrap().report()).collect();
        let rss=std::env::var("PM_CAPACITY_SERVER_PID").ok().and_then(|pid| {
          if pid.parse::<u32>().is_err(){return None;}
          std::process::Command::new("ps").args(["-p",&pid,"-o","rss="]).output().ok().filter(|o|o.status.success()).and_then(|o|String::from_utf8(o.stdout).ok()).and_then(|s|s.trim().parse::<u64>().ok())
        });
        samples.push(json!({"elapsed_s":elapsed,"server":response.body,"server_rss_kib":rss,"bots":bots}));
        let report=json!({"duration_s":elapsed,"requested_s":seconds,"completed":false,"native_peers":15,"requires_browser":true,"profile":profile,"seed":424242,"scope":"Local host; cumulative server histograms since startup, populated_tick_work only samples 16-player ticks. UDP payload includes DTLS/SCTP/STUN; excludes IP/UDP and WS framing. Browser network is loopback.","samples":samples});
        if let Some(parent)=Path::new(&out).parent(){std::fs::create_dir_all(parent)?;}
        std::fs::write(&out,serde_json::to_vec(&report)?)?;
        if response.body["population"]["players"].as_u64()!=Some(16){return Err("population fell below 16 during measurement; report is incomplete".into());}
        if bots.iter().any(|s|s["closed"].is_string()){return Err("synthetic peer closed during measurement; report is incomplete".into());}
        println!("Capacity sample t={elapsed:.0}s players={} tick_p95={}ms traffic={} peds={} police={}",response.body["population"]["players"],response.body["populated_tick_work"]["p95_ms"],response.body["population"]["traffic"],response.body["population"]["pedestrians"],response.body["population"]["police"]);
        if elapsed>=seconds as f64 {break;}
        tokio::time::sleep(Duration::from_secs_f64((seconds as f64-elapsed).min(10.0))).await;
      }
      Ok(())
    }.await;
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut cleanup_error = None;
    for task in tasks {
        if let Err(e) = async {
            timeout(Duration::from_secs(12), task).await???;
            Result::Ok(())
        }
        .await
        {
            cleanup_error = Some(e);
        }
    }
    result?;
    if let Some(e) = cleanup_error {
        return Err(e);
    }
    {
        let mut report: Value = serde_json::from_slice(&std::fs::read(&out)?)?;
        report["duration_s"] = json!(started.elapsed().as_secs_f64());
        report["completed"] = json!(true);
        std::fs::write(&out, serde_json::to_vec(&report)?)?;
    }
    println!("Capacity measurement saved: {out}");
    Ok(())
}
