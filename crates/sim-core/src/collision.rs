use crate::{CharacterSettings, DT_SECONDS, Result, finite3};
use content::CollisionPackage;
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::{
    control::{CharacterAutostep, CharacterLength, KinematicCharacterController},
    prelude::*,
};
use std::collections::BTreeSet;
const OVERLAP_TOLERANCE: f32 = 0.0001;
const MAX_CONTACTS: usize = 16;
#[derive(Clone, Copy, Debug)]
pub struct QueryFilter {
    pub flags: u32,
    pub exclude_static_id: Option<u32>,
}
impl QueryFilter {
    pub const MOVEMENT: Self = Self {
        flags: 1,
        exclude_static_id: None,
    };
    /// Cameras collide with both physical surfaces (including ground) and vision blockers.
    pub const CAMERA: Self = Self {
        flags: 3,
        exclude_static_id: None,
    };
    pub const VISION: Self = Self {
        flags: 2,
        exclude_static_id: None,
    };
}
#[derive(Clone, Copy, Debug)]
pub struct CharacterCastOptions {
    pub was_grounded: bool,
    pub allow_snap: bool,
    pub filter: QueryFilter,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    pub collider_id: u32,
    pub normal: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct MoveResult {
    pub applied: [f32; 3],
    pub grounded: bool,
    pub hit_ceiling: bool,
    pub endpoint_clamped: bool,
    pub contacts: Vec<Contact>,
}
#[derive(Clone, Debug)]
pub struct RayHit {
    pub collider_id: u32,
    pub distance: f32,
    pub point: [f32; 3],
    pub normal: [f32; 3],
}
pub trait CollisionQueries {
    /// Resolve only newly overlapping moving car proxies; static penetrations fail.
    fn recover_character(&self, feet: [f32; 3]) -> Result<[f32; 3]>;
    fn cast_vehicle(
        &self,
        position: [f32; 3],
        yaw: f32,
        dimensions: [f32; 3],
        delta: [f32; 3],
    ) -> Result<MoveResult>;
    fn overlaps_vehicle(&self, position: [f32; 3], yaw: f32, dimensions: [f32; 3]) -> Result<bool>;
    fn character_settings(&self) -> &CharacterSettings;
    fn cast_character(
        &self,
        feet: [f32; 3],
        delta: [f32; 3],
        options: CharacterCastOptions,
    ) -> Result<MoveResult>;
    fn overlaps_character(&self, feet: [f32; 3], filter: QueryFilter) -> Result<bool>;
    fn raycast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
        filter: QueryFilter,
    ) -> Result<Option<RayHit>>;
}
/// Static immutable query world. Build the BVH once; movement never rebuilds it.
/// Bounded car proxy slots update only changed shapes/poses; static BVH is retained.
pub struct CollisionWorld {
    settings: CharacterSettings,
    colliders: ColliderSet,
    bodies: RigidBodySet,
    broad: BroadPhaseBvh,
    narrow: NarrowPhase,
    shape: SharedShape,
    proxies: Vec<(ColliderHandle, Option<VehicleProxy>)>,
    excluded_vehicle: Option<u32>,
}
fn metadata(c: &Collider) -> (u32, u32) {
    ((c.user_data >> 32) as u32, c.user_data as u32)
}

