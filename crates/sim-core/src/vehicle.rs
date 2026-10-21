//! Upright arcade bicycle motor shared by native authority and browser prediction.
use crate::{DT_SECONDS, Result, StepContext, finite3};
use content::VehicleTemplate;
use protocol::{CharacterMotor, Scalar, Steering, Vec3, VehicleMotor, Yaw};

pub fn anchor(motor: &VehicleMotor, offset: [f64; 3]) -> [f32; 3] {
    let (s, c) = motor.yaw.0.sin_cos();
    let p = motor.position.0.map(|v| v.0);
    [
        p[0] + c * offset[0] as f32 + s * offset[2] as f32,
        p[1] + offset[1] as f32,
        p[2] - s * offset[0] as f32 + c * offset[2] as f32,
    ]
}
pub fn presentation_motor(motor: &VehicleMotor) -> CharacterMotor {
    CharacterMotor {
        position: motor.position.clone(),
        velocity: motor.velocity.clone(),
        yaw: motor.yaw,
        grounded: true,
        jump_cooldown_ticks: 0,
    }
}
pub fn spawn_vehicle(position: [f32; 3], yaw: f32) -> Result<VehicleMotor> {
    crate::spawn_character(position, yaw)?;
    if position[1].abs() > 0.001 {
        return Err("vehicle must be on the flat driving plane".into());
    }
    Ok(VehicleMotor {
        position: Vec3(position.map(Scalar)),
        velocity: Vec3([Scalar(0.0); 3]),
        yaw: Yaw(yaw),
        speed: Scalar(0.0),
        steering: Steering(0.0),
    })
}
pub struct VehicleStep {
    pub motor: VehicleMotor,
    pub impact_delta_speed: f32,
    pub contacts: Vec<crate::Contact>,
}
fn approach(value: f32, target: f32, amount: f32) -> f32 {
    value + (target - value).clamp(-amount, amount)
}
pub fn validate_template(t: &VehicleTemplate) -> Result<()> {
    if t.dimensions
        .iter()
        .any(|v| !v.is_finite() || *v < 0.1 || *v > 10.0)
        || [
            t.top_speed,
            t.reverse_speed,
            t.acceleration,
            t.brake,
            t.wheelbase,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0 || *v > 100.0)
        || t.top_speed > 30.0
        || t.reverse_speed > 30.0
    {
        return Err("invalid vehicle template".into());
    }
    Ok(())
}
pub fn step_vehicle(
    state: &VehicleMotor,
    input: &protocol::InputFrame,
    template: &VehicleTemplate,
    context: &StepContext<'_>,
) -> Result<VehicleStep> {
    validate_template(template)?;
    let p = state.position.0.map(|v| v.0);
    if !finite3(p)
        || p[1].abs() > 0.001
        || !finite3(state.velocity.0.map(|v| v.0))
        || !state.speed.0.is_finite()
        || state.speed.0.abs() > 30.0
        || !state.yaw.0.is_finite()
        || state.yaw.0.abs() > std::f32::consts::PI
        || !state.steering.0.is_finite()
        || state.steering.0.abs() > 0.6
        || input.tick != context.tick
        || input.tick == 0
        || input.tick == u32::MAX
        || input.move_x.0 == i16::MIN
        || input.move_z.0 == i16::MIN
        || input.buttons.0 & !15 != 0
        || !input.look_yaw.0.is_finite()
        || input.look_yaw.0.abs() > std::f32::consts::PI
        || !input.look_pitch.0.is_finite()
        || input.look_pitch.0.abs() > 1.2
    {
        return Err("invalid vehicle motor/input".into());
    }
    let dimensions = template.dimensions.map(|v| v as f32);
    if context
        .collision
        .overlaps_vehicle(p, state.yaw.0, dimensions)?
    {
        return Err("vehicle starts obstructed".into());
    }
    let config = &context.config.vehicle;
    let throttle = f32::from(input.move_z.0) / 32767.0;
    let mut speed = state.speed.0;
    if throttle == 0.0 {
        speed = approach(speed, 0.0, config.rolling_resistance as f32 * DT_SECONDS);
    } else if speed.abs() >= 0.25 && speed.signum() != throttle.signum() {
        speed = approach(
            speed,
            0.0,
            template.brake as f32 * throttle.abs() * DT_SECONDS,
        );
    } else {
        speed += throttle * template.acceleration as f32 * DT_SECONDS;
    }
    if input.buttons.0 & 4 != 0 {
        speed = approach(speed, 0.0, config.handbrake as f32 * DT_SECONDS);
    }
    speed = speed.clamp(-template.reverse_speed as f32, template.top_speed as f32);
    let limit = config.steering_slow as f32
        + (config.steering_fast - config.steering_slow) as f32
            * (speed.abs() / template.top_speed as f32).min(1.0);
    let steering = approach(
        state.steering.0,
        -f32::from(input.move_x.0) / 32767.0 * limit,
        config.steering_rate as f32 * DT_SECONDS,
    );
    let yaw_delta = (speed / template.wheelbase as f32 * steering.tan())
        .clamp(-config.max_yaw_rate as f32, config.max_yaw_rate as f32)
        * DT_SECONDS;
    let steps = ((speed.abs() * DT_SECONDS / config.translation_substep as f32).ceil() as u32)
        .max((yaw_delta.abs() / config.yaw_substep as f32).ceil() as u32)
        .max(1);
    if steps > config.max_substeps {
        return Err("vehicle substep budget exceeded".into());
    }
    let mut position = p;
    let mut yaw = state.yaw.0;
    let mut contacts = Vec::new();
    for _ in 0..steps {
        let candidate = (yaw + yaw_delta / steps as f32 + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        if !context
            .collision
            .overlaps_vehicle(position, candidate, dimensions)?
        {
            yaw = candidate;
        }
        let delta = [
            -yaw.sin() * speed * DT_SECONDS / steps as f32,
            0.0,
            -yaw.cos() * speed * DT_SECONDS / steps as f32,
        ];
        let moved = context
            .collision
            .cast_vehicle(position, yaw, dimensions, delta)?;
        position = std::array::from_fn(|i| position[i] + moved.applied[i]);
        for contact in moved.contacts {
            if contacts.len() < 16 && !contacts.contains(&contact) {
                contacts.push(contact);
            }
        }
    }
    let velocity: [f32; 3] = std::array::from_fn(|i| (position[i] - p[i]) / DT_SECONDS);
    let applied_speed = -yaw.sin() * velocity[0] - yaw.cos() * velocity[2];
    if context
        .collision
        .overlaps_vehicle(position, yaw, dimensions)?
    {
        return Err("vehicle endpoint obstructed".into());
    }
    Ok(VehicleStep {
        motor: VehicleMotor {
            position: Vec3(position.map(Scalar)),
            yaw: Yaw(yaw),
            speed: Scalar(applied_speed),
            steering: Steering(steering),
            velocity: Vec3(velocity.map(Scalar)),
        },
        impact_delta_speed: (speed.abs() - applied_speed.abs()).max(0.0),
        contacts,
    })
}
