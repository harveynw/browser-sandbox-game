import * as THREE from "three";

/** A camera-relative celestial sprite: constant angular size, with terrain occlusion. */
export function createMoon(): THREE.Sprite {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 512;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Moon texture requires a 2D canvas");
  const glow = ctx.createRadialGradient(256, 256, 78, 256, 256, 256);
  glow.addColorStop(0, "rgba(190,215,255,0.45)");
  glow.addColorStop(0.4, "rgba(150,185,245,0.14)");
  glow.addColorStop(1, "rgba(130,170,240,0)");
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, 512, 512);
  const disc = ctx.createRadialGradient(225, 223, 6, 256, 256, 90);
  disc.addColorStop(0, "#fffef0");
  disc.addColorStop(1, "#d8e5ed");
  ctx.beginPath();
  ctx.arc(256, 256, 90, 0, Math.PI * 2);
  ctx.fillStyle = disc;
  ctx.fill();
  ctx.save();
  ctx.clip();
  ctx.fillStyle = "rgba(112,139,158,0.13)";
  for (const [x, y, r] of [
    [230, 225, 23],
    [266, 211, 14],
    [288, 248, 25],
    [240, 277, 19],
    [277, 296, 12],
    [209, 252, 10],
  ] as const) {
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();
  const map = new THREE.CanvasTexture(canvas);
  map.colorSpace = THREE.SRGBColorSpace;
  const material = new THREE.SpriteMaterial({
    map,
    transparent: true,
    depthWrite: false,
    fog: false,
    toneMapped: false,
  });
  const moon = new THREE.Sprite(material);
  moon.name = "moon";
  moon.scale.set(900, 900, 1);
  return moon;
}

// Matches the cool directional light's southwest bearing, above the mountain ridge.
export const moonOffset = new THREE.Vector3(-1, 1.1, 1)
  .normalize()
  .multiplyScalar(4500);
