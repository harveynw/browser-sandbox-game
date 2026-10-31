import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { loadPackage } from "@port-meridian/content";
import type { Primitive } from "@port-meridian/content";
import { buildingAppearance } from "./architecture";
import { facadeDetails } from "./facades";

function required<T>(value: T | null | undefined): T {
  if (value === null || value === undefined)
    throw new Error("Missing architecture preview element or recipe");
  return value;
}

/** Review the exact facade generator used by streamed game chunks. */
export function mountArchitecturePreview(root: HTMLElement): () => void {
  root.innerHTML = `<div class="architecture-workspace">
    <canvas style="width:100%;height:100%;display:block"></canvas>
    <section class="architecture-panel">
      <h1>Neighbourhood buildings</h1>
      <label>Neighbourhood <select aria-label="Neighbourhood"><option value="old_quay">Old Quay</option><option value="ironworks">Ironworks</option></select></label>
      <p><label>Width <input aria-label="Width" type="range" step="0.5"><output id="width-value"></output></label></p>
      <p><label>Depth <input aria-label="Depth" type="range" step="0.5"><output id="depth-value"></output></label></p>
      <p><label>Floors <input aria-label="Floors" type="range" step="1"><output id="floors-value"></output></label></p>
      <button type="button">Another building</button>
      <p>Drag to orbit · scroll to zoom</p><a href="/">Back to the game</a>
      <p role="status">Loading buildings…</p>
    </section></div>`;
  const canvas = required(root.querySelector("canvas"));
  const graphics = new THREE.WebGLRenderer({ canvas, antialias: true });
  graphics.setPixelRatio(Math.min(devicePixelRatio, 1.5));
  const scene = new THREE.Scene();
  scene.background = new THREE.Color("#8aabb8");
  const camera = new THREE.PerspectiveCamera(45, 1, 0.1, 200);
  camera.position.set(28, 23, 32);
  const controls = new OrbitControls(camera, canvas);
  controls.target.set(0, 5, 0);
  controls.maxPolarAngle = Math.PI * 0.48;
  controls.minDistance = 12;
  controls.maxDistance = 80;
  controls.update();
  scene.add(new THREE.HemisphereLight(0xdcecff, 0x6c6550, 2.2));
  const sun = new THREE.DirectionalLight(0xffedcf, 3);
  sun.position.set(20, 35, 15);
  scene.add(sun);
  const cube = new THREE.BoxGeometry(1, 1, 1);
  const groundMaterial = new THREE.MeshStandardMaterial({ color: "#616d6c" });
  const ground = new THREE.Mesh(cube, groundMaterial);
  ground.position.y = -0.2;
  ground.scale.set(80, 0.4, 80);
  scene.add(ground);
  const grid = new THREE.GridHelper(80, 40, "#879391", "#737f7c");
  grid.position.y = 0.01;
  scene.add(grid);
  let group = new THREE.Group(),
    variant = 8,
    disposed = false;
  function clear() {
    group.traverse((o) => {
      if (o instanceof THREE.InstancedMesh) {
        (o.material as THREE.Material).dispose();
        o.dispose();
      }
    });
    scene.remove(group);
    group = new THREE.Group();
  }
  const select = required(root.querySelector("select"));
  const inputs = [...root.querySelectorAll("input")];
  const status = required(root.querySelector<HTMLElement>('[role="status"]'));
  // Same content package as gameplay; no separately maintained example recipes.
  void loadPackage(new URL("/content/city.json", location.href))
    .then((assets) => {
      if (disposed) return;
      select.replaceChildren(
        ...assets.manifest.districts.map((d) => {
          const option = document.createElement("option");
          option.value = d.id;
          option.textContent = d.name;
          return option;
        }),
      );
      select.value = "old_quay";
      const update = (reset = false) => {
        const district = required(
          assets.manifest.districts.find((d) => d.id === select.value),
        );
        const recipe = district.architecture;
        const ranges = [recipe.width, recipe.depth, recipe.floors];
        inputs.forEach((input, i) => {
          const [min, max] = required(ranges[i]);
          input.min = String(min);
          input.max = String(max);
          if (reset) input.value = String(max);
          required(root.querySelectorAll("output")[i]).textContent =
            `${input.value}${i < 2 ? "m" : ""}`;
        });
        const [width, depth, floors] = inputs.map((input) =>
          Number(input.value),
        ) as [number, number, number];
        const height = floors * recipe.floor_height;
        if (reset) {
          const distance = Math.max(width, depth, height) * 1.6;
          controls.target.set(0, height * 0.45, 0);
          camera.position.set(
            distance,
            height * 0.5 + distance * 0.65,
            distance,
          );
          controls.update();
        }
        // Center the example inside its authored district to exercise normal lookup.
        const x = (district.rect.min[0] + district.rect.max[0]) / 2;
        const z = (district.rect.min[1] + district.rect.max[1]) / 2;
        const p: Primitive = {
          id: `solid_${variant}`,
          role: "building",
          center: [x, height / 2, z],
          half_extents: [width / 2, height / 2, depth / 2],
          color: district.color,
          collider_id: null,
        };
        const parts = [
          buildingAppearance(p, [district]),
          ...facadeDetails([p], [district], {
            ...assets.manifest.roads,
            x: [x - 40, x + 40],
            z: [z - 40, z + 15],
          }),
        ];
        clear();
        const buckets = new Map<string, Primitive[]>();
        for (const part of parts) {
          const list = buckets.get(part.color) ?? [];
          list.push(part);
          buckets.set(part.color, list);
        }
        const transform = new THREE.Object3D();
        for (const [color, primitives] of buckets) {
          const mesh = new THREE.InstancedMesh(
            cube,
            new THREE.MeshStandardMaterial({ color, roughness: 0.95 }),
            primitives.length,
          );
          primitives.forEach((part, i) => {
            transform.position.set(
              part.center[0] - x,
              part.center[1],
              part.center[2] - z,
            );
            transform.scale.set(
              ...(part.half_extents.map((n) => n * 2) as [
                number,
                number,
                number,
              ]),
            );
            transform.updateMatrix();
            mesh.setMatrixAt(i, transform.matrix);
          });
          mesh.computeBoundingSphere();
          group.add(mesh);
        }
        scene.add(group);
        status.textContent = `${district.name} · ${height.toFixed(1)}m tall`;
      };
      select.onchange = () => update(true);
      inputs.forEach((input) => {
        input.oninput = () => update();
      });
      required(root.querySelector("button")).onclick = () => {
        variant++;
        update();
      };
      update(true);
    })
    .catch((error: unknown) => {
      if (!disposed) status.textContent = String(error);
    });
  const resize = () => {
    graphics.setSize(canvas.clientWidth, canvas.clientHeight, false);
    camera.aspect = canvas.clientWidth / Math.max(1, canvas.clientHeight);
    camera.updateProjectionMatrix();
  };
  window.addEventListener("resize", resize);
  resize();
  graphics.setAnimationLoop(() => graphics.render(scene, camera));
  return () => {
    disposed = true;
    graphics.setAnimationLoop(null);
    window.removeEventListener("resize", resize);
    controls.dispose();
    clear();
    cube.dispose();
    groundMaterial.dispose();
    grid.geometry.dispose();
    const materials = Array.isArray(grid.material)
      ? grid.material
      : [grid.material];
    materials.forEach((m) => m.dispose());
    graphics.dispose();
    root.replaceChildren();
  };
}
