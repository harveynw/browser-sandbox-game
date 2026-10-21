//! Explicit fixture regeneration tool. Review changed checkpoint values before accepting.
use sim_core::trace::{CharacterTrace, Checkpoint, Segment};
fn main() -> Result<(), String> {
    let authored: content::Authoring =
        serde_json::from_str(include_str!("../../../content/source/slice.json"))
            .map_err(|e| e.to_string())?;
    let collision = content::CollisionPackage {
        schema_version: 1,
        colliders: vec![
            content::StaticCollider {
                id: 1,
                shape: content::BoxShape::Box,
                role: content::ColliderRole::Ground,
                center: [0.0, -0.5, 0.0],
                half_extents: [128.0, 0.5, 128.0],
                yaw: 0.0,
                flags: 1,
            },
            content::StaticCollider {
                id: 2,
                shape: content::BoxShape::Box,
                role: content::ColliderRole::Building,
                center: [0.0, 2.0, -8.0],
                half_extents: [32.0, 2.0, 0.1],
                yaw: 0.0,
                flags: 3,
            },
            content::StaticCollider {
                id: 3,
                shape: content::BoxShape::Box,
                role: content::ColliderRole::Building,
                center: [6.0, 2.0, 0.0],
                half_extents: [0.1, 2.0, 32.0],
                yaw: 0.0,
                flags: 3,
            },
        ],
    };
    let initial = sim_core::spawn_character([0.0, 0.0, 0.0], 0.0)?;
    let segments = [
        (300, 0, 32767, 0.0, 0),
        (300, 32767, 32767, 0.0, 0),
        (300, -32767, -32767, 0.0, 0),
        (1, 0, 32767, 0.0, 2),
        (299, 0, 32767, 0.0, 1),
        (300, 0, 32767, std::f32::consts::FRAC_PI_2, 0),
        (300, 0, 0, std::f32::consts::FRAC_PI_2, 0),
    ]
    .into_iter()
    .map(|(ticks, move_x, move_z, look_yaw, buttons)| Segment {
        ticks,
        move_x,
        move_z,
        look_yaw,
        buttons,
    })
    .collect();
    let mut trace=CharacterTrace {version:1,description:"60s fixed 30Hz: walking, diagonal wall slide/corner, reverse, jump edge, sprint, yaw basis, neutral braking. Inputs are run-length encoded; all motor checkpoints are frozen native f32 outputs.".into(),sim_config_hash:content::canonical::sha256(&content::canonical::bytes(&authored.sim_config)?),sim_config:authored.sim_config,collision_hash:content::canonical::sha256(&content::canonical::bytes(&collision)?),collision,initial_motor:initial.clone(),segments,checkpoints:[30,60,120,300,360,600,900,901,910,930,1200,1500,1800].into_iter().map(|tick|Checkpoint{tick,motor:initial.clone()}).collect()};
    trace.checkpoints = sim_core::trace::replay(&trace)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&trace).map_err(|e| e.to_string())?
    );
    Ok(())
}
