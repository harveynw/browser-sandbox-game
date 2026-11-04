use crate::{
    CharacterCastOptions, CollisionQueries, Contact, DT_SECONDS, QueryFilter, Result, SimConfig,
    finite3,
};
use protocol::{Buttons, CharacterMotor, InputAxis, InputFrame, Pitch, Scalar, Vec3, Yaw};
use rapier3d::prelude::Vector;
const SPRINT: u8 = 1;
const JUMP_EDGE: u8 = 2;
const AIM: u8 = 8;
pub struct StepContext<'a> {
    pub tick: u32,
    pub collision: &'a dyn CollisionQueries,
    pub config: &'a SimConfig,
}
#[derive(Debug)]
pub struct CharacterStep {
    pub tick: u32,
    pub motor: CharacterMotor,
    pub contacts: Vec<Contact>,
    pub jumped: bool,
    pub hit_ceiling: bool,
    pub endpoint_clamped: bool,
}
fn raw(v: &Vec3) -> [f32; 3] {
    v.0.map(|n| n.0)
}
fn wrapped(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}
pub fn neutral_input(tick: u32, yaw: Yaw) -> InputFrame {
    InputFrame {
        tick,
        move_x: InputAxis(0),
        move_z: InputAxis(0),
        look_yaw: yaw,
        look_pitch: Pitch(0.0),
        buttons: Buttons(0),
    }
}
pub fn spawn_character(position: [f32; 3], yaw: f32) -> Result<CharacterMotor> {
    if !finite3(position) || !yaw.is_finite() || yaw.abs() > std::f32::consts::PI {
        return Err("invalid spawn motor".into());
    }
    // Grounded is determined by a real query on the first step, never assumed.
    Ok(CharacterMotor {
        position: Vec3(position.map(Scalar)),
        velocity: Vec3([Scalar(0.0); 3]),
        yaw: Yaw(yaw),
        grounded: false,
        jump_cooldown_ticks: 0,
    })
}
/// Exactly one fixed 1/30s tick. Tick/session deduplication belongs to the session and prediction layers.
/// Errors return no partial motor; caller retains the original authoritative state.
pub fn step_character(
    state: &CharacterMotor,
    input: &InputFrame,
    context: &StepContext<'_>,
) -> Result<CharacterStep> {
    let s = context.config.character();
    if context.collision.character_settings() != s {
        return Err("collision and motor configurations differ".into());
    }
    let position = raw(&state.position);
    let old_velocity = raw(&state.velocity);
    if input.tick != context.tick
        || input.tick == 0
        || input.tick == u32::MAX
        || input.move_x.0 == i16::MIN
        || input.move_z.0 == i16::MIN
        || input.buttons.0 & !15 != 0
        || !input.look_yaw.0.is_finite()
        || input.look_yaw.0.abs() > std::f32::consts::PI
        || !input.look_pitch.0.is_finite()
        || input.look_pitch.0.abs() > 1.2
        || !state.yaw.0.is_finite()
        || state.yaw.0.abs() > std::f32::consts::PI
        || !finite3(position)
        || !finite3(old_velocity)
        || old_velocity.iter().any(|v| v.abs() > 100.0)
        || state.jump_cooldown_ticks > s.jump_cooldown
    {
        return Err("invalid character state/input/tick".into());
    }
    let position = context.collision.recover_character(position)?;
    let mut x = f32::from(input.move_x.0) / 32767.0;
    let mut z = f32::from(input.move_z.0) / 32767.0;
    let length = x.hypot(z);
    if length > 1.0 {
        x /= length;
        z /= length;
    }
    // Normalize axes first; sprint boosts only the forward component. Strafe/backward stay <=4 m/s.
    x *= s.walk;
    z *= if input.buttons.0 & SPRINT != 0 && z > 0.0 {
        s.sprint
    } else {
        s.walk
    };
    let (sin, cos) = input.look_yaw.0.sin_cos();
    let desired = Vector::new(x * cos - z * sin, 0.0, -x * sin - z * cos);
    let mut horizontal = Vector::new(old_velocity[0], 0.0, old_velocity[2]);
    let difference = desired - horizontal;
    let max_change = if state.grounded {
        s.ground_accel
    } else {
        s.air_accel
    } * DT_SECONDS;
    horizontal += difference.clamp_length_max(max_change);
    let jumped =
        input.buttons.0 & JUMP_EDGE != 0 && state.grounded && state.jump_cooldown_ticks == 0;
    let vertical =
        ((if jumped { s.jump } else { old_velocity[1] }) - s.gravity * DT_SECONDS).max(-s.max_fall);
    let movement = context.collision.cast_character(
        position,
        [
            horizontal.x * DT_SECONDS,
            vertical * DT_SECONDS,
            horizontal.z * DT_SECONDS,
        ],
        CharacterCastOptions {
            was_grounded: state.grounded && !jumped,
            allow_snap: state.grounded && !jumped && vertical <= 0.0,
            filter: QueryFilter::MOVEMENT,
        },
    )?;
    let next = std::array::from_fn(|i| position[i] + movement.applied[i]);
    if !finite3(next)
        || context
            .collision
            .overlaps_character(next, QueryFilter::MOVEMENT)?
    {
        return Err(format!(
            "character movement ended outside safe state: {next:?}"
        ));
    }
    let vx = movement.applied[0] / DT_SECONDS;
    let vz = movement.applied[2] / DT_SECONDS;
    let vy = if movement.grounded && vertical <= 0.0 || movement.hit_ceiling && vertical > 0.0 {
        0.0
    } else {
        vertical
    };
    let target_yaw = if input.buttons.0 & AIM != 0 {
        input.look_yaw.0
    // Ground contact corrections can alternate sideways even with no movement.
    // Only meaningful intended motion may change walking facing.
    } else if horizontal.length() > 0.05 && vx.hypot(vz) > 0.05 {
        (-vx).atan2(-vz)
    } else {
        state.yaw.0
    };
    let yaw = wrapped(
        state.yaw.0
            + wrapped(target_yaw - state.yaw.0).clamp(-s.turn * DT_SECONDS, s.turn * DT_SECONDS),
    );
    let motor = CharacterMotor {
        position: Vec3(next.map(Scalar)),
        velocity: Vec3([vx, vy, vz].map(Scalar)),
        yaw: Yaw(yaw),
        grounded: movement.grounded && vertical <= 0.0,
        jump_cooldown_ticks: if jumped {
            s.jump_cooldown.saturating_sub(1)
        } else {
            state.jump_cooldown_ticks.saturating_sub(1)
        },
    };
    Ok(CharacterStep {
        tick: context.tick,
        motor,
        contacts: movement.contacts,
        jumped,
        hit_ceiling: movement.hit_ceiling,
        endpoint_clamped: movement.endpoint_clamped,
    })
}
