//! Deterministic shot geometry used by the ordered weapon execution system.
use crate::components::*;
use protocol::*;

pub(crate) fn shot_ray(player: &Player, aim: &Fire, tick: u32) -> ([f32; 3], [f32; 3]) {
    let p = player.state.motor.position.0.map(|v| v.0);
    let moving = player.state.motor.velocity.0[0]
        .0
        .hypot(player.state.motor.velocity.0[2].0)
        > 0.5;
    let definition = crate::systems::weapons::state::definition(&player.weapon);
    let spread = (definition.spread_degrees as f32
        * if moving {
            definition.moving_multiplier as f32
        } else {
            1.0
        }
        + player.weapon.slots[player.weapon.equipped.index()].bloom)
        .to_radians();
    let seed = player.id.0.wrapping_mul(73856093)
        ^ player.weapon.shot_sequence.wrapping_mul(19349663)
        ^ tick;
    let mix = |v: u32| {
        let v = (v ^ (v >> 16)).wrapping_mul(0x7feb352d);
        (v ^ (v >> 15)).wrapping_mul(0x846ca68b)
    };
    let u = mix(seed) as f32 / u32::MAX as f32;
    let angle = (mix(seed ^ 0x12345678) as f32 / u32::MAX as f32) * std::f32::consts::TAU;
    let radius = u.sqrt() * spread;
    let (sin, cos) = (aim.aim_yaw.0 + angle.cos() * radius).sin_cos();
    let (sp, cp) = (aim.aim_pitch.0 + angle.sin() * radius)
        .clamp(-1.5, 1.5)
        .sin_cos();
    ([p[0], p[1] + 1.45, p[2]], [-sin * cp, sp, -cos * cp])
}
