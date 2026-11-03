import { Box3, Ray, Sphere, Vector3, type PerspectiveCamera } from "three";
import type { PredictionWorld } from "../simulation/bridge";
import type { RenderActor } from "../world/replica";

// Match the server's standing capsule: radius .35, segment from .35 to 1.45.
function capsuleDistance(ray: Ray, feet: readonly number[]): number | null {
  const local = ray.origin.clone().sub(new Vector3(...feet));
  const d = ray.direction;
  const candidates: number[] = [];
  const a = d.x * d.x + d.z * d.z;
  const b = 2 * (local.x * d.x + local.z * d.z);
  const c = local.x * local.x + local.z * local.z - 0.35 ** 2;
  const disc = b * b - 4 * a * c;
  if (a > 1e-10 && disc >= 0) {
    for (const t of [
      (-b - Math.sqrt(disc)) / (2 * a),
      (-b + Math.sqrt(disc)) / (2 * a),
    ]) {
      const y = local.y + t * d.y;
      if (t >= 0 && y >= 0.35 && y <= 1.45) candidates.push(t);
    }
  }
  for (const y of [0.35, 1.45]) {
    const hit = ray.intersectSphere(
      new Sphere(new Vector3(feet[0], (feet[1] ?? 0) + y, feet[2]), 0.35),
      new Vector3(),
    );
    if (hit) candidates.push(hit.distanceTo(ray.origin));
  }
  return candidates.length ? Math.min(...candidates) : null;
}

/** Select only what the centre camera ray intersects, then converge from the chest.
 * This is aim direction only: the server still decides spread, obstruction and damage.
 */
export function crosshairAim(
  camera: PerspectiveCamera,
  feet: readonly number[],
  queries: Pick<PredictionWorld, "raycastStatic">,
  actors: readonly RenderActor[],
  dimensions: (template: string) => readonly number[] | undefined,
): { yaw: number; pitch: number } {
  const direction = camera.getWorldDirection(new Vector3());
  const ray = new Ray(camera.position.clone(), direction);
  let nearest =
    queries.raycastStatic({
      version: 1,
      origin: ray.origin.toArray(),
      direction: direction.toArray(),
      max_distance: 120,
    }).hit?.distance ?? 120;
  if (direction.y < -1e-6) {
    const ground = -ray.origin.y / direction.y;
    if (ground >= 0) nearest = Math.min(nearest, ground);
  }
  for (const actor of actors) {
    let distance: number | null = null;
    if (actor.state.kind === "character") {
      if (actor.state.data.health <= 0 || actor.state.data.mode !== "on_foot")
        continue;
      distance = capsuleDistance(ray, actor.motor.position);
    } else {
      const size = dimensions(actor.descriptor.template_id);
      if (!size) continue;
      const origin = ray.origin
        .clone()
        .sub(new Vector3(...actor.motor.position));
      const axis = new Vector3(0, 1, 0);
      origin.applyAxisAngle(axis, -actor.motor.yaw);
      const local = new Ray(
        origin,
        direction.clone().applyAxisAngle(axis, -actor.motor.yaw),
      );
      const hit = local.intersectBox(
        new Box3(
          new Vector3(-(size[0] ?? 0) / 2, 0.02, -(size[2] ?? 0) / 2),
          new Vector3(
            (size[0] ?? 0) / 2,
            (size[1] ?? 0) + 0.02,
            (size[2] ?? 0) / 2,
          ),
        ),
        new Vector3(),
      );
      if (hit) distance = hit.distanceTo(origin);
    }
    if (distance !== null) nearest = Math.min(nearest, distance);
  }
  const chest = new Vector3(feet[0], (feet[1] ?? 0) + 1.45, feet[2]);
  const aim = ray.at(nearest, new Vector3()).sub(chest);
  if (aim.lengthSq() < 1e-8) aim.copy(direction);
  aim.normalize();
  return {
    yaw: Math.atan2(-aim.x, -aim.z),
    pitch: Math.max(
      -1.5,
      Math.min(1.5, Math.asin(Math.max(-1, Math.min(1, aim.y)))),
    ),
  };
}
