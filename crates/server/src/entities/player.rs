//! A connected player character and its equipment.
use protocol::*;
#[derive(bevy_ecs::prelude::Component)]
pub struct Player {
    pub id: EntityId,
    pub state: CharacterState,
    pub tshirt_colour: TshirtColour,
    pub display_name: DisplayName,
    pub lifecycle: crate::entities::weapon::Lifecycle,
    pub weapon: crate::entities::weapon::Weapon,
    pub(crate) slot: usize,
}

impl Player {
    pub(crate) fn shot_ray(&self, aim: &Fire, tick: u32) -> ([f32; 3], [f32; 3]) {
        let p = self.state.motor.position.0.map(|v| v.0);
        let moving = self.state.motor.velocity.0[0]
            .0
            .hypot(self.state.motor.velocity.0[2].0)
            > 0.5;
        let definition = self.weapon.definition();
        let spread = (definition.spread_degrees as f32
            * if moving {
                definition.moving_multiplier as f32
            } else {
                1.0
            }
            + self.weapon.bloom)
            .to_radians();
        let seed = self.id.0.wrapping_mul(73856093)
            ^ self.weapon.shot_sequence.wrapping_mul(19349663)
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

    pub fn public_state(&self) -> CharacterState {
        let mut state = self.state.clone();
        state.equipment = Some(EquipmentView {
            weapon_id: self.weapon.equipped,
            aiming: self.weapon.aiming
                && self.state.mode == CharacterMode::OnFoot
                && self.state.health.0 > 0,
            aim_pitch: self.weapon.aim_pitch,
            reload_end_tick: self.weapon.reload_end,
            equip_end_tick: self.weapon.equip_end,
        });
        state
    }
}
