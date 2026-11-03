// Offline rendering fixture; no networking, inventory grants or production route.
import * as THREE from "three";
import {
  CharacterRig,
  DynamicBatch,
} from "../apps/client/src/render/character";
import { WeaponModels } from "../apps/client/src/render/weapons";
import { WeaponEffects } from "../apps/client/src/render/weapon-effects";
const scene = new THREE.Scene();
scene.background = new THREE.Color("#18232c");
const renderer = new THREE.WebGLRenderer({ antialias: true });
renderer.setSize(innerWidth, innerHeight - 26);
renderer.setPixelRatio(1);
document.body.append(renderer.domElement);
const camera = new THREE.PerspectiveCamera(
  50,
  innerWidth / (innerHeight - 26),
  0.05,
  150,
);
scene.add(new THREE.HemisphereLight(0xe8f4ff, 0x79634b, 3));
const sun = new THREE.DirectionalLight(0xffffff, 3);
sun.position.set(3, 8, 4);
scene.add(sun);
const models = new WeaponModels(),
  effects = new WeaponEffects(scene),
  batch = new DynamicBatch(scene),
  cube = new THREE.BoxGeometry();
const materials = new Map<string, THREE.MeshStandardMaterial>();
const material = (c: string) => {
  let m = materials.get(c);
  if (!m) {
    m = new THREE.MeshStandardMaterial({ color: c });
    materials.set(c, m);
  }
  return m;
};
const ids = ["pistol", "rifle", "machine_gun"] as const;
const rigs = Array.from({ length: 16 }, (_, i) => {
  const rig = new CharacterRig(cube, material, i % 4, models);
  rig.root.position.x = ((i % 4) - 1.5) * 1.6;
  rig.root.position.z = Math.floor(i / 4) * 2;
  return rig;
});
const ground = new THREE.Mesh(
  new THREE.PlaneGeometry(60, 60),
  material("#626c68"),
);
ground.rotation.x = -Math.PI / 2;
ground.position.y = -0.02;
scene.add(ground);
let view = 0,
  low = false,
  reduced = false,
  pose = 0;
document.querySelector("#view").addEventListener("click", () => {
  view = (view + 1) % 5;
  document.querySelector("#view").textContent =
    `View: ${["lineup", "load", "pistol shoulder", "rifle shoulder", "machine gun shoulder"][view]}`;
  samples.length = 0;
  intervals.length = 0;
  emitted = 0;
  measuredAt = performance.now();
  maxCalls = 0;
});
document.querySelector("#quality").addEventListener("click", () => {
  low = !low;
  document.querySelector("#quality").textContent =
    `Quality: ${low ? "low" : "high"}`;
  samples.length = 0;
  intervals.length = 0;
  emitted = 0;
  measuredAt = performance.now();
  maxCalls = 0;
});
document.querySelector("#motion").addEventListener("click", () => {
  reduced = !reduced;
  document.querySelector("#motion").textContent =
    `Motion: ${reduced ? "reduced" : "full"}`;
});
document.querySelector("#pose").addEventListener("click", () => {
  pose = (pose + 1) % 5;
  document.querySelector("#pose").textContent =
    `Pose: ${["aim", "carry", "reload", "equip", "driving"][pose]}`;
});
const samples: number[] = [],
  intervals: number[] = [];
const muzzle = new THREE.Vector3();
let last = performance.now(),
  shotAt = last,
  reportAt = last,
  sequence = 0,
  emitted = 0,
  measuredAt = last,
  maxCalls = 0;