#[derive(Clone, Debug, PartialEq)]
pub struct VehicleProxy {
    pub entity_id: u32,
    pub position: [f32; 3],
    pub yaw: f32,
    pub dimensions: [f32; 3],
}
fn vehicle_shape(position: [f32; 3], yaw: f32, dimensions: [f32; 3]) -> Collider {
    ColliderBuilder::cuboid(
        dimensions[0] / 2.0,
        dimensions[1] / 2.0,
        dimensions[2] / 2.0,
    )
    .translation(Vector::new(
        position[0],
        position[1] + dimensions[1] / 2.0 + 0.02,
        position[2],
    ))
    .rotation(Vector::new(0.0, yaw, 0.0))
    .build()
}
impl CollisionWorld {
    pub fn exclude_vehicle(&mut self, id: Option<u32>) {
        self.excluded_vehicle = id;
    }
    fn admitted(&self, c: &Collider, filter: QueryFilter) -> bool {
        if c.user_data >> 64 != 0 {
            filter.flags & 1 != 0
                && metadata(c).0 != self.excluded_vehicle.unwrap_or(0)
                && metadata(c).1 != 0
        } else {
            admitted(c, filter)
        }
    }
    pub fn set_vehicle_proxies(&mut self, values: &[VehicleProxy]) -> Result<()> {
        if values.len() > 96 {
            return Err("vehicle proxy cap".into());
        }
        let mut ids = BTreeSet::new();
        for p in values {
            if p.entity_id == 0
                || !ids.insert(p.entity_id)
                || !finite3(p.position)
                || !p.yaw.is_finite()
                || p.yaw.abs() > std::f32::consts::PI
                || p.dimensions
                    .iter()
                    .any(|v| !v.is_finite() || *v < 0.1 || *v > 10.0)
            {
                return Err("invalid vehicle proxy".into());
            }
        }
        let mut changed = false;
        for (handle, old) in &mut self.proxies {
            if old.as_ref().is_some_and(|p| !ids.contains(&p.entity_id)) {
                changed = true;
                self.colliders[*handle].user_data = 1u128 << 64;
                *old = None;
            }
        }
        for p in values {
            let slot = self
                .proxies
                .iter()
                .position(|(_, v)| v.as_ref().is_some_and(|old| old.entity_id == p.entity_id));
            if slot.is_some_and(|i| self.proxies[i].1.as_ref() == Some(p)) {
                continue;
            }
            changed = true;
            let mut c = vehicle_shape(p.position, p.yaw, p.dimensions);
            c.user_data = (1u128 << 64) | (u128::from(p.entity_id) << 32) | 1;
            let index = slot.or_else(|| self.proxies.iter().position(|(_, v)| v.is_none()));
            let handle = if let Some(index) = index {
                let handle = self.proxies[index].0;
                self.colliders[handle].set_shape(c.shared_shape().clone());
                self.colliders[handle].set_position(*c.position());
                self.colliders[handle].user_data = c.user_data;
                self.proxies[index].1 = Some(p.clone());
                handle
            } else {
                let handle = self.colliders.insert(c);
                self.proxies.push((handle, Some(p.clone())));
                handle
            };
            self.broad.set_aabb(
                &IntegrationParameters::default(),
                handle,
                self.colliders[handle].compute_aabb(),
            );
        }
        if changed {
            // set_aabb retains pending change notifications even in query-only
            // worlds. Drain them so moving proxies cannot grow that queue.
            self.broad.update(
                &IntegrationParameters::default(),
                &self.colliders,
                &self.bodies,
                &[],
                &[],
                &mut Vec::new(),
            );
        }
        Ok(())
    }

