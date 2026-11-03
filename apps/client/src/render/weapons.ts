import * as THREE from "three";
import type { WeaponId, EquipmentView } from "@port-meridian/protocol";
import type { WeaponDefinition } from "@port-meridian/content";

// Metres in grip space: forward is -Z. Factories own GPU resources, instances own transforms.
export const attachments: Record<
  WeaponId,
  { support: [number, number, number]; muzzle: [number, number, number] }
> = {
  pistol: { support: [-0.07, -0.025, -0.04], muzzle: [0, 0.09, -0.26] },
  rifle: { support: [0, 0.04, -0.36], muzzle: [0, 0.09, -0.83] },
  machine_gun: { support: [0, 0.035, -0.32], muzzle: [0, 0.1, -0.68] },
};
export interface WeaponModel {
  root: THREE.Group;
  magazine: THREE.Group;
  slide: THREE.Group;
  muzzle: THREE.Object3D;
}
export function deadlineProgress(
  end: number | null,
  tick: number,
  duration: number,
): number | null {
  return end === null || tick >= end
    ? null
    : THREE.MathUtils.clamp(1 - (end - tick) / duration, 0, 1);
}
export function equipmentProgress(
  equipment: EquipmentView | undefined,
  tick: number,
  catalogue: readonly WeaponDefinition[],
) {
  const definition = catalogue.find((w) => w.id === equipment?.weapon_id);
  return {
    reload:
      definition && equipment
        ? deadlineProgress(
            equipment.reload_end_tick,
            tick,
            definition.reload_ticks,
          )
        : null,
    equip:
      definition && equipment
        ? deadlineProgress(
            equipment.equip_end_tick,
            tick,
            definition.equip_ticks,
          )
        : null,
  };
}
export class WeaponModels {
  readonly geometry = new THREE.BoxGeometry();
  private readonly materials = [
    "#33424a",
    "#151e25",
    "#9ba9ac",
    "#98663e",
    "#647147",
    "#bba060",
  ].map(
    (color) =>
      new THREE.MeshStandardMaterial({
        color,
        roughness: 0.65,
        metalness: 0.35,
      }),
  );
  create(id: WeaponId): WeaponModel {
    const root = new THREE.Group(),
      magazine = new THREE.Group(),
      slide = new THREE.Group(),
      muzzle = new THREE.Object3D();
    root.name = id;
    root.add(magazine, slide, muzzle);
    muzzle.position.set(...attachments[id].muzzle);
    const box = (
      parent: THREE.Group,
      color: number,
      size: [number, number, number],
      at: [number, number, number],
    ) => {
      const mesh = new THREE.Mesh(this.geometry, this.materials[color]);
      mesh.scale.set(...size);
      mesh.position.set(...at);
      parent.add(mesh);
    };
    box(root, 1, [0.065, 0.16, 0.08], [0, -0.035, 0]);
    if (id === "pistol") {
      box(slide, 2, [0.08, 0.085, 0.3], [0, 0.09, -0.11]);
      box(root, 0, [0.075, 0.04, 0.24], [0, 0.035, -0.1]);
      box(magazine, 1, [0.07, 0.035, 0.085], [0, -0.12, 0]);
      box(root, 1, [0.02, 0.025, 0.025], [0, 0.14, -0.23]);
    } else {
      const rifle = id === "rifle";
      box(root, 0, [0.105, 0.13, 0.37], [0, 0.09, -0.1]);
      box(root, rifle ? 3 : 4, [0.1, 0.16, 0.29], [0, 0.03, 0.23]);
      box(root, 1, [0.12, 0.19, 0.04], [0, 0.03, 0.39]);
      box(root, rifle ? 3 : 4, [0.1, 0.1, 0.25], [0, 0.07, -0.36]);
      box(
        root,
        2,
        [rifle ? 0.035 : 0.06, 0.04, rifle ? 0.38 : 0.25],
        [0, 0.09, rifle ? -0.64 : -0.55],
      );
      box(
        magazine,
        rifle ? 1 : 4,
        rifle ? [0.07, 0.22, 0.12] : [0.23, 0.23, 0.2],
        [0, -0.1, -0.16],
      );
      box(slide, 2, [0.035, 0.035, 0.07], [0.075, 0.11, -0.06]);
      box(root, 1, [0.035, 0.05, 0.04], [0, 0.18, -0.26]);
      if (!rifle) {
        box(root, 1, [0.045, 0.05, 0.2], [0, 0.24, -0.13]);
        box(root, 5, [0.08, 0.035, 0.16], [0.1, 0.02, -0.15]);
      }
    }
    return { root, magazine, slide, muzzle };
  }
  dispose(): void {
    this.geometry.dispose();
    for (const m of this.materials) m.dispose();
  }
}
