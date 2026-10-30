import * as THREE from "three";
import { mergeGeometries } from "three/addons/utils/BufferGeometryUtils.js";
import type { VehicleTemplate } from "@port-meridian/content";

/** Roll around local X from rendered travel, including locally predicted cars.
 * Local forward is -Z; sideways slides and teleport corrections do not spin tires.
 */
export function wheelRollDelta(
  previous: THREE.Vector3,
  current: THREE.Vector3,
  yaw: number,
): number {
  if (previous.distanceToSquared(current) >= 25) return 0;
  const longitudinal =
    (current.x - previous.x) * Math.sin(yaw) +
    (current.z - previous.z) * Math.cos(yaw);
  return longitudinal / 0.34;
}

/** Shared, original vehicle art. Static panels are merged once per model/material. */
export class VehicleModels {
  private readonly models = new Map<string, THREE.Group>();
  private readonly materials = new Map<string, THREE.MeshStandardMaterial>();
  private readonly geometries = new Set<THREE.BufferGeometry>();
  private readonly box = new THREE.BoxGeometry();
  private readonly shell: THREE.BufferGeometry;
  private readonly tire = new THREE.CylinderGeometry(0.34, 0.34, 0.24, 20);
  private readonly rim = new THREE.CylinderGeometry(0.23, 0.23, 0.252, 16);
  private readonly hub = new THREE.CylinderGeometry(0.065, 0.065, 0.27, 12);
  private readonly arch = new THREE.TorusGeometry(0.38, 0.045, 6, 16, Math.PI);

  constructor() {
    const shape = new THREE.Shape();
    shape.moveTo(-0.44, -0.44);
    shape.lineTo(0.44, -0.44);
    shape.lineTo(0.44, 0.44);
    shape.lineTo(-0.44, 0.44);
    shape.closePath();
    this.shell = new THREE.ExtrudeGeometry(shape, {
      depth: 0.88,
      bevelEnabled: true,
      bevelSegments: 1,
      steps: 1,
      bevelSize: 0.06,
      bevelThickness: 0.06,
    });
    this.shell.center();
    for (const geometry of [
      this.box,
      this.shell,
      this.tire,
      this.rim,
      this.hub,
      this.arch,
    ])
      this.geometries.add(geometry);
  }

  private material(
    color: string,
    finish = "paint",
  ): THREE.MeshStandardMaterial {
    const key = `${color}:${finish}`;
    let material = this.materials.get(key);
    if (!material) {
      material = new THREE.MeshStandardMaterial({
        color,
        roughness:
          finish === "rubber" ? 0.95 : finish === "glass" ? 0.16 : 0.32,
        metalness: finish === "metal" ? 0.75 : finish === "paint" ? 0.3 : 0.05,
        ...(finish === "lamp"
          ? { emissive: color, emissiveIntensity: 1.3 }
          : {}),
      });
      this.materials.set(key, material);
    }
    return material;
  }

  private merge(group: THREE.Group): THREE.Group {
    const parts = new Map<THREE.Material, THREE.BufferGeometry[]>();
    group.updateMatrixWorld(true);
    group.traverse((object) => {
      if (!(object instanceof THREE.Mesh) || Array.isArray(object.material))
        return;
      const geometry = object.geometry.clone().applyMatrix4(object.matrixWorld);
      const list = parts.get(object.material) ?? [];
      list.push(geometry.index ? geometry.toNonIndexed() : geometry);
      if (geometry.index) geometry.dispose();
      parts.set(object.material, list);
    });
    const result = new THREE.Group();
    for (const [material, pieces] of parts) {
      const geometry = mergeGeometries(pieces);
      for (const piece of pieces) piece.dispose();
      if (!geometry) throw new Error("Vehicle geometry attributes must match");
      this.geometries.add(geometry);
      result.add(new THREE.Mesh(geometry, material));
    }
    return result;
  }

