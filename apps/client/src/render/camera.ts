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
    queries: Pick<PredictionWorld, "spherecastStatic">,
    reducedMotion = false,
    driving = false,
  ): number {
    const range = driving ? 7 : 4.5;
    const target = new Vector3(
      feet[0],
      (feet[1] ?? 0) + (driving ? 1.1 : 1.45),
      feet[2],
    );
    const desired = new Vector3(
      Math.sin(yaw) * Math.cos(pitch) * range,
      -Math.sin(pitch) * range,
      Math.cos(yaw) * Math.cos(pitch) * range,
    );
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
    // Keep the collision pivot inside the capsule; frame the view at head height.
    camera.lookAt(
      new Vector3(feet[0], (feet[1] ?? 0) + (driving ? 1.5 : 1.8), feet[2]),
    );
    return this.distance;
  }
}
