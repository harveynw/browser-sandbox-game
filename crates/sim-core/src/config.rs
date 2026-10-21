use crate::{Result, TICK_RATE};
/// Validated runtime constants. Fields cannot be mutated outside sim-core.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterSettings {
    pub(crate) height: f32,
    pub(crate) radius: f32,
    pub(crate) walk: f32,
    pub(crate) sprint: f32,
    pub(crate) ground_accel: f32,
    pub(crate) air_accel: f32,
    pub(crate) jump: f32,
    pub(crate) jump_cooldown: u16,
    pub(crate) gravity: f32,
    pub(crate) max_fall: f32,
    pub(crate) skin: f32,
    pub(crate) slope: f32,
    pub(crate) step: f32,
    pub(crate) snap: f32,
    pub(crate) turn: f32,
}
impl CharacterSettings {
    pub fn height(&self) -> f32 {
        self.height
    }
    pub fn radius(&self) -> f32 {
        self.radius
    }
    pub fn skin(&self) -> f32 {
        self.skin
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct SimConfig {
    pub(crate) character: CharacterSettings,
    pub(crate) vehicle: content::VehicleConfig,
}
impl SimConfig {
    pub fn from_content(source: &content::SimConfig) -> Result<Self> {
        if source.schema_version != 1 || source.tick_rate != TICK_RATE {
            return Err("simulation requires content v1 at 30 Hz".into());
        }
        let c = &source.character;
        fn bounded(n: f64, min: f64, max: f64) -> Result<f32> {
            if n.is_finite() && n >= min && n <= max {
                Ok(n as f32)
            } else {
                Err("character constant outside supported range".into())
            }
        }
        let settings = CharacterSettings {
            height: bounded(c.height, 0.5, 3.0)?,
            radius: bounded(c.radius, 0.1, 1.0)?,
            walk: bounded(c.walk_speed, 0.1, 30.0)?,
            sprint: bounded(c.sprint_speed, 0.1, 30.0)?,
            ground_accel: bounded(c.ground_acceleration, 0.1, 300.0)?,
            air_accel: bounded(c.air_acceleration, 0.1, 300.0)?,
            jump: bounded(c.jump_speed, 0.1, 20.0)?,
            jump_cooldown: u16::try_from(c.jump_cooldown_ticks)
                .map_err(|_| "jump cooldown exceeds u16")?,
            gravity: bounded(c.gravity, 0.1, 100.0)?,
            max_fall: bounded(c.max_fall_speed, 1.0, 100.0)?,
            skin: bounded(c.skin, 0.001, 0.1)?,
            slope: bounded(c.slope_limit_degrees, 1.0, 80.0)?.to_radians(),
            step: bounded(c.step_height, 0.0, 0.5)?,
            snap: bounded(c.ground_snap, 0.0, 0.5)?,
            turn: bounded(c.turn_speed, 0.1, 30.0)?,
        };
        if settings.height <= 2.0 * settings.radius
            || settings.skin >= settings.radius
            || settings.step >= settings.height
            || settings.sprint < settings.walk
            || settings.jump_cooldown == 0
        {
            return Err("inconsistent character configuration".into());
        }
        let v = &source.vehicle;
        for value in [
            v.rolling_resistance,
            v.handbrake,
            v.steering_slow,
            v.steering_fast,
            v.steering_rate,
            v.max_yaw_rate,
            v.translation_substep,
            v.yaw_substep,
        ] {
            bounded(value, 0.001, 100.0)?;
        }
        if v.max_substeps != 8
            || v.translation_substep > 0.25
            || v.yaw_substep > 0.03
            || v.steering_slow > 0.6
            || v.max_yaw_rate > 1.8
        {
            return Err("unsupported vehicle query budget".into());
        }
        Ok(Self {
            vehicle: v.clone(),
            character: settings,
        })
    }
    pub fn character(&self) -> &CharacterSettings {
        &self.character
    }
}
