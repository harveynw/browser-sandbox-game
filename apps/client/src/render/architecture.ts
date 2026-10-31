import { neighbourhoodDetails } from "./neighbourhood-details";
import type { District, Primitive, Roads } from "@port-meridian/content";

export function buildingDistrict(p: Primitive, districts: readonly District[]) {
  return districts.find(
    ({ rect }) =>
      p.center[0] >= rect.min[0] &&
      p.center[0] < rect.max[0] &&
      p.center[2] >= rect.min[1] &&
      p.center[2] < rect.max[1],
  );
}
function seed(id: string): number {
  return [...id].reduce(
    (h, c) => Math.imul(h ^ c.charCodeAt(0), 16777619) >>> 0,
    2166136261,
  );
}
export function buildingAppearance(
  p: Primitive,
  districts: readonly District[],
): Primitive {
  if (p.role !== "building") return p;
  const style =
    p.building?.architecture.style ??
    buildingDistrict(p, districts)?.architecture.style;
  const palette =
    style === "quay"
      ? ["#a77861", "#bdac88", "#8d6860", "#b39a7d"]
      : style === "industrial"
        ? ["#827a69", "#756b62", "#8b8070", "#777e79"]
        : style === "residential"
          ? ["#b3b6a0", "#a5b3ad", "#b5b0a6"]
          : style === "civic"
            ? ["#c9c4ae", "#bcbfb5", "#d0c6b4"]
            : style === "market"
              ? ["#c49b72", "#af826c", "#b6ac86", "#91a49b"]
              : style === "dockside"
                ? ["#6d8994", "#81989c", "#89958c"]
                : null;
  return palette
    ? { ...p, color: palette[seed(p.id) % palette.length] ?? p.color }
    : p;
}

/** Small split grammar: shell → floors → bays; ground floor follows street frontage.
 * All parts share the renderer's unit cube and small material palette. Roof equipment
 * and shallow facade relief are decorative; the baked shell owns collision. */
