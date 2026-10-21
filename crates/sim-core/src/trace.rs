//! Pure recorded-trace replay. Filesystem/CLI orchestration stays outside sim-core.
use crate::{CollisionQueries, CollisionWorld, Result, SimConfig, StepContext, step_character};
use protocol::{Buttons, CharacterMotor, InputAxis, InputFrame, Pitch, Yaw};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub ticks: u32,
    pub move_x: i16,
    pub move_z: i16,
    pub look_yaw: f32,
    pub buttons: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub tick: u32,
    pub motor: CharacterMotor,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterTrace {
    pub version: u32,
    pub description: String,
    pub sim_config: content::SimConfig,
    pub sim_config_hash: String,
    pub collision: content::CollisionPackage,
    pub collision_hash: String,
    pub initial_motor: CharacterMotor,
    pub segments: Vec<Segment>,
    pub checkpoints: Vec<Checkpoint>,
}
#[derive(Debug)]
pub struct TraceReport {
    pub ticks: u32,
    pub checkpoints: usize,
    pub max_position_error: f32,
}
pub fn replay(trace: &CharacterTrace) -> Result<Vec<Checkpoint>> {
    if trace.version != 1
        || trace.segments.is_empty()
        || trace.segments.len() > 1800
        || trace.checkpoints.is_empty()
        || trace.checkpoints.len() > 1800
    {
        return Err("invalid trace version/count".into());
    }
    if content::canonical::sha256(&content::canonical::bytes(&trace.sim_config)?)
        != trace.sim_config_hash
        || content::canonical::sha256(&content::canonical::bytes(&trace.collision)?)
            != trace.collision_hash
    {
        return Err("trace fixture hash mismatch".into());
    }
    let total = trace
        .segments
        .iter()
        .try_fold(0u32, |sum, s| {
            if s.ticks == 0 {
                None
            } else {
                sum.checked_add(s.ticks)
            }
        })
        .ok_or("invalid trace duration")?;
    if total > 1800
        || trace.checkpoints.windows(2).any(|p| p[0].tick >= p[1].tick)
        || trace.checkpoints[0].tick == 0
        || trace.checkpoints.last().ok_or("no checkpoints")?.tick != total
    {
        return Err("trace checkpoints/duration invalid".into());
    }
    let config = SimConfig::from_content(&trace.sim_config)?;
    let world = CollisionWorld::from_content(&trace.collision, &config)?;
    let mut state = trace.initial_motor.clone();
    let mut tick = 0;
    let mut output = Vec::new();
    for segment in &trace.segments {
        for _ in 0..segment.ticks {
            tick += 1;
            let input = InputFrame {
                tick,
                move_x: InputAxis(segment.move_x),
                move_z: InputAxis(segment.move_z),
                look_yaw: Yaw(segment.look_yaw),
                look_pitch: Pitch(0.0),
                buttons: Buttons(segment.buttons),
            };
            let result = step_character(
                &state,
                &input,
                &StepContext {
                    tick,
                    collision: &world,
                    config: &config,
                },
            )
            .map_err(|e| {
                format!(
                    "trace tick {tick}, position {:?}, velocity {:?}: {e}",
                    state.position, state.velocity
                )
            })?;
            state = result.motor;
            if world
                .overlaps_character(state.position.0.map(|s| s.0), crate::QueryFilter::MOVEMENT)?
            {
                return Err(format!("trace overlap at tick {tick}"));
            }
            if trace.checkpoints.iter().any(|c| c.tick == tick) {
                output.push(Checkpoint {
                    tick,
                    motor: state.clone(),
                });
            }
        }
    }
    Ok(output)
}
pub fn verify(trace: &CharacterTrace, tolerance: f32) -> Result<TraceReport> {
    if !tolerance.is_finite() || tolerance <= 0.0 || tolerance > 0.01 {
        return Err("trace tolerance must be in (0,0.01] meters".into());
    }
    let actual = replay(trace)?;
    let mut max_error = 0.0f32;
    for (a, b) in actual.iter().zip(&trace.checkpoints) {
        let error = (0..3)
            .map(|i| (a.motor.position.0[i].0 - b.motor.position.0[i].0).powi(2))
            .sum::<f32>()
            .sqrt();
        max_error = max_error.max(error);
        let velocity_error = (0..3)
            .map(|i| (a.motor.velocity.0[i].0 - b.motor.velocity.0[i].0).powi(2))
            .sum::<f32>()
            .sqrt();
        let yaw_error = (a.motor.yaw.0 - b.motor.yaw.0)
            .sin()
            .atan2((a.motor.yaw.0 - b.motor.yaw.0).cos())
            .abs();
        if error > tolerance
            || velocity_error > tolerance
            || yaw_error > 0.001
            || a.motor.grounded != b.motor.grounded
            || a.motor.jump_cooldown_ticks != b.motor.jump_cooldown_ticks
        {
            return Err(format!(
                "trace checkpoint mismatch at tick {} (position error {error:.6}m)",
                a.tick
            ));
        }
    }
    Ok(TraceReport {
        ticks: actual.last().ok_or("no replay output")?.tick,
        checkpoints: actual.len(),
        max_position_error: max_error,
    })
}
