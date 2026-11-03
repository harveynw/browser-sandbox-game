import { Vector3, type PerspectiveCamera } from "three";
import type { PredictionWorld } from "../simulation/bridge";
export class FollowCamera {
  private distance = 0;
  reset(): void {
    this.distance = 0;
  }
  update(
    camera: PerspectiveCamera,
    feet: readonly number[],
    yaw: number,
    pitch: number,
    dt: number,
    queries: Pick<PredictionWorld, "spherecastStatic"> &
      Partial<Pick<PredictionWorld, "raycastStatic">>,
    reducedMotion = false,
    driving = false,
    vehicleHeight = 1.4,
    aiming = false,
  ): number {
    const fov = aiming && !driving ? 54 : driving ? 70 : 65;
    const next = reducedMotion
      ? fov
      : camera.fov +
        (fov - camera.fov) * (1 - Math.exp(-Math.min(dt, 0.1) / 0.15));
    if (Math.abs(camera.fov - next) > 0.001) {
      camera.fov = next;
      camera.updateProjectionMatrix();
    }
    const range = aiming && !driving ? 1.65 : driving ? 7 : 4.5;
    const lift = driving ? Math.max(0, vehicleHeight - 1.4) : 0;
    const target = new Vector3(
      feet[0],
      (feet[1] ?? 0) + (driving ? 1.1 + lift : 1.45),
      feet[2],
    );
    const desired = new Vector3(
      Math.sin(yaw) * Math.cos(pitch) * range,
      -Math.sin(pitch) * range,
      Math.cos(yaw) * Math.cos(pitch) * range,
    );
    if (aiming && !driving) {
      desired.x += Math.cos(yaw) * 0.5;
      desired.z -= Math.sin(yaw) * 0.5;
    }
    const full = desired.length();
    desired.normalize();
    const hit = queries.spherecastStatic({
      version: 1,
      origin: target.toArray(),
      direction: desired.toArray(),
      max_distance: full,
      radius: 0.2,
    }).hit;
    const allowed = hit ? Math.max(0, hit.distance - 0.1) : full;
    if (allowed < this.distance || reducedMotion) this.distance = allowed;
    else
      this.distance +=
        (allowed - this.distance) * (1 - Math.exp(-Math.min(dt, 0.1) / 0.2));
    camera.position.copy(target).addScaledVector(desired, this.distance);
    if (aiming && !driving) {
      const direction = new Vector3(
        -Math.sin(yaw) * Math.cos(pitch),
        Math.sin(pitch),
        -Math.cos(yaw) * Math.cos(pitch),
      );
      const obstruction = queries.raycastStatic?.({
        version: 1,
        origin: target.toArray(),
        direction: direction.toArray(),
        max_distance: 120,
      }).hit;
      camera.lookAt(
        target
          .clone()
          .addScaledVector(
            direction,
            Math.max(0.05, obstruction?.distance ?? 80),
          ),
      );
      return this.distance;
    }
    // Keep the collision pivot inside the capsule; frame the view at head height.
    camera.lookAt(
      new Vector3(
        feet[0],
        (feet[1] ?? 0) + (driving ? 1.5 + lift : 1.8),
        feet[2],
      ),
    );
    return this.distance;
  }
}