export function architectureDetails(
  p: Primitive,
  district: District,
  roads: Roads,
): Primitive[] {
  const { style, floor_height: floorHeight } =
    p.building?.architecture ?? district.architecture;
  const [x, y, z] = p.center,
    [hx, hy, hz] = p.half_extents;
  const base = y - hy,
    top = y + hy,
    h = seed(p.id);
  const industrial = style === "industrial",
    shop =
      !industrial &&
      (p.building ? p.building.purpose === "shop_housing" : h % 2 === 0);
  const result: Primitive[] = [];
  const trim = industrial ? "#aaa99c" : "#d7c8a8";
  const dark = "#34464b",
    glass = "#718b91",
    metal = "#525b5a";
  function box(
    label: string,
    center: Primitive["center"],
    half: Primitive["half_extents"],
    color: string,
  ) {
    result.push({
      id: `${p.id}-architecture-${label}`,
      role: "building",
      center,
      half_extents: half,
      color,
      collider_id: null,
    });
  }
  // Select the closest street, including east/west approaches; no fixed 'front'.
  const faces: { axis: 0 | 2; sign: number; distance: number }[] = [
    {
      axis: 0,
      sign: -1,
      distance: Math.min(
        ...roads.x.filter((v) => v < x).map((v) => x - hx - v),
      ),
    },
    {
      axis: 0,
      sign: 1,
      distance: Math.min(
        ...roads.x.filter((v) => v > x).map((v) => v - x - hx),
      ),
    },
    {
      axis: 2,
      sign: -1,
      distance: Math.min(
        ...roads.z.filter((v) => v < z).map((v) => z - hz - v),
      ),
    },
    {
      axis: 2,
      sign: 1,
      distance: Math.min(
        ...roads.z.filter((v) => v > z).map((v) => v - z - hz),
      ),
    },
  ];
  const front = faces.reduce((a, b) => (a.distance <= b.distance ? a : b));
  // Local facade coordinates: u runs along wall, v is height, depth projects outwards.
  function faceBox(
    face: typeof front,
    label: string,
    u: number,
    v: number,
    width: number,
    height: number,
    depth: number,
    color: string,
    offset = 0,
  ) {
    const axis = face.axis,
      tangent = axis === 0 ? 2 : 0;
    const center: Primitive["center"] = [x, base + v, z];
    const half: Primitive["half_extents"] = [depth / 2, height / 2, depth / 2];
    center[axis] =
      center[axis] + face.sign * (p.half_extents[axis] + offset + depth / 2);
    center[tangent] += u;
    half[tangent] = width / 2;
    box(`${axis}-${face.sign}-${label}`, center, half, color);
  }
  const floors = Math.max(1, Math.round((hy * 2) / floorHeight));
  if (["residential", "civic", "market", "dockside"].includes(style)) {
    neighbourhoodDetails({
      style,
      h,
      x,
      z,
      top,
      hx,
      hy,
      hz,
      floors,
      floorHeight,
      faces,
      front,
      box,
      faceBox,
    });
    return exposedDetails(p, result);
  }
  for (const face of faces) {
    const width = (face.axis === 0 ? hz : hx) * 2;
    const bays = Math.max(1, Math.floor((width - 1.4) / (industrial ? 4 : 3)));
    const pitch = (width - 1.4) / bays;
    for (let floor = 0; floor < floors; floor++) {
      for (let bay = 0; bay < bays; bay++) {
        const u = (bay + 0.5 - bays / 2) * pitch;
        if (floor === 0 && face === front) continue;
        const v =
          floor * floorHeight + floorHeight * (industrial ? 0.72 : 0.55);
        const w = industrial ? Math.min(2.8, pitch - 0.5) : 1.25;
        const wh = industrial ? 1.1 : 1.55;
        faceBox(
          face,
          `frame-${floor}-${bay}`,
          u,
          v,
          w + 0.24,
          wh + 0.24,
          0.08,
          trim,
        );
        faceBox(face, `pane-${floor}-${bay}`, u, v, w, wh, 0.035, glass, 0.085);
        if (!industrial)
          faceBox(
            face,
            `sill-${floor}-${bay}`,
            u,
            v - wh / 2 - 0.08,
            w + 0.38,
            0.14,
            0.22,
            trim,
          );
      }
      if (!industrial && floor > 0)
        faceBox(
          face,
          `floor-${floor}`,
          0,
          floor * floorHeight,
          width + 0.1,
          0.16,
          0.12,
          trim,
        );
    }
    faceBox(face, "plinth", 0, 0.25, width, 0.5, 0.08, metal);
    faceBox(face, "cornice", 0, hy * 2 - 0.15, width + 0.2, 0.3, 0.16, trim);
    if (industrial) {
      for (let bay = 0; bay <= bays; bay++)
        faceBox(
          face,
          `rib-${bay}`,
          (bay - bays / 2) * pitch,
          hy,
          0.12,
          hy * 2 - 0.6,
          0.12,
          trim,
        );
    }
  }
  const frontWidth = (front.axis === 0 ? hz : hx) * 2;
  if (industrial) {
    faceBox(front, "loading-frame", 0, 1.85, 5.2, 3.7, 0.12, trim);
    faceBox(front, "loading-door", 0, 1.8, 4.7, 3.5, 0.05, metal, 0.13);
    for (let i = 1; i <= 6; i++)
      faceBox(front, `shutter-${i}`, 0, i * 0.48, 4.7, 0.06, 0.03, dark, 0.19);
    faceBox(front, "service-door", frontWidth * 0.34, 1.2, 1.3, 2.4, 0.1, dark);
    faceBox(front, "loading-canopy", 0, 4, 6, 0.2, 0.65, metal);
    // Two industrial families: ventilated depot or a plant with paired exhaust stacks.
    box("roof", [x, top + 0.1, z], [hx + 0.12, 0.1, hz + 0.12], metal);
    for (let i = -1; i <= 1; i += 2) {
      box(
        `vent-${i}`,
        [x + i * hx * 0.45, top + 0.45, z],
        [1.2, 0.35, hz * 0.6],
        trim,
      );
      if (h % 2)
        box(
          `stack-${i}`,
          [x + i * hx * 0.45, top + 1.15, z + hz * 0.55],
          [0.4, 1.05, 0.4],
          metal,
        );
    }
  } else {
    faceBox(
      front,
      "entrance-frame",
      -frontWidth * 0.3,
      1.3,
      1.65,
      2.6,
      0.1,
      trim,
    );
    faceBox(
      front,
      "entrance",
      -frontWidth * 0.3,
      1.2,
      1.25,
      2.4,
      0.04,
      dark,
      0.11,
    );
    faceBox(
      front,
      "store-frame",
      frontWidth * 0.12,
      1.6,
      frontWidth * 0.47,
      2,
      0.09,
      trim,
    );
    faceBox(
      front,
      "store-glass",
      frontWidth * 0.12,
      1.6,
      frontWidth * 0.47 - 0.3,
      1.7,
      0.04,
      glass,
      0.1,
    );
    if (shop) {
      const accent = h % 4 === 0 ? "#92594c" : "#597c75";
      faceBox(
        front,
        "awning",
        frontWidth * 0.12,
        2.8,
        frontWidth * 0.53,
        0.18,
        0.7,
        accent,
      );
      faceBox(
        front,
        "sign",
        frontWidth * 0.12,
        3.05,
        frontWidth * 0.42,
        0.35,
        0.12,
        accent,
      );
    }
    box("roof", [x, top + 0.08, z], [hx, 0.08, hz], metal);
    // Raised perimeter parapet gives a readable roofline without inventing walkable space.
    for (const sign of [-1, 1]) {
      box(
        `parapet-x-${sign}`,
        [x + sign * (hx - 0.12), top + 0.3, z],
        [0.12, 0.3, hz],
        trim,
      );
      box(
        `parapet-z-${sign}`,
        [x, top + 0.3, z + sign * (hz - 0.12)],
        [hx, 0.3, 0.12],
        trim,
      );
    }
    box(
      "chimney",
      [x + hx * 0.5, top + 0.75, z + hz * 0.45],
      [0.45, 0.75, 0.45],
      "#8d6860",
    );
    box(
      "chimney-cap",
      [x + hx * 0.5, top + 1.5, z + hz * 0.45],
      [0.55, 0.1, 0.55],
      trim,
    );
  }
  return exposedDetails(p, result);
}

/** Baked neighbours include buildings across streaming chunk boundaries. */
function exposedDetails(shell: Primitive, details: Primitive[]): Primitive[] {
  return details.filter(
    (part) =>
      !(shell.building?.neighbours ?? []).some((bounds) =>
        ([0, 1, 2] as const).every(
          (axis) =>
            part.center[axis] + part.half_extents[axis] >
              bounds.min[axis] + 0.001 &&
            part.center[axis] - part.half_extents[axis] <
              bounds.max[axis] - 0.001,
        ),
      ),
  );
}
