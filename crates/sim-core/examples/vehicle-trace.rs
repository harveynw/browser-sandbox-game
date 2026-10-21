//! Explicit fixture regeneration: cargo run -p sim-core --example vehicle-trace.
use content::{Authoring, BoxShape, ColliderRole, CollisionPackage, StaticCollider};
use sim_core::{
    CollisionQueries, CollisionWorld, SimConfig, StepContext, neutral_input, spawn_vehicle,
    step_vehicle,
};
fn main() {
    let source: Authoring =
        serde_json::from_str(include_str!("../../../content/source/slice.json")).unwrap();
    let config = SimConfig::from_content(&source.sim_config).unwrap();
    let template = source.vehicle_templates[0].clone();
    let collision = CollisionPackage {
        schema_version: 1,
        colliders: vec![
            StaticCollider {
                id: 1,
                shape: BoxShape::Box,
                role: ColliderRole::Ground,
                center: [0.0, -0.5, 0.0],
                half_extents: [128.0, 0.5, 128.0],
                yaw: 0.0,
                flags: 3,
            },
            StaticCollider {
                id: 2,
                shape: BoxShape::Box,
                role: ColliderRole::Building,
                center: [0.0, 2.0, -30.0],
                half_extents: [45.0, 2.0, 0.05],
                yaw: 0.0,
                flags: 3,
            },
            StaticCollider {
                id: 3,
                shape: BoxShape::Box,
                role: ColliderRole::Building,
                center: [-35.0, 2.0, 0.0],
                half_extents: [0.05, 2.0, 45.0],
                yaw: 0.0,
                flags: 3,
            },
        ],
    };
    let world = CollisionWorld::from_content(&collision, &config).unwrap();
    let initial = spawn_vehicle([0.0, 0.0, 0.0], 0.0).unwrap();
    let segments = [
        (180, 0, 32767, 0),
        (120, 32767, 32767, 0),
        (90, 0, 0, 0),
        (180, 0, -32767, 0),
        (120, -32767, -32767, 0),
        (60, 0, 0, 4),
        (300, -32767, 32767, 0),
        (300, 32767, 32767, 0),
        (300, 0, 32767, 0),
        (150, 0, 0, 0),
    ];
    let mut checkpoints = Vec::new();
    let mut motor = initial.clone();
    let mut tick = 0;
    for (ticks, x, z, buttons) in segments {
        for _ in 0..ticks {
            tick += 1;
            let mut input = neutral_input(tick, protocol::Yaw(0.0));
            input.move_x = protocol::InputAxis(x);
            input.move_z = protocol::InputAxis(z);
            input.buttons = protocol::Buttons(buttons);
            motor = step_vehicle(
                &motor,
                &input,
                &template,
                &StepContext {
                    tick,
                    collision: &world,
                    config: &config,
                },
            )
            .unwrap()
            .motor;
            assert!(
                !world
                    .overlaps_vehicle(
                        motor.position.0.map(|v| v.0),
                        motor.yaw.0,
                        template.dimensions.map(|v| v as f32)
                    )
                    .unwrap()
            );
            if tick % 30 == 0 {
                checkpoints.push(serde_json::json!({"tick":tick,"motor":motor}));
            }
        }
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"version":1,"description":"60-second native f32 compact driving trace: throttle, brake, reverse, both steering directions, handbrake and thin-wall collision.","sim_config":source.sim_config,"template":template,"collision":collision,"initial_motor":initial,"segments":segments.into_iter().map(|(ticks,move_x,move_z,buttons)|serde_json::json!({"ticks":ticks,"move_x":move_x,"move_z":move_z,"buttons":buttons})).collect::<Vec<_>>(),"checkpoints":checkpoints})).unwrap());
}