const motor = {
  position: [0, 0, 0] as [number, number, number],
  velocity: [0, 0, 0] as [number, number, number],
  yaw: 0,
  grounded: true,
  jump_cooldown_ticks: 0,
};
function frame(now: number) {
  const start = performance.now(),
    dt = Math.min(0.05, (now - last) / 1000);
  intervals.push(now - last);
  last = now;
  const count = view === 1 ? 16 : view === 0 ? 3 : 1;
  batch.begin();
  for (let i = 0; i < count; i++) {
    const rig = rigs[i];
    if (!rig) continue;
    const id =
      view === 1 ? "machine_gun" : (ids[view >= 2 ? view - 2 : i] ?? "pistol");
    rig.root.position.set(
      view >= 2 ? 0 : view === 0 ? (i - 1) * 1.4 : ((i % 4) - 1.5) * 1.6,
      0,
      view === 1 ? Math.floor(i / 4) * 2 : 0,
    );
    rig.pose(
      {
        motor,
        weapon: id,
        aiming: pose === 0,
        reloadProgress: pose === 2 ? (now % 3000) / 3000 : null,
        reloading: pose === 2,
        equipProgress: pose === 3 ? (now % 1500) / 1500 : null,
        driving: pose === 4,
      },
      dt,
      now / 1000,
      reduced,
    );
    rig.root.updateMatrixWorld(true);
    rig.root.traverse((o) => {
      if (!(o instanceof THREE.Mesh)) return;
      for (let p: THREE.Object3D | null = o; p; p = p.parent)
        if (!p.visible) return;
      batch.add(o);
    });
    if (now >= shotAt && pose === 0) {
      rig.fire();
      rig.muzzle(muzzle);
      effects.shot(
        {
          weapon_id: id,
          shot_sequence: sequence,
          event_id: sequence * 16 + i,
          server_tick: sequence * 3,
          shooter_id: i + 1,
          origin: [rig.root.position.x, 1.45, rig.root.position.z],
          end: [rig.root.position.x, 1.45, -15],
          hit_entity_id: null,
        },
        muzzle,
        now,
        reduced,
        low,
        "masonry",
      );
    }
  }
  if (now >= shotAt) {
    shotAt = Math.max(shotAt + 100, now);
    emitted += count;
    sequence++;
  }
  batch.end();
  effects.update(now, dt, low);
  if (view >= 2) {
    camera.position.set(0.5, 1.5, 1.65);
    camera.lookAt(0, 1.45, -80);
  } else {
    camera.position.set(
      view === 1 ? 9 : 3.6,
      view === 1 ? 8 : 2.5,
      view === 1 ? -10 : -6.5,
    );
    camera.lookAt(0, view === 1 ? 1 : 0.9, view === 1 ? 2 : 0);
  }
  renderer.render(scene, camera);
  maxCalls = Math.max(maxCalls, renderer.info.render.calls);
  samples.push(performance.now() - start);
  if (samples.length > 1800) samples.shift();
  if (intervals.length > 1800) intervals.shift();
  if (now - reportAt > 1000) {
    reportAt = now;
    const sorted = [...samples].sort((a, b) => a - b),
      frames = [...intervals].sort((a, b) => a - b);
    document.querySelector("#report").textContent =
      `\n\n${count} actors · ${view === 1 ? "160" : count * 10} requested shots/s · ${((emitted * 1000) / (now - measuredAt)).toFixed(1)} measured\nCPU frame p50 ${sorted[Math.floor(sorted.length * 0.5)]?.toFixed(2)} ms · p95 ${sorted[Math.floor(sorted.length * 0.95)]?.toFixed(2)} ms\nrAF p50 ${frames[Math.floor(frames.length * 0.5)]?.toFixed(2)} ms · p95 ${frames[Math.floor(frames.length * 0.95)]?.toFixed(2)} ms\nDraw calls ${renderer.info.render.calls} (max ${maxCalls}) · triangles ${renderer.info.render.triangles}\nPools ${JSON.stringify(effects.counts)}\n${innerWidth} × ${innerHeight - 26} DPR 1 · ${navigator.userAgent}`;
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
window.addEventListener("pagehide", () => {
  effects.dispose();
  models.dispose();
  batch.dispose();
  cube.dispose();
  ground.geometry.dispose();
  materials.forEach((m) => m.dispose());
  renderer.dispose();
});
