import * as THREE from "three";
import type { LoadedPackage, Primitive } from "@port-meridian/content";
import type { RenderActor } from "../world/replica";
import type { CharacterMotor } from "@port-meridian/protocol";
export class GameRenderer {
  readonly graphics: THREE.WebGLRenderer;
  readonly scene = new THREE.Scene();
  readonly camera = new THREE.PerspectiveCamera(65, 1, 0.05, 650);
  readonly actor = new THREE.Group();
  private readonly geometry = new Set<THREE.BufferGeometry>();
  private readonly materials = new Set<THREE.Material>();
  private readonly characterMaterial = new THREE.MeshStandardMaterial({
    color: "#e9a359",
    roughness: 0.8,
    transparent: true,
  });
  private low = false;
  private readonly carMaterials = new Map<string, THREE.MeshStandardMaterial>();
  private readonly remoteActors = new Map<number, THREE.Group>();
  private readonly remoteMaterial = new THREE.MeshStandardMaterial({
    color: "#66d8c3",
    roughness: 0.8,
  });
  private readonly staleMaterial = new THREE.MeshStandardMaterial({
    color: "#9ca9aa",
    roughness: 0.8,
  });
  private capsule: THREE.CapsuleGeometry;
  private faceMaterial: THREE.MeshStandardMaterial;
  private cube: THREE.BoxGeometry;
  constructor(
    readonly canvas: HTMLCanvasElement,
    private readonly assets: LoadedPackage,
  ) {
    this.graphics = new THREE.WebGLRenderer({ canvas, antialias: true });
    this.graphics.outputColorSpace = THREE.SRGBColorSpace;
    this.scene.background = new THREE.Color("#b3ced1");
    this.scene.fog = new THREE.Fog("#b3ced1", 130, 450);
    this.scene.add(new THREE.HemisphereLight("#eff8ff", "#6a6351", 2.3));
    const sun = new THREE.DirectionalLight("#ffdfb0", 2.8);
    sun.position.set(-80, 130, 60);
    this.scene.add(sun);
    const cube = new THREE.BoxGeometry(1, 1, 1);
    this.geometry.add(cube);
    this.cube = cube;
    const palette = new Map<string, THREE.MeshStandardMaterial>();
    const box = (p: Primitive) => {
      const layer =
        p.role === "marking"
          ? 3
          : p.role === "road"
            ? 2
            : p.role === "sidewalk"
              ? 1
              : 0;
      const key = `${p.color}:${layer}`;
      let mat = palette.get(key);
      if (!mat) {
        mat = new THREE.MeshStandardMaterial({
          color: p.color,
          roughness: 0.95,
          polygonOffset: layer > 0,
          polygonOffsetFactor: -layer,
          polygonOffsetUnits: -layer,
        });
        palette.set(key, mat);
        this.materials.add(mat);
      }
      const mesh = new THREE.Mesh(cube, mat);
      mesh.position.set(...p.center);
      mesh.scale.set(
        p.half_extents[0] * 2,
        p.half_extents[1] * 2,
        p.half_extents[2] * 2,
      );
      this.scene.add(mesh);
    };
    for (const chunk of assets.chunks) for (const p of chunk.primitives) box(p);
    // Car visuals are instantiated from the verified shared content package.
    const capsule = new THREE.CapsuleGeometry(0.32, 1.1, 4, 8);
    this.geometry.add(capsule);
    this.capsule = capsule;
    this.materials.add(this.remoteMaterial);
    this.materials.add(this.staleMaterial);
    this.materials.add(this.characterMaterial);
    const body = new THREE.Mesh(capsule, this.characterMaterial);
    body.position.y = 0.87;
    this.actor.add(body);
    const faceMaterial = new THREE.MeshStandardMaterial({
      color: "#263e45",
      transparent: true,
    });
    this.materials.add(faceMaterial);
    this.faceMaterial = faceMaterial;
    const face = new THREE.Mesh(cube, faceMaterial);
    face.position.set(0, 1.48, -0.29);
    face.scale.set(0.34, 0.12, 0.12);
    this.actor.add(face);
    this.scene.add(this.actor);
    this.resize();
  }
  quality(low: boolean): void {
    this.low = low;
    this.resize();
  }
  resize(): void {
    const { width, height } = this.canvas.getBoundingClientRect();
    this.graphics.setPixelRatio(Math.min(devicePixelRatio, this.low ? 1 : 1.5));
    this.graphics.setSize(Math.max(1, width), Math.max(1, height), false);
    this.camera.aspect = width / Math.max(1, height);
    this.camera.updateProjectionMatrix();
  }
  pose(motor: CharacterMotor, driving = false): void {
    this.actor.position.set(...motor.position);
    this.actor.rotation.y = motor.yaw;
    this.actor.scale.setScalar(driving ? 0.55 : 1);
    if (driving) {
      this.actor.position.y += 0.8;
      this.actor.position.x -= Math.sin(motor.yaw) * 0.25;
      this.actor.position.z -= Math.cos(motor.yaw) * 0.25;
    }
  }
  remotes(actors: readonly RenderActor[]): void {
    const present = new Set(actors.map((a) => a.id));
    for (const [id, group] of this.remoteActors)
      if (!present.has(id)) {
        this.scene.remove(group);
        this.remoteActors.delete(id);
      }
    for (const actor of actors) {
      let group = this.remoteActors.get(actor.id);
      if (!group) {
        group = new THREE.Group();
        if (actor.descriptor.entity_kind === "vehicle") {
          const template = this.assets.manifest.vehicle_templates.find(
            (t) => t.id === actor.descriptor.template_id,
          );
          const visual = this.assets.vehicleVisuals.find(
            (v) => v.id === template?.visual_asset_id,
          );
          for (const p of visual?.primitives ?? []) {
            let material = this.carMaterials.get(p.color);
            if (!material) {
              material = new THREE.MeshStandardMaterial({
                color: p.color,
                roughness: 0.8,
              });
              this.carMaterials.set(p.color, material);
              this.materials.add(material);
            }
            const mesh = new THREE.Mesh(this.cube, material);
            mesh.position.set(...p.center);
            mesh.scale.set(
              p.half_extents[0] * 2,
              p.half_extents[1] * 2,
              p.half_extents[2] * 2,
            );
            group.add(mesh);
          }
        } else {
          const body = new THREE.Mesh(this.capsule, this.remoteMaterial);
          body.position.y = 0.87;
          group.add(body);
          const face = new THREE.Mesh(this.cube, this.faceMaterial);
          face.position.set(0, 1.48, -0.29);
          face.scale.set(0.34, 0.12, 0.12);
          group.add(face);
        }
        this.remoteActors.set(actor.id, group);
        this.scene.add(group);
      }
      const body = group.children[0];
      if (
        body instanceof THREE.Mesh &&
        actor.descriptor.entity_kind !== "vehicle"
      )
        body.material = actor.stale ? this.staleMaterial : this.remoteMaterial;
      group.scale.setScalar(
        actor.state.kind === "character" && actor.state.data.mode === "driving"
          ? 0.55
          : 1,
      );
      group.position.set(...actor.motor.position);
      group.rotation.y = actor.motor.yaw;
    }
  }
  render(distance: number): void {
    this.actor.visible = distance > 0.15;
    this.actor.traverse((object) => {
      if (object instanceof THREE.Mesh)
        object.material.opacity =
          distance < 0.8 ? Math.max(0.05, distance / 0.8) : 1;
    });
    this.graphics.render(this.scene, this.camera);
  }
  dispose(): void {
    for (const g of this.geometry) g.dispose();
    for (const m of this.materials) m.dispose();
    this.remoteActors.clear();
    this.scene.clear();
    this.graphics.dispose();
  }
}
