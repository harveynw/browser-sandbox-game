//! Full independent records, bounded bytes and spatial relevance; no delta dependencies.
use protocol::*;
use std::collections::{BTreeMap, BTreeSet};
use std::result::Result;
/// Keep a bounded baseline/lifecycle burst available while snapshots saturate the link.
/// This reserves tokens inside the existing 60 KiB/s, 120 KiB burst budget.
pub const LIFECYCLE_RESERVE: usize = 64 * 1024;
#[derive(Debug)]
pub struct Budget {
    tokens: f64,
    at: u64,
    rate: f64,
    capacity: f64,
}
impl Default for Budget {
    fn default() -> Self {
        Self::new(60 * 1024, 120 * 1024)
    }
}
impl Budget {
    pub fn new(rate: u32, burst: u32) -> Self {
        Self {
            tokens: f64::from(burst),
            at: 0,
            rate: f64::from(rate),
            capacity: f64::from(burst),
        }
    }
    pub fn allow(&mut self, bytes: usize, now: u64, reserve: usize) -> bool {
        self.tokens = (self.tokens + now.saturating_sub(self.at) as f64 * self.rate / 1000.0)
            .min(self.capacity);
        self.at = now;
        if self.tokens < (bytes + reserve) as f64 {
            return false;
        }
        self.tokens -= bytes as f64;
        true
    }
}
/// Strict rolling one-second admission cap, independent of queue draining cadence.
#[derive(Default)]
pub struct InputRate {
    arrivals: std::collections::VecDeque<u64>,
}
impl InputRate {
    pub fn allow(&mut self, now: u64) -> bool {
        while self
            .arrivals
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= 1000)
        {
            self.arrivals.pop_front();
        }
        if self.arrivals.len() >= 60 {
            return false;
        }
        self.arrivals.push_back(now);
        true
    }
}
type CellContents = Vec<(u32, [f32; 2])>;
pub struct SpatialIndex {
    cells: BTreeMap<(i32, i32), CellContents>,
}
impl SpatialIndex {
    pub fn new(points: impl Iterator<Item = (u32, [f32; 2])>) -> Self {
        let mut cells: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for (id, point) in points {
            cells.entry(cell(point)).or_default().push((id, point));
        }
        Self { cells }
    }
    pub fn relevant(
        &self,
        center: [f32; 2],
        owner: u32,
        previous: &BTreeMap<u32, u32>,
    ) -> BTreeSet<u32> {
        let (cx, cz) = cell(center);
        let mut candidates = Vec::new();
        for x in cx - 3..=cx + 3 {
            for z in cz - 3..=cz + 3 {
                if let Some(points) = self.cells.get(&(x, z)) {
                    for &(id, p) in points {
                        let d = (p[0] - center[0]).hypot(p[1] - center[1]);
                        if id == owner
                            || d <= if previous.contains_key(&id) {
                                192.0
                            } else {
                                160.0
                            }
                        {
                            candidates.push((id, d));
                        }
                    }
                }
            }
        }
        candidates.sort_by(|a, b| {
            (a.0 != owner)
                .cmp(&(b.0 != owner))
                .then(a.1.total_cmp(&b.1))
                .then(a.0.cmp(&b.0))
        });
        candidates.into_iter().take(96).map(|(id, _)| id).collect()
    }
}
fn cell(p: [f32; 2]) -> (i32, i32) {
    ((p[0] / 64.0).floor() as i32, (p[1] / 64.0).floor() as i32)
}
pub fn snapshot(
    world: &str,
    session: &str,
    tick: u32,
    records: Vec<StateRecord>,
) -> ServerEnvelope {
    ServerEnvelope {
        v: ProtocolVersion(3),
        world_id: Some(OpaqueId(world.into())),
        session_id: Some(OpaqueId(session.into())),
        message: ServerMessage::Snapshot(Snapshot {
            server_tick: tick,
            records,
        }),
    }
}
/// Pack by the real MessagePack encoder, including scope/header overhead. Never split a record.
pub fn packets(
    world: &str,
    session: &str,
    tick: u32,
    records: impl IntoIterator<Item = StateRecord>,
) -> Result<Vec<ServerEnvelope>, String> {
    let mut result = Vec::new();
    let mut pending = Vec::new();
    for record in records {
        let mut candidate = pending.clone();
        candidate.push(record.clone());
        if encode_server(
            &snapshot(world, session, tick, candidate.clone()),
            Delivery::Unreliable,
        )
        .is_ok()
        {
            pending = candidate;
        } else {
            if pending.is_empty() {
                return Err("single snapshot record exceeds RTC byte budget".into());
            }
            result.push(snapshot(world, session, tick, std::mem::take(&mut pending)));
            encode_server(
                &snapshot(world, session, tick, vec![record.clone()]),
                Delivery::Unreliable,
            )
            .map_err(|e| e.to_string())?;
            pending.push(record);
        }
    }
    if !pending.is_empty() {
        result.push(snapshot(world, session, tick, pending));
    }
    Ok(result)
}
/// Coalesce per entity/visibility, preserving same-tick packet splits and unrelated older records.
#[derive(Default)]
pub struct SnapshotBuffer {
    scope: Option<(String, String)>,
    records: BTreeMap<u32, (u32, StateRecord)>,
}
impl SnapshotBuffer {
    pub fn pending_records(&self) -> usize {
        self.records.len()
    }
    pub fn clear(&mut self) {
        self.scope = None;
        self.records.clear();
    }
    pub fn hide(&mut self, entity: u32, visibility: u32) {
        if self
            .records
            .get(&entity)
            .is_some_and(|(_, r)| r.visibility_id.0 <= visibility)
        {
            self.records.remove(&entity);
        }
    }
    pub fn push(&mut self, envelope: &ServerEnvelope) -> Result<bool, String> {
        let ServerMessage::Snapshot(data) = &envelope.message else {
            return Err("expected snapshot".into());
        };
        let scope = (
            envelope.world_id.as_ref().ok_or("missing world")?.0.clone(),
            envelope
                .session_id
                .as_ref()
                .ok_or("missing session")?
                .0
                .clone(),
        );
        if self.scope.as_ref().is_some_and(|s| *s != scope) {
            self.clear();
        }
        self.scope = Some(scope);
        let mut replaced = false;
        for record in &data.records {
            if let Some((tick, old)) = self.records.get(&record.entity_id.0) {
                if record.visibility_id.0 < old.visibility_id.0
                    || (record.visibility_id == old.visibility_id && data.server_tick <= *tick)
                {
                    continue;
                }
                replaced = true;
            } else if self.records.len() >= 96 {
                return Err("snapshot queue entity cap".into());
            }
            self.records
                .insert(record.entity_id.0, (data.server_tick, record.clone()));
        }
        Ok(replaced)
    }
    pub fn drain(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let Some((world, session)) = self.scope.take() else {
            return Ok(Vec::new());
        };
        let mut groups: BTreeMap<u32, Vec<StateRecord>> = BTreeMap::new();
        for (_, (tick, record)) in std::mem::take(&mut self.records) {
            groups.entry(tick).or_default().push(record);
        }
        let mut bytes = Vec::new();
        for (tick, records) in groups {
            for envelope in packets(&world, &session, tick, records)? {
                bytes.push(
                    encode_server(&envelope, Delivery::Unreliable).map_err(|e| e.to_string())?,
                );
            }
        }
        Ok(bytes)
    }
}
