import * as THREE from "three";

export const NAMETAG_DISTANCE = 40;
export const NAMETAG_HEIGHT_PX = 22;
export const NAMETAG_GAP_PX = 6;

/** One cached billboard per visible remote player; labels are plain canvas text. */
export class PlayerNametags {
  private readonly labels = new Map<
    number,
    {
      name: string;
      sprite: THREE.Sprite;
      texture: THREE.CanvasTexture;
      anchor: THREE.Vector3;
    }
  >();
  private readonly viewPosition = new THREE.Vector3();
  private readonly up = new THREE.Vector3();
  constructor(private readonly scene: THREE.Scene) {}
  update(id: number, name: string, x: number, y: number, z: number): void {
    let label = this.labels.get(id);
    if (label?.name !== name) {
      this.remove(id);
      const canvas = document.createElement("canvas"),
        context = canvas.getContext("2d");
      if (!context) return;
      context.font = "600 28px Arial, sans-serif";
      canvas.width = Math.ceil(context.measureText(name).width) + 28;
      canvas.height = 48;
      context.fillStyle = "rgba(0, 0, 0, 0.65)";
      context.fillRect(0, 0, canvas.width, canvas.height);
      context.font = "600 28px Arial, sans-serif";
      context.fillStyle = "#ffffff";
      context.textAlign = "center";
      context.textBaseline = "middle";
      context.fillText(name, canvas.width / 2, canvas.height / 2);
      const texture = new THREE.CanvasTexture(canvas);
      texture.colorSpace = THREE.SRGBColorSpace;
      texture.generateMipmaps = false;
      texture.minFilter = THREE.LinearFilter;
      const material = new THREE.SpriteMaterial({
        map: texture,
        transparent: true,
        depthTest: true,
        depthWrite: false,
        sizeAttenuation: true,
        toneMapped: false,
      });
      const sprite = new THREE.Sprite(material);
      sprite.center.set(0.5, 0);
      label = { name, sprite, texture, anchor: new THREE.Vector3() };
      this.labels.set(id, label);
      this.scene.add(sprite);
    }
    label.anchor.set(x, y, z);
    label.sprite.position.copy(label.anchor);
  }
  layout(
    camera: THREE.PerspectiveCamera,
    viewportHeight: number,
    viewer: THREE.Vector3,
  ): void {
    camera.updateMatrixWorld();
    this.up.setFromMatrixColumn(camera.matrixWorld, 1);
    for (const { sprite, texture, anchor } of this.labels.values()) {
      const depth = -this.viewPosition
        .copy(anchor)
        .applyMatrix4(camera.matrixWorldInverse).z;
      sprite.visible =
        depth > camera.near &&
        anchor.distanceToSquared(viewer) <= NAMETAG_DISTANCE ** 2;
      if (!sprite.visible) continue;
      // Convert CSS pixels to world units at this anchor's camera depth. This
      // cancels both perspective distance and FOV changes, including aim zoom.
      const unitsPerPixel =
        (2 * depth) /
        (Math.max(1, viewportHeight) * camera.projectionMatrix.elements[5]);
      const height = NAMETAG_HEIGHT_PX * unitsPerPixel;
      const image = texture.image as HTMLCanvasElement;
      sprite.scale.set((height * image.width) / image.height, height, 1);
      sprite.position
        .copy(anchor)
        .addScaledVector(this.up, NAMETAG_GAP_PX * unitsPerPixel);
    }
  }
  retain(ids: ReadonlySet<number>): void {
    for (const id of this.labels.keys()) if (!ids.has(id)) this.remove(id);
  }
  private remove(id: number): void {
    const label = this.labels.get(id);
    if (!label) return;
    this.scene.remove(label.sprite);
    label.sprite.material.dispose();
    label.texture.dispose();
    this.labels.delete(id);
  }
  dispose(): void {
    for (const id of this.labels.keys()) this.remove(id);
  }
}
