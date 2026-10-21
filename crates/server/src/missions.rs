//! Server-only courier lifecycle. Clients supply acceptance/cancellation intent only.
use protocol::{MissionPhase, MissionView, OpaqueId, Scalar, TemplateId, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Active,
    RewardPending,
    Completed,
    Failed,
}
pub struct Courier {
    pub id: String,
    pub template: String,
    pub deadline: u32,
    pub target: [f32; 3],
    pub radius: f32,
    pub max_speed: f32,
    pub required_dwell: u32,
    pub reward: u32,
    pub dwell: u32,
    pub phase: Phase,
    pub clear_at: Option<u32>,
}
impl Courier {
    pub fn view(&self) -> MissionView {
        MissionView {
            instance_id: OpaqueId(self.id.clone()),
            template_id: TemplateId(self.template.clone()),
            phase: match self.phase {
                Phase::Completed => MissionPhase::Completed,
                Phase::Failed => MissionPhase::Failed,
                _ => MissionPhase::Active,
            },
            checkpoint_index: 0,
            deadline_tick: self.deadline,
            target_entity_id: None,
            target_position: Some(Vec3(self.target.map(Scalar))),
        }
    }
    pub fn fail(&mut self, tick: u32) {
        self.phase = Phase::Failed;
        self.dwell = 0;
        self.clear_at = Some(tick.saturating_add(150));
    }
    pub fn advance(
        &mut self,
        tick: u32,
        alive: bool,
        playing: bool,
        position: [f32; 3],
        speed: f32,
    ) {
        if self.phase != Phase::Active {
            return;
        }
        // Failure/timeout precede completion, including the last dwell tick.
        if !alive || tick >= self.deadline {
            self.fail(tick);
            return;
        }
        let distance_sq = position
            .iter()
            .zip(self.target)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>();
        if playing && distance_sq <= self.radius.powi(2) && speed < self.max_speed {
            self.dwell += 1;
            if self.dwell >= self.required_dwell {
                self.phase = Phase::RewardPending;
            }
        } else {
            self.dwell = 0;
        }
    }
}
