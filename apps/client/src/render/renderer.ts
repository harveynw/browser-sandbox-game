import { PlayerNametags } from "./nametags";
import { WeaponModels, equipmentProgress } from "./weapons";
import { WeaponEffects } from "./weapon-effects";
import type { PredictionWorld } from "../simulation/bridge";
import type { EquipmentView, PlayerStatus } from "@port-meridian/protocol";
import { treeShape, treeGeometries } from "./trees";
import * as THREE from "three";
import { CharacterRig, DynamicBatch, carLean } from "./character";
import { buildingAppearance } from "./architecture";
import { facadeDetails } from "./facades";
import { VehicleModels, wheelRollDelta, steerWheels } from "./vehicles";
import {
  courierRoute,
  ChunkStream,
  type VisualChunk,
  type VehicleTemplate,
  type LoadedPackage,
  type Primitive,
} from "@port-meridian/content";
import type { RenderActor } from "../world/replica";
import type { CharacterMotor, WorldEvent } from "@port-meridian/protocol";
export class GameRenderer {
  readonly graphics: THREE.WebGLRenderer;
  readonly scene = new THREE.Scene();
  readonly camera = new THREE.PerspectiveCamera(65, 1, 0.05, 650);
  readonly actor = new THREE.Group();
  private readonly dynamic = new DynamicBatch(this.scene);
  private readonly nametags = new PlayerNametags(this.scene);
  private readonly nametagAnchor = new THREE.Vector3();
  private readonly lods = new Map<number, THREE.Mesh>();
  private readonly rigs = new Map<number, CharacterRig>();
  private localRig: CharacterRig;
  tshirtColour(colour: number): void {
    this.localRig.setShirtColour(colour);
  }
  private readonly textures = new Set<THREE.Texture>();
  private readonly signs: THREE.Sprite[] = [];
  private readonly vehicleModels = new VehicleModels();
  private readonly wheelData = new Map<
    number,
    {
      wheels: THREE.Group[];
      body: THREE.Group;
      previous: THREE.Vector3;
      roll: number;
    }
  >();
  private readonly sun = new THREE.DirectionalLight("#aecbff", 1.6);
  private reduced = false;
  private localFired = false;
  private frameDt = 0.016;
  private readonly geometry = new Set<THREE.BufferGeometry>();
  private readonly materials = new Set<THREE.Material>();
  private readonly characterMaterial = new THREE.MeshStandardMaterial({
    color: "#e9a359",
    roughness: 0.8,
    transparent: true,
  });
  private readonly weaponModels = new WeaponModels();
  private readonly weaponEffects = new WeaponEffects(this.scene);
  private readonly muzzlePosition = new THREE.Vector3();
  private readonly muzzleDirection = new THREE.Vector3();
  private equipment?: EquipmentView;
  private presentationTick = 0;
  equipmentStatus(status: PlayerStatus): void {
    this.equipment = {
      weapon_id: status.equipped_weapon,
      reload_end_tick: status.reload_end_tick,
      equip_end_tick: status.equip_end_tick,
      aiming: false,
      aim_pitch: 0,
    };
  }
  private dustClock = 0;
  private dustCursor = 0;
  private readonly dust: {
    mesh: THREE.Mesh<THREE.BoxGeometry, THREE.MeshBasicMaterial>;
    until: number;
  }[] = [];
  effect(event: WorldEvent, own = false, queries?: PredictionWorld): void {
    if (event.kind !== "shot") {
      this.weaponEffects.impact(
        event.data.position,
        event.data.severity,
        performance.now(),
        this.low || this.reduced,
      );
      return;
    }
    const shot = event.data,
      rig = own ? this.localRig : this.rigs.get(shot.shooter_id);
    if (rig) rig.fire();
    if (own) this.localFired = true;
    this.muzzlePosition.set(...shot.origin);
    if (rig?.muzzle(this.muzzlePosition)) {
      this.muzzleDirection.set(
        this.muzzlePosition.x - shot.origin[0],
        this.muzzlePosition.y - shot.origin[1],
        this.muzzlePosition.z - shot.origin[2],
      );
      const length = this.muzzleDirection.length();
      const hitDistance = Math.hypot(
        shot.end[0] - shot.origin[0],
        shot.end[1] - shot.origin[1],
        shot.end[2] - shot.origin[2],
      );
      const blocked =
        length > 0 &&
        queries?.raycastStatic({
          version: 1,
          origin: shot.origin,
          direction: this.muzzleDirection.normalize().toArray(),
          max_distance: length,
        }).hit;
      if (blocked || hitDistance < length)
        this.muzzlePosition.set(...shot.origin);
    }
    const range =
      this.assets.manifest.weapons.find((w) => w.id === shot.weapon_id)
        ?.range ?? 80;
    const length = Math.hypot(
      shot.end[0] - shot.origin[0],
      shot.end[1] - shot.origin[1],
      shot.end[2] - shot.origin[2],
    );
    const surface =
      shot.hit_entity_id !== null
        ? this.wheelData.has(shot.hit_entity_id)
          ? "metal"
          : "actor"
        : shot.end[1] < 0.05
          ? "ground"
          : length < range - 0.1
            ? "masonry"
            : "none";
    this.weaponEffects.shot(
      shot,
      this.muzzlePosition,
      performance.now(),
      this.reduced,
      this.low,
      surface,
    );
  }
  private low = false;
  private readonly trees = treeGeometries();
  private readonly palette = new Map<string, THREE.MeshStandardMaterial>();
  private readonly chunks = new Map<string, THREE.Group>();
  private readonly fallback = new Map<string, THREE.Group>();
  private readonly stream: ChunkStream;
  private readonly missionMarker = new THREE.Group();
  private readonly courierRoute = new THREE.Group();
  private routeKey = "";
  private routeMaterial: THREE.Material | undefined;
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
  private cube: THREE.BoxGeometry;
  setMissionTarget(
    position: readonly number[] | null | undefined,
    courier = true,
    radius = 5,
  ): void {
    const key = position && courier ? position.join(",") : "";
    if (key !== this.routeKey) {
      this.routeKey = key;
      this.courierRoute.clear();
      if (position && courier && this.routeMaterial) {
        const route = courierRoute(
          this.assets.manifest,
          this.actor.position.toArray(),
          position,
        );
        for (let i = 1; i < route.length; i++) {
          const a = route[i - 1],
            b = route[i];
          if (!a || !b) continue;
          const segment = new THREE.Mesh(this.cube, this.routeMaterial);
          segment.position.set((a[0] + b[0]) / 2, 0.07, (a[2] + b[2]) / 2);
          segment.scale.set(0.25, 0.02, Math.hypot(b[0] - a[0], b[2] - a[2]));
          segment.rotation.y = Math.atan2(b[0] - a[0], b[2] - a[2]);
          this.courierRoute.add(segment);
        }
      }
    }
    this.missionMarker.visible = !!position;
    this.courierRoute.visible = !!position && courier;
    this.missionMarker.children[0]?.scale.setScalar(radius);
    if (position)
      this.missionMarker.position.set(position[0] ?? 0, 0, position[2] ?? 0);
  }
  constructor(
    readonly canvas: HTMLCanvasElement,
    private readonly assets: LoadedPackage,
  ) {
    this.graphics = new THREE.WebGLRenderer({ canvas, antialias: true });
    this.graphics.outputColorSpace = THREE.SRGBColorSpace;
    this.graphics.toneMapping = THREE.ACESFilmicToneMapping;
    this.graphics.toneMappingExposure = 1.25;
    const sky = document.createElement("canvas");
    sky.width = 2;
    sky.height = 256;
    const skyContext = sky.getContext("2d");
    if (skyContext) {
      const gradient = skyContext.createLinearGradient(0, 0, 0, 256);
      gradient.addColorStop(0, "#080912");
      gradient.addColorStop(1, "#202b46");
      skyContext.fillStyle = gradient;
      skyContext.fillRect(0, 0, 2, 256);
      const texture = new THREE.CanvasTexture(sky);
      texture.colorSpace = THREE.SRGBColorSpace;
      this.textures.add(texture);
      this.scene.background = texture;
    } else this.scene.background = new THREE.Color("#121827");
    this.scene.fog = new THREE.Fog("#121827", 100, 420);
    this.scene.add(new THREE.HemisphereLight("#aecbff", "#182014", 1.8));
    this.sun.position.set(-30, 60, 30);
    this.sun.castShadow = true;
    this.sun.shadow.mapSize.set(2048, 2048);
    this.sun.shadow.camera.left = -16;
    this.sun.shadow.camera.right = 16;
    this.sun.shadow.camera.top = 16;
    this.sun.shadow.camera.bottom = -16;
    this.sun.shadow.camera.far = 150;
    this.sun.shadow.bias = -0.0003;
    this.graphics.shadowMap.enabled = true;
    this.graphics.shadowMap.type = THREE.PCFSoftShadowMap;
    this.scene.add(this.sun, this.sun.target);
    const cube = new THREE.BoxGeometry(1, 1, 1);
    this.geometry.add(cube);
    this.cube = cube;
    for (const geometry of Object.values(this.trees))
      this.geometry.add(geometry);

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
    const ringGeometry = new THREE.RingGeometry(0.85, 1, 48);
    const terminalMaterial = new THREE.MeshBasicMaterial({
      color: "#56e5e1",
      side: THREE.DoubleSide,
      // Opaque beacons must occlude geometry behind them, regardless of draw order.
      depthWrite: true,
    });
    const targetMaterial = new THREE.MeshBasicMaterial({
      color: "#ffcf4a",
      side: THREE.DoubleSide,
      depthWrite: true,
    });
    this.geometry.add(ringGeometry);
    this.materials.add(terminalMaterial);
    this.materials.add(targetMaterial);
    for (const zone of assets.manifest.zones.filter(
      (z) =>
        z.kind === "terminal" || z.kind === "garage" || z.kind === "repair",
    )) {
      let material = terminalMaterial;
      if (zone.kind !== "terminal") {
        material = new THREE.MeshBasicMaterial({
          color: zone.kind === "repair" ? "#82ef8f" : "#99c4ff",
          side: THREE.DoubleSide,
          depthWrite: true,
        });
        this.materials.add(material);
      }
      const ring = new THREE.Mesh(ringGeometry, material);
      ring.rotation.x = -Math.PI / 2;
      ring.scale.setScalar(zone.radius);
      ring.position.set(zone.center[0], 0.06, zone.center[2]);
      this.scene.add(ring);
      const pole = new THREE.Mesh(cube, material);
      pole.position.set(zone.center[0], 1.5, zone.center[2]);
      pole.scale.set(0.3, 3, 0.3);
      this.scene.add(pole);
      const label = document.createElement("canvas");
      label.width = 256;
      label.height = 96;
      const context = label.getContext("2d");
      if (context) {
        context.fillStyle = "#142c37";
        context.fillRect(0, 0, 256, 96);
        context.strokeStyle =
          zone.kind === "terminal"
            ? "#66dfcb"
            : zone.kind === "repair"
              ? "#b8ee88"
              : "#a9c9ff";
        context.lineWidth = 8;
        context.strokeRect(4, 4, 248, 88);
        context.fillStyle = "#fff4df";
        context.font = "bold 27px system-ui";
        context.textAlign = "center";
        context.fillText(
          zone.kind === "terminal"
            ? "JOBS"
            : zone.kind === "repair"
              ? "REPAIR · 50"
              : "GARAGE",
          128,
          38,
        );
        context.font = "20px system-ui";
        context.fillText(
          zone.id
            .replace(/^(terminal|garage|repair)_/, "")
            .replaceAll("_", " ")
            .toUpperCase(),
          128,
          70,
        );
        const texture = new THREE.CanvasTexture(label);
        texture.colorSpace = THREE.SRGBColorSpace;
        this.textures.add(texture);
        const material = new THREE.SpriteMaterial({ map: texture });
        this.materials.add(material);
        const sign = new THREE.Sprite(material);
        sign.position.set(zone.center[0], 4.2, zone.center[2]);
        sign.scale.set(4.5, 1.7, 1);
        this.signs.push(sign);
        this.scene.add(sign);
      }
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
    this.routeMaterial = targetMaterial;
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
    const localMaterials = new Map<string, THREE.MeshStandardMaterial>();
    this.localRig = new CharacterRig(
      cube,
      (color) => {
        let m = localMaterials.get(color);
        if (!m) {
          m = new THREE.MeshStandardMaterial({
            color,
            roughness: 0.85,
            transparent: true,
          });
          localMaterials.set(color, m);
          this.materials.add(m);
        }
        return m;
      },
      0,
      this.weaponModels,
    );
    this.actor.add(this.localRig.root);
    this.actor.traverse((o) => {
      if (o instanceof THREE.Mesh) o.castShadow = true;
    });
    const faceMaterial = new THREE.MeshStandardMaterial({ color: "#263e45" });
    this.materials.add(faceMaterial);
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
      // Neon facade accents remain luminous without adding per-building lights.
      const architectural = p.id.includes("-architecture-");
      const luminous =
        !architectural &&
        (p.id.includes("-front-") ||
          p.id.includes("-side-") ||
          p.id.endsWith("-roof"));
      const color =
        p.role === "road"
          ? "#242b35"
          : p.role === "sidewalk"
            ? "#535a63"
            : p.role === "ground"
              ? "#17221e"
              : p.role === "building" && !luminous
                ? new THREE.Color(p.color)
                    .lerp(
                      new THREE.Color("#192034"),
                      architectural ? 0.12 : 0.35,
                    )
                    .getStyle()
                : p.color;
      const key = `${color}:${layer}:${luminous}:${treeShape(p)}`;
      let material = this.palette.get(key);
      if (!material) {
        material = new THREE.MeshStandardMaterial({
          color,
          emissive: luminous ? color : "#000000",
          emissiveIntensity: luminous ? 0.65 : 0,
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
      const first = values[0];
      if (!first) continue;
      const shape = treeShape(first);
      const mesh = new THREE.InstancedMesh(
        shape === "box" ? this.cube : this.trees[shape],
        material,
        values.length,
      );
      mesh.receiveShadow = true;
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
    const { districts, roads } = this.assets.manifest;
    const group = this.batch(
      chunk.primitives.map((p) => buildingAppearance(p, districts)),
    );
    const facade = this.batch(
      facadeDetails(chunk.primitives, districts, roads),
    );
    facade.name = "facades";
    facade.visible = !this.low;
    group.add(facade);
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
  get streamingMetrics() {
    let matrices = 0;
    for (const group of [...this.chunks.values(), ...this.fallback.values()])
      group.traverse((o) => {
        if (o instanceof THREE.InstancedMesh) matrices += o.count * 64;
      });
    return {
      ...this.stream.metrics,
      matrixBytes: matrices + this.dynamic.matrixBytes,
      drawCalls: this.graphics.info.render.calls,
      geometries: this.graphics.info.memory.geometries,
      triangles: this.graphics.info.render.triangles,
    };
  }
  quality(low: boolean): void {
    const changed = this.low !== low;
    this.low = low;
    this.graphics.shadowMap.enabled = !low;
    this.sun.castShadow = !low;
    if (changed) {
      for (const material of this.materials) material.needsUpdate = true;
    }
    if (low && this.sun.shadow.map) {
      this.sun.shadow.map.dispose();
      this.sun.shadow.map = null;
    }
    for (const group of this.chunks.values()) {
      const facade = group.getObjectByName("facades");
      if (facade) facade.visible = !low;
    }
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
    dt = 0.016,
    reduced = false,
    aiming = false,
    reloading = false,
    disabled = false,
    skidding = false,
    tick = 0,
    pitch = 0,
    queries?: PredictionWorld,
    surrendering = false,
  ): void {
    this.presentationTick = tick;
    this.dustClock += dt;
    if (
      driving &&
      skidding &&
      !this.low &&
      Math.hypot(motor.velocity[0], motor.velocity[2]) > 4 &&
      this.dustClock > 0.12
    ) {
      this.dustClock = 0;
      const index = this.dustCursor++ % 12;
      let dust = this.dust[index];
      if (!dust) {
        const material = new THREE.MeshBasicMaterial({
          color: "#d6c3a4",
          transparent: true,
          depthWrite: false,
        });
        this.materials.add(material);
        const mesh = new THREE.Mesh(this.cube, material);
        this.scene.add(mesh);
        dust = { mesh, until: 0 };
        this.dust[index] = dust;
      }
      dust.mesh.position.set(
        motor.position[0] + Math.sin(motor.yaw) * 2,
        0.3,
        motor.position[2] + Math.cos(motor.yaw) * 2,
      );
      dust.mesh.scale.setScalar(0.3);
      dust.mesh.material.opacity = 0.35;
      dust.mesh.visible = true;
      dust.until = performance.now() + 450;
    }
    this.frameDt = dt;
    this.reduced = reduced;
    this.localRig.pose(
      {
        motor,
        driving,
        aiming,
        reloading,
        incapacitated: disabled,
        surrendering,
        fired: this.localFired,
        weapon: this.equipment?.weapon_id,
        obstructed:
          aiming &&
          !!queries?.raycastStatic({
            version: 1,
            origin: [
              motor.position[0],
              motor.position[1] + 1.45,
              motor.position[2],
            ],
            direction: [
              -Math.sin(motor.yaw) * Math.cos(pitch),
              Math.sin(pitch),
              -Math.cos(motor.yaw) * Math.cos(pitch),
            ],
            max_distance: this.equipment?.weapon_id === "pistol" ? 0.65 : 1.2,
          }).hit,
        pitch,
        reloadProgress: equipmentProgress(
          this.equipment,
          tick,
          this.assets.manifest.weapons,
        ).reload,
        equipProgress: equipmentProgress(
          this.equipment,
          tick,
          this.assets.manifest.weapons,
        ).equip,
      },
      dt,
      performance.now() / 1000,
      reduced,
    );
    this.localFired = false;
    for (const sign of this.signs)
      sign.visible =
        sign.position.distanceTo(new THREE.Vector3(...motor.position)) < 90;
    this.sun.position.set(motor.position[0] - 30, 60, motor.position[2] + 30);
    this.sun.target.position.set(motor.position[0], 0, motor.position[2]);
    this.stream.update(motor.position);
    this.actor.position.set(...motor.position);
    this.actor.rotation.y = motor.yaw;
    this.actor.scale.setScalar(driving ? 0.6 : 1);
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
  private visualMaterial(color: string): THREE.MeshStandardMaterial {
    let m = this.carMaterials.get(color);
    if (!m) {
      m = new THREE.MeshStandardMaterial({ color, roughness: 0.82 });
      this.carMaterials.set(color, m);
      this.materials.add(m);
    }
    return m;
  }
  remotes(actors: readonly RenderActor[]): void {
    this.dynamic.begin();
    const players = new Set(
      actors
        .filter(
          (a) =>
            a.descriptor.entity_kind === "player" &&
            a.state.kind === "character",
        )
        .map((a) => a.id),
    );
    this.nametags.retain(players);
    for (const actor of actors) {
      if (!players.has(actor.id) || actor.state.kind !== "character") continue;
      if (actor.state.data.mode !== "driving") continue;
      let height = actor.motor.position[1] + 1.8;
      if (actor.state.data.mode === "driving") {
        const vehicleId = actor.state.data.vehicle_id;
        const vehicle = actors.find(
          (a) => a.id === vehicleId && a.state.kind === "vehicle",
        );
        const template = this.assets.manifest.vehicle_templates.find(
          (t) => t.id === vehicle?.descriptor.template_id,
        );
        height = vehicle
          ? vehicle.motor.position[1] + (template?.dimensions[1] ?? 1.4) + 0.05
          : actor.motor.position[1] + 1.1;
      }
      this.nametags.update(
        actor.id,
        actor.descriptor.display_name ?? "Guest",
        actor.motor.position[0],
        height,
        actor.motor.position[2],
      );
    }
    const present = new Set(actors.map((a) => a.id));
    for (const [id] of this.remoteActors)
      if (!present.has(id)) {
        this.remoteActors.delete(id);
        this.rigs.delete(id);
        this.lods.delete(id);
        this.wheelData.delete(id);
      }
    for (const actor of actors) {
      let group = this.remoteActors.get(actor.id);
      if (!group) {
        group = new THREE.Group();
        if (actor.state.kind === "vehicle") {
          const template = this.assets.manifest.vehicle_templates.find(
            (t) => t.id === actor.descriptor.template_id,
          );
          const visual = this.assets.vehicleVisuals.find(
            (v) => v.id === template?.visual_asset_id,
          );
          const { body, wheels } = this.vehicleModels.create(
            template,
            visual?.primitives.find((p) => p.id === "body")?.color ?? "#4e818e",
            actor.descriptor.entity_kind === "police",
          );
          group.add(body, ...wheels);
          const smoke = new THREE.Mesh(this.cube, this.staleMaterial);
          smoke.name = "disabled-smoke";
          smoke.position.y = 2.1;
          smoke.scale.set(0.65, 1, 0.65);
          group.add(smoke);
          this.wheelData.set(actor.id, {
            body,
            wheels,
            roll: 0,
            previous: new THREE.Vector3(...actor.motor.position),
          });
        } else {
          const rig = new CharacterRig(
            this.cube,
            (c) => this.visualMaterial(c),
            actor.id % 4,
            this.weaponModels,
            actor.descriptor.entity_kind === "police",
          );
          this.rigs.set(actor.id, rig);
          group.add(rig.root);
        }
        this.remoteActors.set(actor.id, group);
      }
      if (actor.descriptor.tshirt_colour != null)
        this.rigs.get(actor.id)?.setShirtColour(actor.descriptor.tshirt_colour);
      group.position.set(...actor.motor.position);
      group.rotation.y = actor.motor.yaw;
      group.scale.setScalar(
        actor.state.kind === "character" && actor.state.data.mode === "driving"
          ? 0.6
          : 1,
      );
      if (actor.state.kind === "vehicle") {
        const data = this.wheelData.get(actor.id);
        if (!data) continue;
        data.roll += wheelRollDelta(
          data.previous,
          group.position,
          group.rotation.y,
        );
        data.previous.copy(group.position);
        const steering = actor.steering ?? actor.state.data.motor.steering;
        data.body.rotation.z = carLean(
          actor.state.data.motor.speed,
          steering,
          this.reduced,
        );
        steerWheels(data.wheels, steering);
        data.wheels.forEach((wheel) => {
          const roller = wheel.children[0];
          if (roller) roller.rotation.x = data.roll;
        });
        const smoke = group.getObjectByName("disabled-smoke");
        if (smoke) {
          smoke.visible = actor.state.data.disabled;
          smoke.rotation.y = this.reduced ? 0 : performance.now() / 1500;
        }
      } else {
        const rig = this.rigs.get(actor.id);
        if (!rig) continue;
        rig.pose(
          {
            motor: actor.motor,
            driving:
              actor.state.kind === "character" &&
              actor.state.data.mode === "driving",
            incapacitated: actor.state.data.health === 0,
            ...(actor.state.kind === "character"
              ? {
                  weapon: actor.state.data.equipment?.weapon_id,
                  aiming: actor.state.data.equipment?.aiming,
                  pitch: actor.state.data.equipment?.aim_pitch,
                  reloadProgress: equipmentProgress(
                    actor.state.data.equipment,
                    this.presentationTick,
                    this.assets.manifest.weapons,
                  ).reload,
                  equipProgress: equipmentProgress(
                    actor.state.data.equipment,
                    this.presentationTick,
                    this.assets.manifest.weapons,
                  ).equip,
                  reloading:
                    equipmentProgress(
                      actor.state.data.equipment,
                      this.presentationTick,
                      this.assets.manifest.weapons,
                    ).reload !== null,
                }
              : {}),
          },
          this.frameDt,
          performance.now() / 1000,
          this.reduced,
        );
        if (actor.state.data.health === 0) group.position.y += 0.4;
      }
      group.updateMatrixWorld(true);
      if (
        actor.descriptor.entity_kind === "player" &&
        actor.state.kind === "character" &&
        actor.state.data.mode !== "driving"
      ) {
        const rig = this.rigs.get(actor.id);
        if (rig) {
          rig.headTop(this.nametagAnchor);
          this.nametags.update(
            actor.id,
            actor.descriptor.display_name ?? "Guest",
            this.nametagAnchor.x,
            this.nametagAnchor.y,
            this.nametagAnchor.z,
          );
        }
      }
      const distant =
        actor.state.kind !== "vehicle" &&
        group.position.distanceTo(this.actor.position) > (this.low ? 25 : 55);
      if (distant) {
        let simple = this.lods.get(actor.id);
        if (!simple) {
          simple = new THREE.Mesh(this.capsule, this.staleMaterial);
          this.lods.set(actor.id, simple);
        }
        simple.material = actor.stale
          ? this.staleMaterial
          : this.visualMaterial(
              actor.descriptor.entity_kind === "pedestrian"
                ? "#b16f5b"
                : "#4a9d9a",
            );
        simple.rotation.z = actor.state.data.health === 0 ? Math.PI / 2 : 0;
        simple.position.set(0, 0.87, 0);
        simple.updateMatrix();
        simple.matrixWorld.multiplyMatrices(group.matrixWorld, simple.matrix);
        this.dynamic.add(simple);
      } else
        group.traverse((o) => {
          if (o instanceof THREE.Mesh && o.visible) {
            let visible = true;
            for (let parent = o.parent; parent; parent = parent.parent)
              visible &&= parent.visible;
            if (visible) this.dynamic.add(o);
          }
        });
    }
    this.dynamic.end();
  }
  render(distance: number): void {
    this.nametags.layout(
      this.camera,
      this.canvas.clientHeight,
      this.actor.position,
    );
    this.weaponEffects.update(
      performance.now(),
      Math.min(this.frameDt, 0.05),
      this.low,
    );
    for (const d of this.dust) {
      const left = (d.until - performance.now()) / 450;
      d.mesh.visible = left > 0 && !this.low;
      if (d.mesh.visible) {
        d.mesh.material.opacity = Math.max(0, left) * 0.35;
        d.mesh.scale.setScalar(0.3 + (1 - left) * 0.5);
        d.mesh.position.y += this.frameDt * 0.25;
      }
    }
    this.actor.visible = distance > 0.35;
    this.actor.traverse((object) => {
      if (object instanceof THREE.Mesh)
        object.material.opacity =
          distance < 0.8 ? Math.max(0.05, distance / 0.8) : 1;
    });
    this.graphics.render(this.scene, this.camera);
  }
  dispose(): void {
    this.nametags.dispose();
    this.weaponEffects.dispose();
    this.weaponModels.dispose();
    this.stream.dispose();
    this.dynamic.dispose();
    for (const texture of this.textures) texture.dispose();
    this.textures.clear();
    this.sun.shadow.map?.dispose();
    for (const group of this.fallback.values())
      group.traverse((o) => {
        if (o instanceof THREE.InstancedMesh) o.dispose();
      });
    this.fallback.clear();
    this.palette.clear();
    for (const g of this.geometry) g.dispose();
    for (const m of this.materials) m.dispose();
    this.remoteActors.clear();
    this.rigs.clear();
    this.lods.clear();
    this.wheelData.clear();
    this.vehicleModels.dispose();
    this.scene.clear();
    this.graphics.dispose();
  }
}
