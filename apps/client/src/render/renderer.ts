import * as THREE from "three";
import {
  findRoute,
  ChunkStream,
  type VisualChunk,
  type VehicleTemplate,
  type LoadedPackage,
  type Primitive,
} from "@port-meridian/content";
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
  private readonly palette = new Map<string, THREE.MeshStandardMaterial>();
  private readonly chunks = new Map<string, THREE.Group>();
  private readonly fallback = new Map<string, THREE.Group>();
  private readonly stream: ChunkStream;
  private readonly minimap = document.createElement("canvas");
  private readonly missionMarker = new THREE.Group();
  private missionTarget: readonly number[] | null | undefined;
  private readonly courierRoute = new THREE.Group();
  private readonly carMaterials = new Map<string, THREE.MeshStandardMaterial>();
  private readonly remoteActors = new Map<number, THREE.Group>();
  private readonly remoteMaterial = new THREE.MeshStandardMaterial({
    color: "#66d8c3",
    roughness: 0.8,
  });
  private readonly pedestrianMaterial = new THREE.MeshStandardMaterial({
    color: "#c5a5dc",
    roughness: 0.8,
  });
  private readonly staleMaterial = new THREE.MeshStandardMaterial({
    color: "#9ca9aa",
    roughness: 0.8,
  });
  private capsule: THREE.CapsuleGeometry;
  private faceMaterial: THREE.MeshStandardMaterial;
  private cube: THREE.BoxGeometry;
  setMissionTarget(position: readonly number[] | null | undefined): void {
    this.missionTarget = position;
    this.missionMarker.visible = !!position;
    this.courierRoute.visible = !!position;
    if (position)
      this.missionMarker.position.set(position[0] ?? 0, 0, position[2] ?? 0);
  }
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
    const outside = assets.backdrop.primitives.filter(
      (p) => p.id === "ocean" || p.id === "heights_distant_hill",
    );
    const outsideGroup = this.batch(outside);
    this.fallback.set("outside", outsideGroup);
    this.scene.add(outsideGroup);
    // Fixed coarse silhouettes remain visible wherever detailed art is absent.
    for (const chunk of assets.manifest.chunks) {
      const primitives = assets.backdrop.primitives
        .filter((p) => !outside.includes(p))
        .flatMap((p) => {
          const lo = p.center.map((v, a) =>
            Math.max(v - (p.half_extents[a] ?? 0), chunk.bounds.min[a] ?? 0),
          );
          const hi = p.center.map((v, a) =>
            Math.min(v + (p.half_extents[a] ?? 0), chunk.bounds.max[a] ?? 0),
          );
          if (!hi.every((v, a) => v > (lo[a] ?? 0))) return [];
          return [
            {
              ...p,
              color: p.role === "building" ? "#718184" : p.color,
              center: lo.map((v, a) => (v + (hi[a] ?? 0)) / 2) as [
                number,
                number,
                number,
              ],
              half_extents: lo.map((v, a) => ((hi[a] ?? 0) - v) / 2) as [
                number,
                number,
                number,
              ],
            },
          ];
        });
      const group = this.batch(primitives);
      this.fallback.set(chunk.id, group);
      this.scene.add(group);
    }
    for (const chunk of assets.chunks) this.showChunk(chunk);
    this.stream = new ChunkStream(
      assets.manifest.chunks,
      assets.chunks.map((c) => c.id),
      assets.loadChunk,
      (chunk) => this.showChunk(chunk),
      (id) => this.hideChunk(id),
    );
    this.bakeMinimap();
    const ringGeometry = new THREE.RingGeometry(0.85, 1, 48);
    const terminalMaterial = new THREE.MeshBasicMaterial({
      color: "#56e5e1",
      side: THREE.DoubleSide,
      depthWrite: false,
    });
    const targetMaterial = new THREE.MeshBasicMaterial({
      color: "#ffcf4a",
      side: THREE.DoubleSide,
      depthWrite: false,
    });
    this.geometry.add(ringGeometry);
    this.materials.add(terminalMaterial);
    this.materials.add(targetMaterial);
    for (const zone of assets.manifest.zones.filter(
      (z) => z.kind === "terminal",
    )) {
      const ring = new THREE.Mesh(ringGeometry, terminalMaterial);
      ring.rotation.x = -Math.PI / 2;
      ring.scale.setScalar(zone.radius);
      ring.position.set(zone.center[0], 0.06, zone.center[2]);
      this.scene.add(ring);
      const pole = new THREE.Mesh(cube, terminalMaterial);
      pole.position.set(zone.center[0], 1.5, zone.center[2]);
      pole.scale.set(0.3, 3, 0.3);
      this.scene.add(pole);
    }
    const targetRing = new THREE.Mesh(ringGeometry, targetMaterial);
    targetRing.rotation.x = -Math.PI / 2;
    targetRing.scale.setScalar(5);
    targetRing.position.y = 0.06;
    const targetPole = new THREE.Mesh(cube, targetMaterial);
    targetPole.scale.set(0.4, 12, 0.4);
    targetPole.position.y = 6;
    this.missionMarker.add(targetRing, targetPole);
    this.missionMarker.visible = false;
    this.scene.add(this.missionMarker);
    const route = findRoute(
      assets.manifest.road_graph,
      "terminal_access",
      "market_access",
    );
    for (let i = 1; i < route.length; i++) {
      const a = route[i - 1],
        b = route[i];
      if (!a || !b) continue;
      const segment = new THREE.Mesh(cube, targetMaterial);
      segment.position.set((a[0] + b[0]) / 2, 0.07, (a[2] + b[2]) / 2);
      segment.scale.set(0.25, 0.02, Math.hypot(b[0] - a[0], b[2] - a[2]));
      segment.rotation.y = Math.atan2(b[0] - a[0], b[2] - a[2]);
      this.courierRoute.add(segment);
    }
    this.courierRoute.visible = false;
    this.scene.add(this.courierRoute);
    // Car visuals are instantiated from the verified shared content package.
    const capsule = new THREE.CapsuleGeometry(0.32, 1.1, 4, 8);
    this.geometry.add(capsule);
    this.capsule = capsule;
    this.materials.add(this.remoteMaterial);
    this.materials.add(this.pedestrianMaterial);
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
  private batch(primitives: readonly Primitive[]): THREE.Group {
    const buckets = new Map<THREE.MeshStandardMaterial, Primitive[]>();
    for (const p of primitives) {
      const layer =
        p.role === "marking"
          ? 3
          : p.role === "road"
            ? 2
            : p.role === "sidewalk"
              ? 1
              : 0;
      const key = `${p.color}:${layer}`;
      let material = this.palette.get(key);
      if (!material) {
        material = new THREE.MeshStandardMaterial({
          color: p.color,
          roughness: 0.95,
          polygonOffset: layer > 0,
          polygonOffsetFactor: -layer,
          polygonOffsetUnits: -layer,
        });
        this.palette.set(key, material);
        this.materials.add(material);
      }
      const group = buckets.get(material) ?? [];
      group.push(p);
      buckets.set(material, group);
    }
    const group = new THREE.Group();
    const dummy = new THREE.Object3D();
    for (const [material, values] of buckets) {
      const mesh = new THREE.InstancedMesh(this.cube, material, values.length);
      values.forEach((p, i) => {
        dummy.position.set(...p.center);
        dummy.scale.set(
          p.half_extents[0] * 2,
          p.half_extents[1] * 2,
          p.half_extents[2] * 2,
        );
        dummy.updateMatrix();
        mesh.setMatrixAt(i, dummy.matrix);
      });
      mesh.computeBoundingSphere();
      group.add(mesh);
    }
    return group;
  }
  private showChunk(chunk: VisualChunk): void {
    this.hideChunk(chunk.id);
    const group = this.batch(chunk.primitives);
    this.chunks.set(chunk.id, group);
    this.scene.add(group);
    const fallback = this.fallback.get(chunk.id);
    if (fallback) fallback.visible = false;
  }
  private hideChunk(id: string): void {
    const group = this.chunks.get(id);
    if (group) {
      group.traverse((o) => {
        if (o instanceof THREE.InstancedMesh) o.dispose();
      });
      this.scene.remove(group);
      this.chunks.delete(id);
    }
    const fallback = this.fallback.get(id);
    if (fallback) fallback.visible = true;
  }
  private bakeMinimap(): void {
    this.minimap.width = 384;
    this.minimap.height = 384;
    const ctx = this.minimap.getContext("2d");
    if (!ctx) return;
    const bounds = this.assets.manifest.bounds;
    const width = bounds.max[0] - bounds.min[0];
    const point = (x: number, z: number) => [
      ((x - bounds.min[0]) / width) * 384,
      ((z - bounds.min[2]) / width) * 384,
    ];
    for (const p of this.assets.minimap.primitives) {
      const [x, z] = point(
        p.center[0] - p.half_extents[0],
        p.center[2] - p.half_extents[2],
      );
      ctx.fillStyle = p.color;
      ctx.fillRect(
        x ?? 0,
        z ?? 0,
        ((p.half_extents[0] * 2) / width) * 384,
        ((p.half_extents[2] * 2) / width) * 384,
      );
    }
    ctx.font = "11px sans-serif";
    ctx.textAlign = "center";
    for (const d of this.assets.manifest.districts) {
      const [x, z] = point(
        (d.rect.min[0] + d.rect.max[0]) / 2,
        (d.rect.min[1] + d.rect.max[1]) / 2,
      );
      ctx.fillStyle = "#0b2534";
      ctx.fillText(d.name, x ?? 0, z ?? 0);
    }
  }
  drawMinimap(canvas: HTMLCanvasElement, motor: CharacterMotor): void {
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.drawImage(this.minimap, 0, 0, canvas.width, canvas.height);
    const b = this.assets.manifest.bounds;
    const scale = canvas.width / (b.max[0] - b.min[0]);
    const x = (motor.position[0] - b.min[0]) * scale,
      z = (motor.position[2] - b.min[2]) * scale;
    if (this.missionTarget) {
      ctx.fillStyle = "#ffcf4a";
      ctx.beginPath();
      ctx.arc(
        ((this.missionTarget[0] ?? 0) - b.min[0]) * scale,
        ((this.missionTarget[2] ?? 0) - b.min[2]) * scale,
        4,
        0,
        Math.PI * 2,
      );
      ctx.fill();
    }
    ctx.save();
    ctx.translate(x, z);
    ctx.rotate(-motor.yaw);
    ctx.fillStyle = "#ffed67";
    ctx.beginPath();
    ctx.moveTo(0, -6);
    ctx.lineTo(-4, 4);
    ctx.lineTo(4, 4);
    ctx.closePath();
    ctx.fill();
    ctx.restore();
    const district = this.assets.manifest.districts.find(
      (d) =>
        motor.position[0] >= d.rect.min[0] &&
        motor.position[0] < d.rect.max[0] &&
        motor.position[2] >= d.rect.min[1] &&
        motor.position[2] < d.rect.max[1],
    );
    canvas.setAttribute(
      "aria-label",
      `City minimap — ${district?.name ?? "Harbour"}`,
    );
  }
  get streamingMetrics() {
    let matrices = 0;
    for (const group of [...this.chunks.values(), ...this.fallback.values()])
      group.traverse((o) => {
        if (o instanceof THREE.InstancedMesh) matrices += o.count * 64;
      });
    return {
      ...this.stream.metrics,
      matrixBytes: matrices,
      drawCalls: this.graphics.info.render.calls,
      geometries: this.graphics.info.memory.geometries,
      triangles: this.graphics.info.render.triangles,
    };
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
  pose(
    motor: CharacterMotor,
    driving = false,
    template?: VehicleTemplate,
  ): void {
    this.stream.update(motor.position);
    this.actor.position.set(...motor.position);
    this.actor.rotation.y = motor.yaw;
    this.actor.scale.setScalar(driving ? 0.55 : 1);
    if (driving) {
      const offset = template?.seat_offset ?? [0, 0.8, -0.25];
      this.actor.position.y += offset[1] ?? 0.8;
      this.actor.position.x +=
        Math.cos(motor.yaw) * (offset[0] ?? 0) +
        Math.sin(motor.yaw) * (offset[2] ?? -0.25);
      this.actor.position.z +=
        -Math.sin(motor.yaw) * (offset[0] ?? 0) +
        Math.cos(motor.yaw) * (offset[2] ?? -0.25);
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
        body.material = actor.stale
          ? this.staleMaterial
          : actor.descriptor.entity_kind === "pedestrian"
            ? this.pedestrianMaterial
            : this.remoteMaterial;
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
    this.stream.dispose();
    for (const group of this.fallback.values())
      group.traverse((o) => {
        if (o instanceof THREE.InstancedMesh) o.dispose();
      });
    this.fallback.clear();
    this.palette.clear();
    for (const g of this.geometry) g.dispose();
    for (const m of this.materials) m.dispose();
    this.remoteActors.clear();
    this.scene.clear();
    this.graphics.dispose();
  }
}
