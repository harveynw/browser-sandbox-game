import * as THREE from "three";
import type { Shot } from "@port-meridian/protocol";

/** Fixed GPU/CPU pools; saturation replaces cosmetics only. No lights or per-shot geometry. */
export class WeaponEffects {
  private readonly cube = new THREE.BoxGeometry();
  private readonly flashMaterial = new THREE.MeshBasicMaterial({
    color: "#ffe4a1",
  });
  private readonly brass = new THREE.MeshBasicMaterial({ color: "#bca35d" });
  private readonly dust = new THREE.MeshBasicMaterial({ color: "#b9ac98" });
  private readonly tracerMaterial = new THREE.LineBasicMaterial({
    color: "#f6d393",
    transparent: true,
    opacity: 0.5,
  });
  private readonly casings = new THREE.InstancedMesh(this.cube, this.brass, 64);
  private readonly particles = new THREE.InstancedMesh(
    this.cube,
    this.dust,
    96,
  );
  private readonly transform = new THREE.Object3D();
  private readonly color = new THREE.Color();
  private readonly shells = Array.from({ length: 64 }, () => ({
    position: new THREE.Vector3(),
    velocity: new THREE.Vector3(),
    until: 0,
  }));
  private readonly debris = Array.from({ length: 96 }, () => ({
    position: new THREE.Vector3(),
    velocity: new THREE.Vector3(),
    until: 0,
    color: 0,
  }));
  private readonly flashes: {
    mesh: THREE.Mesh;
    line: THREE.Line;
    until: number;
  }[] = [];
  private cursor = 0;
  private shellCursor = 0;
  private debrisCursor = 0;
  constructor(private readonly scene: THREE.Scene) {
    this.casings.frustumCulled = this.particles.frustumCulled = false;
    this.casings.count = this.particles.count = 0;
    this.casings.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
    this.particles.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
    scene.add(this.casings, this.particles);
    for (let i = 0; i < 32; i++) {
      const mesh = new THREE.Mesh(this.cube, this.flashMaterial);
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute(
        "position",
        new THREE.BufferAttribute(new Float32Array(6), 3),
      );
      const line = new THREE.Line(geometry, this.tracerMaterial);
      mesh.visible = line.visible = false;
      scene.add(mesh, line);
      this.flashes.push({ mesh, line, until: 0 });
    }
  }
  shot(
    shot: Shot,
    muzzle: THREE.Vector3,
    now: number,
    reduced: boolean,
    low: boolean,
    surface: "metal" | "ground" | "masonry" | "actor" | "none",
  ): void {
    const slot = this.flashes[this.cursor++ % 32];
    if (!slot) return;
    slot.mesh.position.copy(muzzle);
    slot.mesh.lookAt(...shot.end);
    const size =
      shot.weapon_id === "pistol"
        ? 0.09
        : shot.weapon_id === "rifle"
          ? 0.14
          : 0.12;
    slot.mesh.scale.set(size, size, size * (reduced ? 0.5 : 2.5));
    slot.mesh.visible = true;
    const positions = slot.line.geometry.getAttribute("position");
    // Keep the authoritative segment, including chest-to-obstruction; never retarget hits.
    positions.setXYZ(0, ...shot.origin);
    positions.setXYZ(1, ...shot.end);
    positions.needsUpdate = true;
    slot.line.geometry.computeBoundingSphere();
    slot.line.visible = !reduced && shot.shot_sequence % 3 === 0;
    slot.until = now + (reduced ? 20 : 55);
    if (!low) {
      const shell = this.shells[this.shellCursor++ % 64];
      if (!shell) return;
      shell.position.copy(muzzle);
      shell.position.y -= 0.05;
      shell.velocity.set(
        1.4 * Math.cos(shot.shot_sequence),
        1.5,
        1.4 * Math.sin(shot.shot_sequence),
      );
      shell.until = now + 650;
    }
    if (surface !== "none") {
      const count = low || reduced ? 1 : 4;
      for (let i = 0; i < count; i++) {
        const p = this.debris[this.debrisCursor++ % 96];
        if (!p) continue;
        p.position.set(...shot.end);
        p.velocity.set(
          Math.sin(shot.shot_sequence + i * 2) * 1.2,
          0.5 + i * 0.2,
          Math.cos(shot.shot_sequence + i * 2) * 1.2,
        );
        p.color =
          surface === "metal"
            ? 0xffd78a
            : surface === "ground"
              ? 0x9e906b
              : surface === "actor"
                ? 0x90969a
                : 0xc9c1b4;
        p.until = now + 220;
      }
    }
  }
  impact(
    position: readonly number[],
    severity: number,
    now: number,
    low: boolean,
  ): void {
    for (let i = 0; i < (low ? 1 : 6); i++) {
      const p = this.debris[this.debrisCursor++ % 96];
      if (!p) continue;
      p.position.set(position[0] ?? 0, position[1] ?? 0, position[2] ?? 0);
      p.velocity.set(
        Math.sin(i * 2) * severity,
        0.5 + severity,
        Math.cos(i * 2) * severity,
      );
      p.color = 0xffd78a;
      p.until = now + 220;
    }
  }
  update(now: number, dt: number, low: boolean): void {
    for (const slot of this.flashes)
      if (now >= slot.until) slot.mesh.visible = slot.line.visible = false;
    let count = 0;
    for (const s of this.shells) {
      if (s.until <= now || low) continue;
      s.velocity.y -= 9.8 * dt;
      s.position.addScaledVector(s.velocity, dt);
      this.transform.position.copy(s.position);
      this.transform.rotation.set(now * 0.01, count, now * 0.006);
      this.transform.scale.set(0.018, 0.018, 0.05);
      this.transform.updateMatrix();
      this.casings.setMatrixAt(count++, this.transform.matrix);
    }
    this.casings.count = count;
    this.casings.instanceMatrix.needsUpdate = true;
    count = 0;
    for (const p of this.debris) {
      if (p.until <= now) continue;
      p.position.addScaledVector(p.velocity, dt);
      this.transform.position.copy(p.position);
      this.transform.scale.setScalar(
        Math.max(0.005, ((p.until - now) / 220) * 0.045),
      );
      this.transform.updateMatrix();
      this.particles.setMatrixAt(count, this.transform.matrix);
      this.particles.setColorAt(count++, this.color.setHex(p.color));
    }
    this.particles.count = count;
    this.particles.instanceMatrix.needsUpdate = true;
    if (this.particles.instanceColor)
      this.particles.instanceColor.needsUpdate = true;
  }
  get counts() {
    return {
      flashes: this.flashes.filter((s) => s.mesh.visible).length,
      casings: this.casings.count,
      particles: this.particles.count,
    };
  }
  dispose(): void {
    for (const s of this.flashes) {
      this.scene.remove(s.mesh, s.line);
      s.line.geometry.dispose();
    }
    this.scene.remove(this.casings, this.particles);
    this.casings.dispose();
    this.particles.dispose();
    this.cube.dispose();
    this.flashMaterial.dispose();
    this.brass.dispose();
    this.dust.dispose();
    this.tracerMaterial.dispose();
  }
}
