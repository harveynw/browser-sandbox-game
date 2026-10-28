import * as THREE from "three";
import type { CharacterMotor } from "@port-meridian/protocol";
export type Clip =
  | "idle"
  | "walk"
  | "run"
  | "jump_start"
  | "fall"
  | "land"
  | "aim_idle"
  | "aim_walk"
  | "fire"
  | "reload"
  | "driving"
  | "incapacitated";
export interface AnimationInput {
  motor: CharacterMotor;
  driving?: boolean;
  incapacitated?: boolean;
  aiming?: boolean;
  reloading?: boolean;
  fired?: boolean;
}
export class AnimationStateMachine {
  clip: Clip = "idle";
  speed = 0;
  phase = 0;
  private grounded: boolean | undefined;
  private edgeUntil = 0;
  private edge: Clip = "idle";
  update(input: AnimationInput, dt: number, time: number): Clip {
    const m = input.motor,
      speed = Math.hypot(m.velocity[0], m.velocity[2]);
    this.speed +=
      (speed - this.speed) * (1 - Math.exp(-Math.min(dt, 0.1) / 0.15));
    this.phase += dt * this.speed * 2.8;
    if (this.grounded !== undefined && m.grounded !== this.grounded) {
      this.edge = m.grounded ? "land" : "jump_start";
      this.edgeUntil = time + 0.14;
    }
    this.grounded = m.grounded;
    if (input.fired) {
      this.edge = "fire";
      this.edgeUntil = time + 0.12;
    }
    return (this.clip = input.incapacitated
      ? "incapacitated"
      : input.driving
        ? "driving"
        : input.reloading
          ? "reload"
          : time < this.edgeUntil
            ? this.edge
            : !m.grounded
              ? "fall"
              : input.aiming
                ? this.speed > 0.4
                  ? "aim_walk"
                  : "aim_idle"
                : this.speed > 4.6
                  ? "run"
                  : this.speed > 0.25
                    ? "walk"
                    : "idle");
  }
}
export const clothing = ["#e4a25b", "#4a9d9a", "#b16f5b", "#7f83a9"];
/** One articulated rig with independently configurable shirt and skin colours. */
export class CharacterRig {
  private readonly shirtParts: THREE.Mesh[] = [];
  private shirtColour: number | undefined;
  setShirtColour(colour: number): void {
    if (this.shirtColour === colour) return;
    this.shirtColour = colour;
    const material = this.material(`#${colour.toString(16).padStart(6, "0")}`);
    for (const part of this.shirtParts) part.material = material;
  }
  readonly root = new THREE.Group();
  private pendingFire = false;
  fire(): void {
    this.pendingFire = true;
  }
  readonly animation = new AnimationStateMachine();
  private torso = new THREE.Group();
  private head = new THREE.Group();
  private arms: [THREE.Group, THREE.Group] = [
    new THREE.Group(),
    new THREE.Group(),
  ];
  private legs: [THREE.Group, THREE.Group] = [
    new THREE.Group(),
    new THREE.Group(),
  ];
  constructor(
    cube: THREE.BoxGeometry,
    private readonly material: (color: string) => THREE.MeshStandardMaterial,
    palette = 0,
  ) {
    const shirt = clothing[palette % 4] ?? "#e4a25b",
      skin =
        ["#c79370", "#875b49", "#dbb18a", "#aa785a"][palette % 4] ?? "#c79370";
    const part = (
      parent: THREE.Group,
      color: string,
      size: [number, number, number],
      at: [number, number, number],
    ) => {
      const mesh = new THREE.Mesh(cube, material(color));
      if (color === shirt) this.shirtParts.push(mesh);
      mesh.scale.set(...size);
      mesh.position.set(...at);
      parent.add(mesh);
      return mesh;
    };
    this.torso.position.y = 1.05;
    this.root.add(this.torso);
    part(this.torso, shirt, [0.48, 0.55, 0.3], [0, 0, 0]);
    this.head.position.y = 0.43;
    this.torso.add(this.head);
    part(this.head, skin, [0.3, 0.34, 0.29], [0, 0, 0]);
    part(this.head, "#293c46", [0.31, 0.09, 0.3], [0, 0.14, 0]);
    part(this.head, "#26343b", [0.2, 0.06, 0.015], [0, 0.015, -0.15]);
    this.arms.forEach((arm, i) => {
      arm.position.set(i === 0 ? -0.31 : 0.31, 0.19, 0);
      this.torso.add(arm);
      part(arm, shirt, [0.17, 0.3, 0.2], [0, -0.13, 0]);
      part(arm, skin, [0.14, 0.27, 0.16], [0, -0.4, 0]);
    });
    this.legs.forEach((leg, i) => {
      leg.position.set(i === 0 ? -0.13 : 0.13, 0.8, 0);
      this.root.add(leg);
      part(leg, "#354754", [0.2, 0.65, 0.24], [0, -0.32, 0]);
      part(leg, "#24343a", [0.22, 0.14, 0.33], [0, -0.72, -0.04]);
    });
  }
  pose(
    input: AnimationInput,
    dt: number,
    time: number,
    reduced: boolean,
  ): void {
    const clip = this.animation.update(
        { ...input, fired: input.fired || this.pendingFire },
        dt,
        time,
      ),
      moving = ["walk", "run", "aim_walk"].includes(clip),
      swing = moving
        ? Math.sin(this.animation.phase) * (clip === "run" ? 0.85 : 0.5)
        : 0;
    this.pendingFire = false;
    this.root.position.y = input.driving ? -0.77 : 0;
    this.root.rotation.z = clip === "incapacitated" ? Math.PI / 2 : 0;
    this.torso.rotation.set(0, 0, 0);
    this.head.rotation.set(0, 0, 0);
    this.torso.position.y = 1.05;
    this.arms.forEach((arm, i) => {
      arm.rotation.set(
        (i === 0 ? 1 : -1) * swing * 0.8,
        0,
        i === 0 ? -0.07 : 0.07,
      );
    });
    this.legs.forEach((leg, i) => {
      leg.rotation.x = (i === 0 ? -1 : 1) * swing;
    });
    if (!reduced)
      this.torso.position.y += moving
        ? Math.abs(Math.sin(this.animation.phase)) * 0.025
        : Math.sin(time * 2) * 0.006;
    if (clip === "driving") {
      this.arms.forEach((a) => (a.rotation.x = -1.15));
      this.legs.forEach((a) => (a.rotation.x = -1.35));
    }
    if (clip === "aim_idle" || clip === "aim_walk" || clip === "fire") {
      this.arms.forEach((a) => (a.rotation.x = -1.35));
      if (clip === "fire" && !reduced) this.torso.rotation.x = -0.09;
    }
    if (clip === "reload") {
      this.arms[0].rotation.x = -1.2;
      this.arms[1].rotation.x = -0.8;
      this.arms[1].rotation.z = -0.5;
    }
    if (clip === "jump_start" || clip === "fall") {
      this.arms.forEach((a, i) => (a.rotation.z = i === 0 ? -0.35 : 0.35));
      this.legs.forEach((a) => (a.rotation.x = -0.25));
    }
    if (clip === "land" && !reduced) {
      this.torso.position.y -= 0.1;
      this.legs.forEach((a) => (a.rotation.x = 0.18));
    }
  }
}
/** Batch all remote rig/car parts by shared geometry/material to keep draw calls bounded. */
export class DynamicBatch {
  private batches = new Map<string, THREE.InstancedMesh>();
  constructor(private readonly scene: THREE.Scene) {}
  begin(): void {
    for (const b of this.batches.values()) b.count = 0;
  }
  add(mesh: THREE.Mesh): void {
    if (Array.isArray(mesh.material)) return;
    const key = `${mesh.geometry.uuid}:${mesh.material.uuid}`;
    let batch = this.batches.get(key);
    if (!batch) {
      batch = new THREE.InstancedMesh(mesh.geometry, mesh.material, 512);
      batch.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
      batch.count = 0;
      batch.frustumCulled = false;
      this.batches.set(key, batch);
      this.scene.add(batch);
    }
    if (batch.count < 512) batch.setMatrixAt(batch.count++, mesh.matrixWorld);
  }
  end(): void {
    for (const b of this.batches.values()) {
      b.visible = b.count > 0;
      b.instanceMatrix.needsUpdate = true;
    }
  }
  get matrixBytes(): number {
    return [...this.batches.values()].reduce(
      (n, b) => n + b.instanceMatrix.array.byteLength,
      0,
    );
  }
  dispose(): void {
    for (const b of this.batches.values()) {
      this.scene.remove(b);
      b.dispose();
    }
    this.batches.clear();
  }
}
export function carLean(
  speed: number,
  steering: number,
  reduced: boolean,
): number {
  return reduced
    ? 0
    : Math.max(
        -Math.PI / 45,
        Math.min(Math.PI / 45, -speed * steering * 0.006),
      );
}
