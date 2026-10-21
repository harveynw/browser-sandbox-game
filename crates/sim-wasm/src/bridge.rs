use protocol::{CharacterMotor, ControlEpoch, EntityId, InputFrame, VehicleMotor};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sim_core::{CollisionQueries, CollisionWorld, QueryFilter, SimConfig, StepContext};
use ts_rs::TS;
type Result<T> = std::result::Result<T, String>;
pub const BRIDGE_VERSION: u32 = 1;
pub const CONTROL_BYTES: usize = 16_384;
pub const CONTENT_BYTES: usize = 2 * 1024 * 1024;
macro_rules! dto {
    ($name:ident { $($(#[$meta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $($(#[$meta])* pub $field: $ty),* }
    };
}
dto!(PredictedFrame {
    version: u32,
    tick: u32,
    control_epoch: ControlEpoch,
    controlled_entity_id: EntityId,
    motor: CharacterMotor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    vehicle: Option<VehiclePrediction>
});
dto!(VehiclePrediction {
    motor: VehicleMotor,
    template: content::VehicleTemplate
});
dto!(StepRequest {
    version: u32,
    control_epoch: ControlEpoch,
    controlled_entity_id: EntityId,
    input: InputFrame
});
dto!(VehicleProxy {
    entity_id: EntityId,
    template: content::VehicleTemplate,
    motor: VehicleMotor
});
dto!(ProxySet { version: u32, proxies: Vec<VehicleProxy> });
dto!(StaticRayQuery {
    version: u32,
    origin: [f32; 3],
    direction: [f32; 3],
    max_distance: f32
});
dto!(StaticSphereQuery {
    version: u32,
    origin: [f32; 3],
    direction: [f32; 3],
    max_distance: f32,
    radius: f32
});
dto!(StaticHit {
    collider_id: u32,
    distance: f32,
    point: [f32; 3],
    normal: [f32; 3]
});
dto!(StaticRayResult { version: u32, hit: Option<StaticHit> });
pub fn decode<T: DeserializeOwned>(bytes: &[u8], limit: usize) -> Result<T> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err("bridge: empty or oversized buffer".into());
    }
    let cursor = std::io::Cursor::new(bytes);
    let mut decoder = rmp_serde::Deserializer::new(cursor);
    decoder.set_max_depth(32);
    let value = T::deserialize(&mut decoder).map_err(|e| format!("bridge decode: {e}"))?;
    if decoder.get_ref().position() != bytes.len() as u64 {
        return Err("bridge: trailing bytes".into());
    }
    Ok(value)
}
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let bytes = rmp_serde::to_vec_named(value).map_err(|e| e.to_string())?;
    if bytes.len() > CONTROL_BYTES {
        return Err("bridge output limit".into());
    }
    Ok(bytes)
}
fn version(value: u32) -> Result<()> {
    if value == BRIDGE_VERSION {
        Ok(())
    } else {
        Err("bridge version mismatch".into())
    }
}
struct LoadedWorld {
    config: SimConfig,
    collision: CollisionWorld,
}
#[derive(Default)]
pub struct PredictionWorld {
    world: Option<LoadedWorld>,
    frame: Option<PredictedFrame>,
}
impl PredictionWorld {
    pub fn load_collision(&mut self, collision: &[u8], config: &[u8]) -> Result<()> {
        let source: content::SimConfig = decode(config, CONTENT_BYTES)?;
        let package: content::CollisionPackage = decode(collision, CONTENT_BYTES)?;
        let config = SimConfig::from_content(&source)?;
        let collision = CollisionWorld::from_content(&package, &config)?;
        self.world = Some(LoadedWorld { config, collision });
        self.frame = None;
        Ok(())
    }
    pub fn set_vehicle_proxies(&mut self, bytes: &[u8]) -> Result<()> {
        self.loaded()?;
        let set: ProxySet = decode(bytes, 64 * 1024)?;
        version(set.version)?;
        let mut proxies = Vec::new();
        for p in set.proxies {
            sim_core::vehicle::validate_template(&p.template)?;
            proxies.push(sim_core::collision::VehicleProxy {
                entity_id: p.entity_id.0,
                position: p.motor.position.0.map(|v| v.0),
                yaw: p.motor.yaw.0,
                dimensions: p.template.dimensions.map(|v| v as f32),
            });
        }
        self.world
            .as_mut()
            .ok_or("collision not loaded")?
            .collision
            .set_vehicle_proxies(&proxies)
    }
    fn loaded(&self) -> Result<&LoadedWorld> {
        self.world.as_ref().ok_or("collision not loaded".into())
    }
    pub fn reset(&mut self, bytes: &[u8]) -> Result<()> {
        self.loaded()?;
        let frame: PredictedFrame = decode(bytes, CONTROL_BYTES)?;
        version(frame.version)?;
        if frame.tick >= u32::MAX - 1 {
            return Err("reset tick exhausted".into());
        }
        if frame
            .vehicle
            .as_ref()
            .is_some_and(|v| frame.motor != sim_core::presentation_motor(&v.motor))
        {
            return Err("vehicle presentation motor mismatch".into());
        }
        let world = self.world.as_mut().ok_or("collision not loaded")?;
        world
            .collision
            .exclude_vehicle(frame.vehicle.as_ref().map(|_| frame.controlled_entity_id.0));
        let context = StepContext {
            tick: 1,
            collision: &world.collision,
            config: &world.config,
        };
        let validation = if let Some(vehicle) = &frame.vehicle {
            sim_core::step_vehicle(
                &vehicle.motor,
                &sim_core::neutral_input(1, vehicle.motor.yaw),
                &vehicle.template,
                &context,
            )
            .map(|_| ())
        } else {
            sim_core::step_character(
                &frame.motor,
                &sim_core::neutral_input(1, frame.motor.yaw),
                &context,
            )
            .map(|_| ())
        };
        if let Err(error) = validation {
            world.collision.exclude_vehicle(
                self.frame
                    .as_ref()
                    .and_then(|f| f.vehicle.as_ref().map(|_| f.controlled_entity_id.0)),
            );
            return Err(error);
        }
        self.frame = Some(frame);
        Ok(())
    }
    pub fn step(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        let world = self.loaded()?;
        let frame = self.frame.as_ref().ok_or("motor not reset")?;
        let request: StepRequest = decode(bytes, CONTROL_BYTES)?;
        version(request.version)?;
        if request.control_epoch != frame.control_epoch
            || request.controlled_entity_id != frame.controlled_entity_id
            || request.input.tick != frame.tick.checked_add(1).ok_or("tick exhausted")?
        {
            return Err("bridge tick/control identity mismatch".into());
        }
        let context = StepContext {
            tick: request.input.tick,
            collision: &world.collision,
            config: &world.config,
        };
        let mut next = frame.clone();
        next.tick = request.input.tick;
        if let Some(vehicle) = &frame.vehicle {
            let result = sim_core::step_vehicle(
                &vehicle.motor,
                &request.input,
                &vehicle.template,
                &context,
            )?;
            next.motor = sim_core::presentation_motor(&result.motor);
            next.vehicle = Some(VehiclePrediction {
                motor: result.motor,
                template: vehicle.template.clone(),
            });
        } else {
            next.motor = sim_core::step_character(&frame.motor, &request.input, &context)?.motor;
        }
        let output = encode(&next)?;
        self.frame = Some(next);
        Ok(output)
    }
    pub fn current_state(&self) -> Result<Vec<u8>> {
        encode(self.frame.as_ref().ok_or("motor not reset")?)
    }
    pub fn spherecast_static(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        let world = self.loaded()?;
        let query: StaticSphereQuery = decode(bytes, CONTROL_BYTES)?;
        version(query.version)?;
        let hit = world.collision.spherecast(
            query.origin,
            query.direction,
            query.max_distance,
            query.radius,
            QueryFilter::CAMERA,
        )?;
        encode(&StaticRayResult {
            version: BRIDGE_VERSION,
            hit: hit.map(|h| StaticHit {
                collider_id: h.collider_id,
                distance: h.distance,
                point: h.point,
                normal: h.normal,
            }),
        })
    }
    pub fn raycast_static(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        let world = self.loaded()?;
        let query: StaticRayQuery = decode(bytes, CONTROL_BYTES)?;
        version(query.version)?;
        let hit = world.collision.raycast(
            query.origin,
            query.direction,
            query.max_distance,
            QueryFilter::VISION,
        )?;
        encode(&StaticRayResult {
            version: BRIDGE_VERSION,
            hit: hit.map(|h| StaticHit {
                collider_id: h.collider_id,
                distance: h.distance,
                point: h.point,
                normal: h.normal,
            }),
        })
    }
}