  create(
    template: VehicleTemplate | undefined,
    color: string,
    police: boolean,
  ) {
    const [w, h, l] = template?.dimensions ?? [1.8, 1.4, 4.2];
    const wheelbase = template?.wheelbase ?? 2.6;
    const key = JSON.stringify([w, h, l, wheelbase, color, police]);
    let prototype = this.models.get(key);
    if (!prototype) {
      const body = new THREE.Group();
      const van = template?.id === "van";
      const sport = template?.id === "sport";
      const paint = this.material(police ? "#e4e9e6" : color);
      const trim = this.material("#172127", "rubber");
      const glass = this.material("#284657", "glass");
      const metal = this.material("#a4b4bc", "metal");
      const headlight = this.material("#fff0c6", "lamp");
      const taillight = this.material("#e43432", "lamp");
      const part = (
        geometry: THREE.BufferGeometry,
        material: THREE.Material,
        position: [number, number, number],
        scale: [number, number, number],
        rotation = 0,
      ) => {
        const mesh = new THREE.Mesh(geometry, material);
        mesh.position.set(...position);
        mesh.scale.set(...scale);
        mesh.rotation.x = rotation;
        body.add(mesh);
        return mesh;
      };
      const box = (
        material: THREE.Material,
        position: [number, number, number],
        scale: [number, number, number],
      ) => part(this.box, material, position, scale);
      const beam = (
        a: THREE.Vector3,
        b: THREE.Vector3,
        thickness: number,
        material = paint,
      ) => {
        const mesh = box(
          material,
          [0, 0, 0],
          [thickness, a.distanceTo(b), thickness],
        );
        mesh.position.copy(a).add(b).multiplyScalar(0.5);
        mesh.quaternion.setFromUnitVectors(
          new THREE.Vector3(0, 1, 0),
          b.clone().sub(a).normalize(),
        );
      };
      const waist = van ? 0.91 : sport ? 0.69 : 0.77;
      part(
        this.shell,
        paint,
        [0, (waist + 0.3) / 2, 0],
        [w * 0.97, waist - 0.3, l * 0.98],
      );
      box(trim, [0, 0.29, 0], [w * 0.8, 0.12, l * 0.86]);
      // Front is local -Z, matching steering and the authored seat offsets.
      const front = -l * (van ? 0.32 : 0.25);
      const rear = l * (van ? 0.43 : sport ? 0.25 : 0.31);
      const roofFront = front + l * (van ? 0.07 : 0.12);
      const roofRear = rear - l * (van ? 0.01 : 0.1);
      const top = h - 0.055;
      const bottomX = w * 0.45;
      const topX = w * (van ? 0.43 : 0.36);
      const corners = [
        [-bottomX, waist, front],
        [bottomX, waist, front],
        [bottomX, waist, rear],
        [-bottomX, waist, rear],
        [-topX, top, roofFront],
        [topX, top, roofFront],
        [topX, top, roofRear],
        [-topX, top, roofRear],
      ];
      const cabin = new THREE.BufferGeometry();
      cabin.setAttribute(
        "position",
        new THREE.Float32BufferAttribute(corners.flat(), 3),
      );
      cabin.setIndex([
        0, 4, 5, 0, 5, 1, 1, 5, 6, 1, 6, 2, 2, 6, 7, 2, 7, 3, 3, 7, 4, 3, 4, 0,
        4, 7, 6, 4, 6, 5, 0, 1, 2, 0, 2, 3,
      ]);
      const flatCabin = cabin.toNonIndexed();
      cabin.dispose();
      flatCabin.computeVertexNormals();
      // Match the shared primitive attributes before merging.
      flatCabin.setAttribute(
        "uv",
        new THREE.Float32BufferAttribute(
          new Float32Array(flatCabin.getAttribute("position").count * 2),
          2,
        ),
      );
      part(flatCabin, glass, [0, 0, 0], [1, 1, 1]);
      part(
        this.shell,
        paint,
        [0, top + 0.025, (roofFront + roofRear) / 2],
        [topX * 2 + 0.07, 0.06, roofRear - roofFront + 0.09],
      );
      for (const side of [-1, 1]) {
        beam(
          new THREE.Vector3(side * bottomX, waist, front),
          new THREE.Vector3(side * topX, top, roofFront),
          0.075,
        );
        beam(
          new THREE.Vector3(side * bottomX, waist, rear),
          new THREE.Vector3(side * topX, top, roofRear),
          0.09,
        );
        beam(
          new THREE.Vector3(side * bottomX, waist, 0.12),
          new THREE.Vector3(side * topX, top, 0.12),
          van ? 0.12 : 0.065,
          trim,
        );
        box(
          metal,
          [side * bottomX, waist + 0.01, (front + rear) / 2],
          [0.035, 0.025, rear - front],
        );
        box(trim, [side * w * 0.49, 0.36, 0], [0.035, 0.095, l * 0.83]);
        // Door shut line and a recessed handle, on each side.
        box(
          trim,
          [side * w * 0.487, (waist + 0.4) / 2, 0.18],
          [0.015, waist - 0.4, 0.016],
        );
        box(
          metal,
          [side * w * 0.495, waist - 0.1, -0.02],
          [0.025, 0.035, 0.18],
        );
        box(
          trim,
          [side * w * 0.49, waist + 0.13, front + 0.19],
          [0.18, 0.04, 0.06],
        );
        part(
          this.shell,
          paint,
          [side * w * 0.55, waist + 0.16, front + 0.2],
          [0.18, 0.13, 0.25],
        );
        box(
          metal,
          [side * w * 0.55, waist + 0.16, front + 0.33],
          [0.13, 0.085, 0.012],
        );
        for (const z of [-1, 1]) {
          const arch = part(
            this.arch,
            paint,
            [side * (w / 2 - 0.025), 0.35, (z * wheelbase) / 2],
            [1, 1, 1],
          );
          arch.rotation.y = Math.PI / 2;
        }
      }
      for (const end of [-1, 1]) {
        part(
          this.shell,
          police ? trim : paint,
          [0, 0.44, end * l * 0.49],
          [w * 0.96, 0.18, 0.14],
        );
        box(trim, [0, 0.56, end * l * 0.498], [w * 0.4, 0.2, 0.025]);
        box(metal, [0, 0.45, end * l * 0.509], [0.3, 0.085, 0.015]);
        for (const side of [-1, 1]) {
          box(
            trim,
            [side * w * 0.33, waist - 0.09, end * l * 0.49],
            [w * 0.24, 0.17, 0.065],
          );
          box(
            end === -1 ? headlight : taillight,
            [side * w * 0.33, waist - 0.085, end * l * 0.503],
            [w * 0.2, sport ? 0.055 : 0.105, 0.035],
          );
        }
      }
      for (const y of [0.51, 0.56, 0.61])
        box(metal, [0, y, -l * 0.513], [w * 0.34, 0.013, 0.02]);
      box(trim, [w * 0.28, 0.3, l * 0.49], [0.17, 0.09, 0.18]);
      if (van) {
        // Solid cargo compartment, sliding-door track and split rear doors.
        part(
          this.shell,
          paint,
          [0, (waist + top) / 2, l * 0.23],
          [w * 0.9, top - waist, l * 0.4],
        );
        for (const side of [-1, 1]) {
          box(
            trim,
            [side * w * 0.453, waist + 0.22, l * 0.23],
            [0.02, 0.025, l * 0.31],
          );
          box(
            metal,
            [side * w * 0.455, waist + 0.42, l * 0.1],
            [0.03, 0.045, 0.2],
          );
        }
        box(
          trim,
          [0, (waist + top) / 2, rear + 0.015],
          [0.025, top - waist, 0.02],
        );
      }
      if (sport) {
        for (const side of [-1, 1]) {
          box(
            trim,
            [side * w * 0.3, waist + 0.015, -l * 0.36],
            [0.17, 0.025, 0.36],
          );
          box(
            trim,
            [side * w * 0.31, waist + 0.14, l * 0.39],
            [0.045, 0.22, 0.06],
          );
        }
        part(
          this.shell,
          paint,
          [0, waist + 0.25, l * 0.39],
          [w * 0.87, 0.06, 0.23],
        );
        box(trim, [0, 0.32, -l * 0.49], [w, 0.04, 0.2]);
      }
      if (police) {
        for (const side of [-1, 1]) {
          box(
            trim,
            [side * w * 0.49, waist - 0.17, 0],
            [0.025, 0.14, l * 0.61],
          );
          box(
            this.material("#e9c373", "metal"),
            [side * w * 0.505, waist - 0.17, -0.25],
            [0.02, 0.19, 0.19],
          );
        }
        box(trim, [0, h + 0.02, 0], [1.05, 0.06, 0.25]);
        for (const side of [-1, 1])
          box(
            this.material(side < 0 ? "#ef3939" : "#399aff", "lamp"),
            [side * 0.3, h + 0.1, 0],
            [0.43, 0.12, 0.22],
          );
      }
      prototype = this.merge(body);
      flatCabin.dispose();
      this.models.set(key, prototype);
    }
    const body = prototype.clone();
    const wheels: THREE.Group[] = [];
    let wheelModel = this.models.get("wheel");
    if (!wheelModel) {
      const roller = new THREE.Group();
      for (const [geometry, material] of [
        [this.tire, this.material("#171d22", "rubber")],
        [this.rim, this.material("#34434c", "metal")],
        [this.hub, this.material("#bcc9d0", "metal")],
      ] as const) {
        const mesh = new THREE.Mesh(geometry, material);
        mesh.rotation.z = Math.PI / 2;
        roller.add(mesh);
      }
      for (let i = 0; i < 5; i++) {
        const angle = (i * Math.PI * 2) / 5;
        const spoke = new THREE.Mesh(
          this.box,
          this.material("#bcc9d0", "metal"),
        );
        spoke.scale.set(0.26, 0.19, 0.045);
        spoke.position.set(0, Math.cos(angle) * 0.12, Math.sin(angle) * 0.12);
        spoke.rotation.x = angle;
        roller.add(spoke);
      }
      wheelModel = this.merge(roller);
      this.models.set("wheel", wheelModel);
    }
    for (const side of [-1, 1])
      for (const end of [-1, 1]) {
        const pivot = new THREE.Group();
        pivot.position.set(side * (w / 2 - 0.04), 0.35, (end * wheelbase) / 2);
        pivot.add(wheelModel.clone());
        wheels.push(pivot);
      }
    return { body, wheels };
  }

  dispose(): void {
    for (const geometry of this.geometries) geometry.dispose();
    for (const material of this.materials.values()) material.dispose();
    this.models.clear();
  }
}

/** Wheels are authored front/rear on each side; roll stays on the inner group. */
export function steerWheels(
  wheels: readonly THREE.Group[],
  steering: number,
): void {
  wheels.forEach((wheel, index) => {
    wheel.rotation.y = index % 2 === 0 ? steering : 0;
  });
}