    pub fn from_content(package: &CollisionPackage, config: &crate::SimConfig) -> Result<Self> {
        if package.schema_version != 1 || package.colliders.len() > 4096 {
            return Err("collision version/count unsupported".into());
        }
        let settings = config.character().clone();
        let mut world = Self {
            proxies: Vec::new(),
            excluded_vehicle: None,
            shape: SharedShape::capsule_y(settings.height / 2.0 - settings.radius, settings.radius),
            settings,
            colliders: ColliderSet::new(),
            bodies: RigidBodySet::new(),
            broad: BroadPhaseBvh::new(),
            narrow: NarrowPhase::new(),
        };
        let mut seen = BTreeSet::new();
        let mut source: Vec<_> = package.colliders.iter().collect();
        source.sort_by_key(|c| c.id);
        for c in source {
            let center = c.center.map(|n| n as f32);
            let half = c.half_extents.map(|n| n as f32);
            let yaw = c.yaw as f32;
            if c.id == 0
                || !seen.insert(c.id)
                || !finite3(center)
                || !finite3(half)
                || half.iter().any(|n| *n < 0.001 || *n > 10_000.0)
                || !yaw.is_finite()
                || yaw.abs() > std::f32::consts::PI
                || c.flags == 0
                || c.flags & !3 != 0
            {
                return Err("invalid static collider".into());
            }
            let collider = ColliderBuilder::cuboid(half[0], half[1], half[2])
                .translation(Vector::from_array(center))
                .rotation(Vector::new(0.0, yaw, 0.0))
                .user_data((u128::from(c.id) << 32) | u128::from(c.flags))
                .build();
            let aabb = collider.compute_aabb();
            let handle = world.colliders.insert(collider);
            world
                .broad
                .set_aabb(&IntegrationParameters::default(), handle, aabb);
        }
        Ok(world)
    }
    pub fn collider_count(&self) -> usize {
        self.colliders.len()
    }
    fn queries<'a>(
        &'a self,
        predicate: &'a impl Fn(ColliderHandle, &Collider) -> bool,
    ) -> rapier3d::pipeline::QueryPipeline<'a> {
        self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            rapier3d::pipeline::QueryFilter::default().predicate(predicate),
        )
    }
    /// Sweep a camera-sized sphere through the immutable static BVH.
    pub fn spherecast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
        radius: f32,
        filter: QueryFilter,
    ) -> Result<Option<RayHit>> {
        validate_filter(filter)?;
        if !finite3(origin)
            || !finite3(direction)
            || !max_distance.is_finite()
            || !(0.0..=10000.0).contains(&max_distance)
            || !radius.is_finite()
            || !(0.001..=2.0).contains(&radius)
        {
            return Err("invalid sphere cast".into());
        }
        let direction = Vector::from_array(direction)
            .try_normalize()
            .ok_or("sphere direction cannot be zero")?;
        let predicate =
            |_: ColliderHandle, c: &Collider| c.user_data >> 64 == 0 && admitted(c, filter);
        let queries = self.queries(&predicate);
        let shape = Ball::new(radius);
        let pose = Pose::translation(origin[0], origin[1], origin[2]);
        // Explicitly catch an obstructed target, including casts moving out of a solid.
        if let Some((handle, _)) = queries.intersect_shape(pose, &shape).next() {
            return Ok(Some(RayHit {
                collider_id: metadata(&self.colliders[handle]).0,
                distance: 0.0,
                point: origin,
                normal: (-direction).to_array(),
            }));
        }
        Ok(queries
            .cast_shape(
                &pose,
                direction,
                &shape,
                ShapeCastOptions {
                    max_time_of_impact: max_distance,
                    stop_at_penetration: true,
                    ..Default::default()
                },
            )
            .map(|(handle, hit)| RayHit {
                collider_id: metadata(&self.colliders[handle]).0,
                distance: hit.time_of_impact,
                point: (Vector::from_array(origin) + direction * hit.time_of_impact).to_array(),
                normal: hit.normal1.to_array(),
            }))
    }
    fn pose(&self, feet: [f32; 3]) -> Pose {
        Pose::translation(feet[0], feet[1] + self.settings.height / 2.0, feet[2])
    }
}
fn admitted(c: &Collider, filter: QueryFilter) -> bool {
    let (id, flags) = metadata(c);
    flags & filter.flags != 0 && filter.exclude_static_id != Some(id)
}
fn validate_filter(filter: QueryFilter) -> Result<()> {
    if filter.flags == 0 || filter.flags & !3 != 0 {
        Err("invalid query flags".into())
    } else {
        Ok(())
    }
}
impl CollisionQueries for CollisionWorld {
    fn recover_character(&self, feet: [f32; 3]) -> Result<[f32; 3]> {
        if !self.overlaps_character(feet, QueryFilter::MOVEMENT)? {
            return Ok(feet);
        }
        let predicate = |_: ColliderHandle, c: &Collider| {
            c.user_data >> 64 == 0 && admitted(c, QueryFilter::MOVEMENT)
        };
        let queries = self.queries(&predicate);
        let capsule = Capsule::new_y(
            self.settings.height / 2.0 - self.settings.radius,
            self.settings.radius - OVERLAP_TOLERANCE,
        );
        if queries
            .intersect_shape(self.pose(feet), &capsule)
            .next()
            .is_some()
        {
            return Err("character starts inside solid geometry".into());
        }
        let mut candidates = Vec::new();
        for (_, proxy) in &self.proxies {
            let Some(p) = proxy else {
                continue;
            };
            if self.excluded_vehicle == Some(p.entity_id) {
                continue;
            }
            let (s, c) = p.yaw.sin_cos();
            let dx = feet[0] - p.position[0];
            let dz = feet[2] - p.position[2];
            let x = c * dx - s * dz;
            let z = s * dx + c * dz;
            let hx = p.dimensions[0] / 2.0 + self.settings.radius + 0.04;
            let hz = p.dimensions[2] / 2.0 + self.settings.radius + 0.04;
            if x.abs() > hx || z.abs() > hz || feet[1] > p.position[1] + p.dimensions[1] + 0.02 {
                continue;
            }
            for (cx, cz) in [(-hx, z), (hx, z), (x, -hz), (x, hz)] {
                let target = [
                    p.position[0] + c * cx + s * cz,
                    feet[1],
                    p.position[2] - s * cx + c * cz,
                ];
                let delta = Vector::from_array(target) - Vector::from_array(feet);
                if !self.overlaps_character(target, QueryFilter::MOVEMENT)?
                    && queries
                        .cast_shape(
                            &self.pose(feet),
                            delta,
                            self.shape.as_ref(),
                            ShapeCastOptions {
                                max_time_of_impact: 1.0,
                                stop_at_penetration: false,
                                ..Default::default()
                            },
                        )
                        .is_none()
                {
                    candidates.push((delta.length_squared(), target));
                }
            }
        }
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
        candidates
            .first()
            .map(|c| c.1)
            .ok_or_else(|| "character trapped by car proxies".into())
    }

