//! Gameplay systems own rule-specific bookkeeping, separately from world entities.
use std::result::Result;
pub mod chat;
pub mod combat;
pub mod missions;
pub mod movement;
pub mod police;
pub mod population;
pub mod repairs;
use crate::world::World;
use combat::CombatSystem;
use missions::MissionSystem;
use movement::MovementSystem;
use police::PoliceSystem;
use population::PopulationSystem;
use repairs::RepairSystem;
/// Concrete resources passed to the tick in an explicit order; no dynamic scheduler.
pub struct Systems {
    pub chat: chat::ChatSystem,
    pub movement: MovementSystem,
    pub combat: CombatSystem,
    pub missions: MissionSystem,
    pub police: PoliceSystem,
    pub population: PopulationSystem,
    pub repairs: RepairSystem,
}
impl Systems {
    pub fn new(
        world: &mut World,
        sessions: &mut crate::sessions::Sessions,
    ) -> Result<Self, String> {
        let population = PopulationSystem::new(
            &world.content.manifest,
            &world.content.config,
            world.vehicles.keys().copied(),
            424242,
        )?;
        let mut police = PoliceSystem::new(&world.content.manifest);
        for (&id, car) in &world.vehicles {
            if world.content.manifest.parking.iter().any(|slot| {
                slot.secured
                    && police::distance(
                        slot.position.map(|v| v as f32),
                        car.state.motor.position.0.map(|v| v.0),
                    ) < 0.1
            }) {
                police.secured.insert(id);
            }
        }
        let mut systems = Self {
            chat: Default::default(),
            movement: MovementSystem,
            population,
            police,
            combat: Default::default(),
            missions: Default::default(),
            repairs: Default::default(),
        };
        // Initial patrols must exist before readiness and the first client baseline.
        systems
            .police
            .spawn_patrols(0, &[], world, sessions, &mut systems.combat)?;
        Ok(systems)
    }
}
