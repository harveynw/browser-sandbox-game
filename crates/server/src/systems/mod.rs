//! Gameplay systems own rule-specific bookkeeping, separately from world entities.
pub(crate) mod planning;
#[cfg(test)]
use crate::components::Vehicle as VehicleComponent;
#[cfg(test)]
use std::result::Result;
pub mod chat;
pub mod combat;
pub(crate) mod impact;
pub(crate) mod impact_consequences;
pub(crate) mod lifecycle;
pub mod missions;
pub mod movement;
pub mod police;
pub mod population;
pub mod repairs;
pub(crate) mod weapons;
#[cfg(test)]
use crate::world::World;
#[cfg(test)]
use combat::CombatSystem;
#[cfg(test)]
use missions::MissionSystem;
#[cfg(test)]
use movement::MovementSystem;
#[cfg(test)]
use police::PoliceSystem;
#[cfg(test)]
use population::PopulationSystem;
#[cfg(test)]
use repairs::RepairSystem;
/// Owned resource bundle used only by scenario fixtures.
#[cfg(test)]
pub struct Systems {
    pub chat: chat::ChatSystem,
    pub movement: MovementSystem,
    pub combat: CombatSystem,
    pub missions: MissionSystem,
    pub police: PoliceSystem,
    pub population: PopulationSystem,
    pub repairs: RepairSystem,
}
#[cfg(test)]
impl Systems {
    pub fn new(
        world: &mut World,
        sessions: &mut crate::sessions::Sessions,
    ) -> Result<Self, String> {
        let population = PopulationSystem::new(
            &world.content.manifest,
            &world.content.config,
            world
                .entities
                .view::<VehicleComponent>()
                .keys()
                .copied()
                .collect::<Vec<_>>()
                .into_iter(),
            424242,
            &mut world.entities,
        )?;
        let police = PoliceSystem::new(&world.content.manifest);
        for id in world
            .entities
            .view::<VehicleComponent>()
            .keys()
            .copied()
            .collect::<Vec<_>>()
        {
            let car = world
                .entities
                .view::<VehicleComponent>()
                .get(&id)
                .expect("parked vehicle");
            if world.content.manifest.parking.iter().any(|slot| {
                slot.secured
                    && police::distance(
                        slot.position.map(|v| v as f32),
                        car.state.motor.position.0.map(|v| v.0),
                    ) < 0.1
            }) {
                world.entities.insert(id, crate::components::SecuredVehicle);
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

pub(crate) mod startup;

pub(crate) mod damage;
pub(crate) mod vehicle;