    fn overlaps_vehicle(&self, position: [f32; 3], yaw: f32, dimensions: [f32; 3]) -> Result<bool> {
        let c = vehicle_shape(position, yaw, dimensions.map(|d| d - 0.0002));
        let predicate = |_: ColliderHandle, c: &Collider| self.admitted(c, QueryFilter::MOVEMENT);
        Ok(self
            .queries(&predicate)
            .intersect_shape(*c.position(), c.shape())
            .next()
            .is_some())
    }
    fn cast_vehicle(
        &self,
        position: [f32; 3],
        yaw: f32,
        dimensions: [f32; 3],
        delta: [f32; 3],
    ) -> Result<MoveResult> {
        let c = vehicle_shape(position, yaw, dimensions);
        // Flat driving already supplies floor support. Surfaces below the car's
        // bottom must not generate grazing GJK normals that lift/slow the car.
        let predicate = |_: ColliderHandle, c: &Collider| {
            self.admitted(c, QueryFilter::MOVEMENT) && c.compute_aabb().maxs.y > position[1] + 0.02
        };
        let mut contacts = Vec::new();
        let controller = KinematicCharacterController {
            offset: CharacterLength::Absolute(0.02),
            autostep: None,
            snap_to_ground: None,
            ..Default::default()
        };
        let movement = controller.move_shape(
            DT_SECONDS,
            &self.queries(&predicate),
            c.shape(),
            c.position(),
            Vector::from_array(delta),
            |hit| {
                if contacts.len() < 16 {
                    contacts.push(Contact {
                        collider_id: metadata(&self.colliders[hit.handle]).0,
                        normal: hit.hit.normal1.to_array(),
                    });
                }
            },
        );
        // The car motor is constrained to the flat driving plane. The generic
        // controller can add vertical sliding at curb/box edges even for a
        // horizontal request; never commit that displacement to a car.
        let mut applied = movement.translation.to_array();
        applied[1] = 0.0;
        let endpoint = std::array::from_fn(|i| position[i] + applied[i]);
        if self.overlaps_vehicle(endpoint, yaw, dimensions)? {
            applied = [0.0; 3];
        }
        Ok(MoveResult {
            applied,
            grounded: true,
            hit_ceiling: false,
            endpoint_clamped: false,
            contacts,
        })
    }

