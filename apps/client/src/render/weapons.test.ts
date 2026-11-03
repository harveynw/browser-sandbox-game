import { expect, it } from "vitest";
import * as THREE from "three";
import { WeaponModels, deadlineProgress, attachments } from "./weapons";
import { WeaponEffects } from "./weapon-effects";
import { CharacterRig } from "./character";
import type { CharacterMotor, Shot, WeaponId } from "@port-meridian/protocol";
const motor: CharacterMotor = {
  position: [0, 0, 0],
  velocity: [0, 0, 0],
  yaw: 0,
  grounded: true,
  jump_cooldown_ticks: 0,
};
it("uses absolute deadlines: late samples seek forward and cancellation removes the pose", () => {
  expect(deadlineProgress(160, 130, 60)).toBe(0.5);
  expect(deadlineProgress(160, 145, 60)).toBe(0.75);
  expect(deadlineProgress(160, 160, 60)).toBeNull();
  expect(deadlineProgress(null, 145, 60)).toBeNull();
});
it("shares GPU assets while keeping magazine and attachment transforms independent", () => {
  const models = new WeaponModels();
  for (const id of ["pistol", "rifle", "machine_gun"] as WeaponId[]) {
    const a = models.create(id),
      b = models.create(id);
    a.magazine.position.y = -0.2;
    expect(b.magazine.position.y).toBe(0);
    expect(a.muzzle.position.toArray()).toEqual(attachments[id].muzzle);
    a.root.traverse((o) => {
      if (o instanceof THREE.Mesh) expect(o.geometry).toBe(models.geometry);
    });
    const box = new THREE.Box3().setFromObject(b.root);
    expect(box.getSize(new THREE.Vector3()).z).toBeGreaterThan(
      id === "pistol" ? 0.25 : 0.9,
    );
  }
  models.dispose();
});
it("recovers interrupted reload/equip, hides stowed weapons, and preserves authoritative motor", () => {
  const models = new WeaponModels(),
    cube = new THREE.BoxGeometry(),
    material = new THREE.MeshStandardMaterial();
  const rig = new CharacterRig(cube, () => material, 0, models),
    muzzle = new THREE.Vector3();
  for (const weapon of ["pistol", "rifle", "machine_gun"] as WeaponId[]) {
    rig.pose(
      { motor, weapon, aiming: true, reloadProgress: 0.5, equipProgress: 0.3 },
      0.016,
      1,
      true,
    );
    expect(rig.muzzle(muzzle)).toBe(true);
    rig.pose({ motor, weapon, aiming: true }, 0.016, 2, true);
    const restored = muzzle.clone();
    rig.muzzle(restored);
    expect(restored.distanceTo(muzzle)).toBeGreaterThan(0.1);
    rig.pose({ motor, weapon, driving: true }, 0.016, 3, true);
    expect(rig.muzzle(muzzle)).toBe(false);
    rig.pose({ motor, weapon, incapacitated: true }, 0.016, 4, true);
    expect(rig.muzzle(muzzle)).toBe(false);
  }
  expect(motor.position).toEqual([0, 0, 0]);
  models.dispose();
  cube.dispose();
  material.dispose();
});
it("bounds saturated cosmetic pools, expires them, and omits casings on low quality", () => {
  const scene = new THREE.Scene(),
    effects = new WeaponEffects(scene),
    muzzle = new THREE.Vector3(0, 1.5, 0);
  const shot: Shot = {
    weapon_id: "machine_gun",
    shot_sequence: 1,
    event_id: 1,
    server_tick: 1,
    shooter_id: 1,
    origin: [0, 1.45, 0],
    end: [0, 1.45, -10],
    hit_entity_id: null,
  };
  const objects = scene.children.length;
  for (let i = 0; i < 1000; i++)
    effects.shot(shot, muzzle, 100, false, false, "metal");
  effects.update(110, 0.01, false);
  expect(effects.counts).toEqual({ flashes: 32, casings: 64, particles: 96 });
  expect(scene.children.length).toBe(objects);
  effects.update(1000, 0.01, false);
  expect(effects.counts).toEqual({ flashes: 0, casings: 0, particles: 0 });
  effects.shot(shot, muzzle, 1100, true, true, "ground");
  effects.update(1101, 0.001, true);
  expect(effects.counts).toEqual({ flashes: 1, casings: 0, particles: 1 });
  effects.dispose();
  expect(scene.children).toHaveLength(0);
});
