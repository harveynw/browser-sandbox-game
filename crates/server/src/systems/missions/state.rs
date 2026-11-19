use protocol::{MissionPhase, MissionView, OpaqueId, Scalar, TemplateId, Vec3};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Active,
    RewardPending,
    Completed,
    Failed,
}
#[derive(Clone)]
pub enum Objective {
    Courier,
    TimeTrial {
        checkpoints: Vec<[f32; 3]>,
        gate_radius: f32,
    },
    Recovery {
        garage: [f32; 3],
    },
}
/// Describes motion used to advance gameplay decisions.
pub struct Motion {
    /// Whether the actor and required vehicles remain usable.
    pub alive: bool,
    /// Number of sessions currently playing.
    pub playing: bool,
    /// World position in metres.
    pub position: [f32; 3],
    /// Movement speed in metres per second.
    pub speed: f32,
    /// Gameplay ID of the vehicle currently driven, if any.
    pub vehicle: Option<u32>,
    /// World position recorded before the current movement step.
    pub previous: Option<[f32; 3]>,
}
/// Tracks a private mission's objective, progress, deadline, and reward.
pub struct Mission {
    /// Rules that define how this mission progresses.
    pub objective: Objective,
    /// Index of the next mission checkpoint.
    pub checkpoint_index: u16,
    /// Gameplay ID of the vehicle reserved for this recovery mission.
    pub reserved_vehicle: Option<u32>,
    /// Unique mission instance ID used to deduplicate rewards.
    pub id: String,
    /// Authored mission template ID.
    pub template: String,
    /// Selected authored mission variant, if present.
    pub variant_index: Option<u16>,
    /// Simulation tick when the active mission fails.
    pub deadline: u32,
    /// Current objective position in world metres.
    pub target: [f32; 3],
    /// Distance within which the mission target counts as reached.
    pub radius: f32,
    /// Maximum speed allowed while dwelling at the target.
    pub max_speed: f32,
    /// Consecutive ticks required at the mission target.
    pub required_dwell: u32,
    /// Credits awarded after the mission completes.
    pub reward: u32,
    /// Consecutive eligible ticks spent at the current target.
    pub dwell: u32,
    /// Current stage of this item's lifecycle.
    pub phase: Phase,
    /// Tick when completed or failed mission status can be removed.
    pub clear_at: Option<u32>,
}
impl Mission {
    /// Builds a borrowed view of the state exposed to its caller.
    pub fn view(&self) -> MissionView {
        MissionView {
            instance_id: OpaqueId(self.id.clone()),
            template_id: TemplateId(self.template.clone()),
            variant_index: self.variant_index,
            phase: match self.phase {
                Phase::Completed => MissionPhase::Completed,
                Phase::Failed => MissionPhase::Failed,
                _ => MissionPhase::Active,
            },
            checkpoint_index: self.checkpoint_index,
            deadline_tick: self.deadline,
            target_entity_id: self.reserved_vehicle.map(protocol::EntityId),
            target_position: (self.phase != Phase::RewardPending)
                .then_some(Vec3(self.target.map(Scalar))),
        }
    }
    /// Marks the mission failed and schedules its private status for removal.
    pub fn fail(&mut self, tick: u32) {
        self.phase = Phase::Failed;
        self.dwell = 0;
        self.clear_at = Some(tick.saturating_add(150));
    }
    /// Advances mission progress from authoritative motion and checks failure deadlines.
    pub fn advance(&mut self, tick: u32, motion: Motion) {
        let Motion {
            alive,
            playing,
            position,
            speed,
            vehicle,
            previous,
        } = motion;
        if self.phase != Phase::Active {
            return;
        }
        if !alive || tick >= self.deadline {
            self.fail(tick);
            return;
        }
        match &self.objective {
            Objective::TimeTrial {
                checkpoints,
                gate_radius,
            } => {
                if playing
                    && vehicle.is_some()
                    && let Some(from) = previous
                    && swept_gate(from, position, self.target, *gate_radius)
                {
                    self.checkpoint_index += 1;
                    if usize::from(self.checkpoint_index) == checkpoints.len() {
                        self.phase = Phase::RewardPending;
                    } else {
                        self.target = checkpoints[usize::from(self.checkpoint_index)];
                    }
                }
            }
            Objective::Courier | Objective::Recovery { .. } => {
                let correct = !matches!(self.objective, Objective::Recovery { .. })
                    || vehicle == self.reserved_vehicle;
                let distance_sq: f32 = position
                    .iter()
                    .zip(self.target)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum();
                if playing
                    && correct
                    && distance_sq <= self.radius.powi(2)
                    && speed < self.max_speed
                {
                    self.dwell += 1;
                    if self.dwell >= self.required_dwell {
                        self.phase = Phase::RewardPending;
                    }
                } else {
                    self.dwell = 0;
                }
            }
        }
    }
}
/// Segment/sphere intersection includes crossings between ticks and near misses.
pub fn swept_gate(from: [f32; 3], to: [f32; 3], center: [f32; 3], radius: f32) -> bool {
    let delta: [f32; 3] = std::array::from_fn(|i| to[i] - from[i]);
    let len: f32 = delta.iter().map(|v| v * v).sum();
    let t = if len > 0.000001 {
        (center
            .iter()
            .zip(from)
            .zip(delta)
            .map(|((c, a), d)| (c - a) * d)
            .sum::<f32>()
            / len)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    (0..3)
        .map(|i| (from[i] + delta[i] * t - center[i]).powi(2))
        .sum::<f32>()
        <= radius * radius
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Creates a time-trial mission with a distinct return checkpoint.
    fn trial() -> Mission {
        Mission {
            id: "trial".into(),
            template: "trial".into(),
            variant_index: None,
            deadline: 100,
            target: [100.0, 0.0, 0.0],
            radius: 1.0,
            max_speed: 2.0,
            required_dwell: 30,
            reward: 200,
            dwell: 0,
            phase: Phase::Active,
            clear_at: None,
            objective: Objective::TimeTrial {
                checkpoints: vec![[0.0; 3], [100.0, 0.0, 0.0], [200.0, 0.0, 0.0], [0.0; 3]],
                gate_radius: 1.0,
            },
            checkpoint_index: 1,
            reserved_vehicle: None,
        }
    }
    /// Creates a driving observation between two positions for mission tests.
    fn motion(from: [f32; 3], to: [f32; 3]) -> Motion {
        Motion {
            alive: true,
            playing: true,
            position: to,
            speed: 30.0,
            vehicle: Some(1),
            previous: Some(from),
        }
    }
    /// Verifies that swept trial consumes one ordered gate per tick and distinct return finish.
    #[test]
    fn swept_trial_consumes_one_ordered_gate_per_tick_and_distinct_return_finish() {
        let mut m = trial();
        m.advance(1, motion([-2.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
        assert_eq!(m.checkpoint_index, 1);
        m.advance(2, motion([0.0; 3], [250.0, 0.0, 0.0]));
        assert_eq!(m.checkpoint_index, 2);
        assert_eq!(m.phase, Phase::Active);
        m.advance(3, motion([250.0, 0.0, 0.0], [150.0, 0.0, 0.0]));
        assert_eq!(m.checkpoint_index, 3);
        m.advance(4, motion([150.0, 0.0, 0.0], [-2.0, 0.0, 0.0]));
        assert_eq!(m.phase, Phase::RewardPending);
        m.advance(101, motion([0.0; 3], [0.0; 3]));
        assert_eq!(m.phase, Phase::RewardPending);
    }
    /// Verifies that swept gates reject near misses on foot and same tick failure or timeout.
    #[test]
    fn swept_gates_reject_near_misses_on_foot_and_same_tick_failure_or_timeout() {
        assert!(swept_gate(
            [-10.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0; 3],
            1.0
        ));
        assert!(!swept_gate(
            [-10.0, 0.0, 1.01],
            [10.0, 0.0, 1.01],
            [0.0; 3],
            1.0
        ));
        let mut m = trial();
        let mut input = motion([90.0, 0.0, 0.0], [110.0, 0.0, 0.0]);
        input.vehicle = None;
        m.advance(1, input);
        assert_eq!(m.checkpoint_index, 1);
        let mut input = motion([90.0, 0.0, 0.0], [110.0, 0.0, 0.0]);
        input.alive = false;
        m.advance(2, input);
        assert_eq!(m.phase, Phase::Failed);
        let mut m = trial();
        m.advance(100, motion([90.0, 0.0, 0.0], [110.0, 0.0, 0.0]));
        assert_eq!(m.phase, Phase::Failed);
    }
}
