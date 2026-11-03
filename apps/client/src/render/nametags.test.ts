import { afterEach, expect, it, vi } from "vitest";
import { Scene, Sprite } from "three";
import { PlayerNametags } from "./nametags";
afterEach(() => vi.unstubAllGlobals());
it("caches unchanged names, follows players and disposes departed or renamed labels", () => {
  const fillText = vi.fn(),
    fillRect = vi.fn();
  vi.stubGlobal("document", {
    createElement: () => ({
      width: 0,
      height: 0,
      getContext: () => ({
        measureText: () => ({ width: 120 }),
        fillRect,
        fillText,
      }),
    }),
  });
  const scene = new Scene(),
    tags = new PlayerNametags(scene);
  tags.update(1, "<Alice>", 1, 2.15, 3);
  const sprite = scene.children[0];
  if (!(sprite instanceof Sprite)) throw new Error("Missing player billboard");
  expect(fillText).toHaveBeenCalledWith("<Alice>", 74, 24);
  expect(sprite.material.depthWrite).toBe(false);
  expect(sprite.material.depthTest).toBe(true);
  if (!sprite.material.map) throw new Error("Missing label texture");
  const disposed = vi.spyOn(sprite.material, "dispose"),
    textureDisposed = vi.spyOn(sprite.material.map, "dispose");
  tags.update(1, "<Alice>", 4, 2.15, 6);
  expect(scene.children).toEqual([sprite]);
  expect(sprite.position.toArray()).toEqual([4, 2.15, 6]);
  expect(fillText).toHaveBeenCalledTimes(1);
  tags.update(1, "Bob", 4, 2.15, 6);
  expect(disposed).toHaveBeenCalledOnce();
  expect(textureDisposed).toHaveBeenCalledOnce();
  tags.retain(new Set());
  expect(scene.children).toHaveLength(0);
  tags.dispose();
});

it("keeps projected size and head gap fixed across distance, FOV and resize, and hides beyond 40m", async () => {
  const { PerspectiveCamera, Vector3 } = await import("three");
  const { NAMETAG_HEIGHT_PX, NAMETAG_GAP_PX } = await import("./nametags");
  vi.stubGlobal("document", {
    createElement: () => ({
      width: 0,
      height: 0,
      getContext: () => ({
        measureText: () => ({ width: 120 }),
        fillRect: vi.fn(),
        fillText: vi.fn(),
      }),
    }),
  });
  const scene = new Scene(),
    tags = new PlayerNametags(scene),
    camera = new PerspectiveCamera(65, 16 / 9, 0.05, 650);
  const viewer = new Vector3();
  tags.update(1, "Alice", 0, 1.8, -10);
  const sprite = scene.children[0];
  if (!(sprite instanceof Sprite)) throw new Error("Missing player billboard");
  for (const distance of [2, 10, 35])
    for (const fov of [54, 70])
      for (const height of [480, 1080]) {
        camera.fov = fov;
        camera.updateProjectionMatrix();
        tags.update(1, "Alice", 0, 1.8, -distance);
        tags.layout(camera, height, viewer);
        const bottom = sprite.position.clone().project(camera);
        const top = sprite.position
          .clone()
          .add(new Vector3(0, sprite.scale.y, 0))
          .project(camera);
        const head = new Vector3(0, 1.8, -distance).project(camera);
        expect(((top.y - bottom.y) * height) / 2).toBeCloseTo(
          NAMETAG_HEIGHT_PX,
        );
        expect(((bottom.y - head.y) * height) / 2).toBeCloseTo(NAMETAG_GAP_PX);
        expect(sprite.visible).toBe(true);
      }
  tags.update(1, "Alice", 0, 1.8, -41);
  tags.layout(camera, 720, viewer);
  expect(sprite.visible).toBe(false);
  tags.update(1, "Alice", 0, 1.8, -10);
  tags.layout(camera, 720, viewer);
  expect(sprite.visible).toBe(true);
  tags.update(1, "Alice", 0, 1.8, 10);
  tags.layout(camera, 720, viewer);
  expect(sprite.visible).toBe(false);
  tags.dispose();
});