    fn character_settings(&self) -> &CharacterSettings {
        &self.settings
    }
    fn cast_character(
        &self,
        feet: [f32; 3],
        delta: [f32; 3],
        options: CharacterCastOptions,
    ) -> Result<MoveResult> {
        validate_filter(options.filter)?;
        if !finite3(feet) || !finite3(delta) || Vector::from_array(delta).length() > 10.0 {
            return Err("invalid/beyond-budget character cast".into());
        }
        let predicate = |_: ColliderHandle, c: &Collider| self.admitted(c, options.filter);
        let queries = self.queries(&predicate);
        let s = &self.settings;
        let controller = KinematicCharacterController {
            offset: CharacterLength::Absolute(s.skin),
            autostep: if options.was_grounded && delta[1] <= 0.0 && s.step > 0.0 {
                Some(CharacterAutostep {
                    max_height: CharacterLength::Absolute(s.step),
                    min_width: CharacterLength::Absolute(2.0 * s.radius),
                    include_dynamic_bodies: false,
                })
            } else {
                None
            },
            max_slope_climb_angle: s.slope,
            min_slope_slide_angle: s.slope,
            snap_to_ground: if options.allow_snap && delta[1] <= 0.0 && s.snap > 0.0 {
                Some(CharacterLength::Absolute(s.snap))
            } else {
                None
            },
            ..Default::default()
        };
        // GJK shape casts at exact surface contact can have an ambiguous normal.
        // Lift only an exactly supported foot to the configured skin, after checking headroom.
        let mut start = feet;
        let support_ray = Ray::new(Vector::new(feet[0], feet[1] + s.skin, feet[2]), -Vector::Y);
        if let Some((_, hit)) = queries.cast_ray_and_get_normal(&support_ray, 2.0 * s.skin, true)
            && hit.normal.y >= s.slope.cos()
            && hit.time_of_impact < 2.0 * s.skin
        {
            let raised = [
                feet[0],
                feet[1] + 2.0 * s.skin - hit.time_of_impact,
                feet[2],
            ];
            if !self.overlaps_character(raised, options.filter)? {
                start = raised;
            }
        }
        let mut contacts = Vec::new();
        let mut hit_ceiling = false;
        let mut callback = |collision: rapier3d::control::CharacterCollision| {
            let normal = collision.hit.normal1.to_array();
            hit_ceiling |= normal[1] < -0.5;
            let id = metadata(&self.colliders[collision.handle]).0;
            if contacts.len() < MAX_CONTACTS
                && !contacts
                    .iter()
                    .any(|c: &Contact| c.collider_id == id && c.normal == normal)
            {
                contacts.push(Contact {
                    collider_id: id,
                    normal,
                });
            }
        };
        // Separate horizontal/vertical casts avoid diagonal ground-contact instability.
        // Every phase is swept. Resolve rising motion first, falling motion last.
        let horizontal = Vector::new(delta[0], 0.0, delta[2]);
        let vertical = Vector::new(0.0, delta[1], 0.0);
        let phases = if delta[1] > 0.0 {
            [vertical, horizontal]
        } else {
            [horizontal, vertical]
        };
        let mut applied = Vector::ZERO;
        let mut grounded = false;
        for (index, phase) in phases.into_iter().enumerate() {
            let mut phase_controller = controller;
            phase_controller.snap_to_ground = None;
            if phase.y != 0.0 {
                phase_controller.autostep = None;
            }
            let movement = phase_controller.move_shape(
                DT_SECONDS,
                &queries,
                self.shape.as_ref(),
                &(Pose::from_translation(applied) * self.pose(start)),
                phase,
                &mut callback,
            );
            applied += movement.translation;
            if delta[1] <= 0.0 && index == 1 {
                grounded = movement.grounded;
            }
        }
        if options.allow_snap && options.was_grounded && delta[1] <= 0.0 && !grounded {
            let pose = Pose::from_translation(applied) * self.pose(start);
            if let Some((_, hit)) = queries.cast_shape(
                &pose,
                -Vector::Y,
                self.shape.as_ref(),
                ShapeCastOptions {
                    max_time_of_impact: s.snap,
                    target_distance: s.skin,
                    stop_at_penetration: false,
                    compute_impact_geometry_on_penetration: true,
                },
            ) && hit.normal1.y >= s.slope.cos()
            {
                applied.y -= hit.time_of_impact;
                grounded = true;
            }
        }
        // Restore the configured floor gap after the casts. Near-contact GJK results
        // may undershoot a flat support surface by a few floating-point ulps.
        if delta[1] <= 0.0 {
            let end = Vector::from_array(start) + applied;
            let ray = Ray::new(end + Vector::Y * s.skin, -Vector::Y);
            if let Some((_, hit)) = queries.cast_ray_and_get_normal(&ray, 2.0 * s.skin, true)
                && hit.normal.y >= s.slope.cos()
                && hit.time_of_impact < 2.0 * s.skin
            {
                let correction = 2.0 * s.skin - hit.time_of_impact;
                let raised = (end + Vector::Y * correction).to_array();
                if !self.overlaps_character(raised, options.filter)? {
                    applied.y += correction;
                    grounded = true;
                }
            }
        }
        // A grazing capsule/box cast can underestimate a corner by millimeters.
        // Keep the overlap test strict and conservatively clip to a clear endpoint.
        let end = (Vector::from_array(start) + applied).to_array();
        let endpoint_clamped = self.overlaps_character(end, options.filter)?;
        if endpoint_clamped {
            if self.overlaps_character(start, options.filter)? {
                return Err("character cast starts inside geometry".into());
            }
            let mut low = 0.0;
            let mut high = 1.0;
            for _ in 0..14 {
                let mid = (low + high) / 2.0;
                let candidate = (Vector::from_array(start) + applied * mid).to_array();
                if self.overlaps_character(candidate, options.filter)? {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            applied *= low;
            let end = Vector::from_array(start) + applied;
            grounded = delta[1] <= 0.0
                && queries
                    .cast_ray_and_get_normal(
                        &Ray::new(end + Vector::Y * s.skin, -Vector::Y),
                        2.0 * s.skin + 0.0001,
                        true,
                    )
                    .is_some_and(|(_, h)| h.normal.y >= s.slope.cos());
        }
        let movement = rapier3d::control::EffectiveCharacterMovement {
            translation: applied,
            grounded,
            is_sliding_down_slope: false,
        };
        if !movement.translation.is_finite() {
            return Err("collision query produced non-finite movement".into());
        }
        Ok(MoveResult {
            applied: (movement.translation + Vector::from_array(start) - Vector::from_array(feet))
                .to_array(),
            grounded: movement.grounded,
            hit_ceiling,
            endpoint_clamped,
            contacts,
        })
    }
    fn overlaps_character(&self, feet: [f32; 3], filter: QueryFilter) -> Result<bool> {
        validate_filter(filter)?;
        if !finite3(feet) {
            return Err("invalid overlap position".into());
        }
        let predicate = |_: ColliderHandle, c: &Collider| self.admitted(c, filter);
        let q = self.queries(&predicate);
        // Ignore exact surface contact; a Y=0 authored spawn may touch the ground.
        let shape = Capsule::new_y(
            self.settings.height / 2.0 - self.settings.radius,
            self.settings.radius - OVERLAP_TOLERANCE,
        );
        Ok(q.intersect_shape(self.pose(feet), &shape).next().is_some())
    }
    fn raycast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
        filter: QueryFilter,
    ) -> Result<Option<RayHit>> {
        validate_filter(filter)?;
        if !finite3(origin)
            || !finite3(direction)
            || !max_distance.is_finite()
            || !(0.0..=10000.0).contains(&max_distance)
        {
            return Err("invalid ray".into());
        }
        let direction = Vector::from_array(direction)
            .try_normalize()
            .ok_or("ray direction cannot be zero")?;
        let predicate = |_: ColliderHandle, c: &Collider| self.admitted(c, filter);
        let q = self.queries(&predicate);
        let ray = Ray::new(Vector::from_array(origin), direction);
        Ok(q.cast_ray_and_get_normal(&ray, max_distance, true)
            .map(|(handle, hit)| RayHit {
                collider_id: metadata(&self.colliders[handle]).0,
                distance: hit.time_of_impact,
                point: (ray.origin + direction * hit.time_of_impact).to_array(),
                normal: hit.normal.to_array(),
            }))
    }
}
