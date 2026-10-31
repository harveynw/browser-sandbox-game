import type { ArchitectureStyle, Primitive } from "@port-meridian/content";

type Face = { axis: 0 | 2; sign: number; distance: number };
type Box = (
  label: string,
  center: Primitive["center"],
  half: Primitive["half_extents"],
  color: string,
) => void;
type FaceBox = (
  face: Face,
  label: string,
  u: number,
  v: number,
  width: number,
  height: number,
  depth: number,
  color: string,
  offset?: number,
) => void;
interface Context {
  style: ArchitectureStyle;
  h: number;
  x: number;
  z: number;
  top: number;
  hx: number;
  hy: number;
  hz: number;
  floors: number;
  floorHeight: number;
  faces: Face[];
  front: Face;
  box: Box;
  faceBox: FaceBox;
}

/** District-specific modules share the street-facing coordinate system and cube batches. */
export function neighbourhoodDetails(c: Context): void {
  const {
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
    faceBox: f,
  } = c;
  const civic = style === "civic",
    residential = style === "residential",
    market = style === "market",
    docks = style === "dockside";
  const trim = civic
    ? "#ded9c5"
    : residential
      ? "#ccd0c5"
      : market
        ? "#dac7a3"
        : "#bcc6c2";
  const glass = civic
    ? "#64818a"
    : residential
      ? "#718f99"
      : market
        ? "#769594"
        : "#91a9ac";
  const dark = "#34464b",
    metal = "#525b5a";
  const accent = market
    ? h % 2
      ? "#a55448"
      : "#4e8076"
    : docks
      ? "#d1a458"
      : "#71847a";
  const frontWidth = (front.axis === 0 ? hz : hx) * 2;
  for (const face of faces) {
    const width = (face.axis === 0 ? hz : hx) * 2;
    const bays = Math.max(
      2,
      Math.floor((width - 1) / (residential ? 4.5 : 3.6)),
    );
    const pitch = (width - 1) / bays;
    if (docks) {
      // Horizontal cladding distinguishes docks from Ironworks' tall vertical ribs.
      for (let row = 1; row < Math.floor((hy * 2) / 0.75); row++)
        f(face, `cladding-${row}`, 0, row * 0.75, width, 0.06, 0.06, trim);
      f(face, "clerestory", 0, hy * 2 - 0.85, width - 2, 0.65, 0.1, glass);
      for (const sign of [-1, 1])
        f(
          face,
          `corner-${sign}`,
          sign * (width / 2 - 0.15),
          hy,
          0.3,
          hy * 2,
          0.15,
          trim,
        );
    } else {
      for (let floor = 0; floor < floors; floor++) {
        if (face === front && floor === 0) continue;
        for (let bay = 0; bay < bays; bay++) {
          const u = (bay + 0.5 - bays / 2) * pitch;
          const v = floor * floorHeight + floorHeight * 0.56;
          const w = civic ? 1.65 : residential ? pitch - 0.75 : 1.4;
          const wh = civic ? 2.35 : 1.6;
          f(face, `frame-${floor}-${bay}`, u, v, w + 0.2, wh + 0.2, 0.08, trim);
          f(face, `pane-${floor}-${bay}`, u, v, w, wh, 0.04, glass, 0.09);
          // Alternating balcony stacks avoid a uniformly extruded apartment slab.
          if (residential && floor > 0 && (bay + h) % 2 === 0) {
            const balconyY = floor * floorHeight + 0.12;
            f(
              face,
              `balcony-slab-${floor}-${bay}`,
              u,
              balconyY,
              pitch - 0.3,
              0.18,
              0.65,
              trim,
            );
            f(
              face,
              `balcony-rail-${floor}-${bay}`,
              u,
              balconyY + 0.55,
              pitch - 0.3,
              0.85,
              0.08,
              accent,
              0.57,
            );
          }
        }
        if (!civic && floor > 0)
          f(
            face,
            `floor-${floor}`,
            0,
            floor * floorHeight,
            width + 0.1,
            0.16,
            0.13,
            trim,
          );
      }
      if (civic) {
        for (let bay = 0; bay <= bays; bay++)
          f(
            face,
            `pilaster-${bay}`,
            (bay - bays / 2) * pitch,
            hy,
            0.28,
            hy * 2 - 0.5,
            0.2,
            trim,
          );
      }
    }
    f(face, "plinth", 0, 0.3, width, 0.6, 0.12, metal);
    f(
      face,
      "cornice",
      0,
      hy * 2 - 0.15,
      width + 0.2,
      civic ? 0.45 : 0.25,
      0.2,
      trim,
    );
  }
  if (docks) {
    for (const side of [-1, 1]) {
      const u = side * frontWidth * 0.25;
      f(front, `bay-frame-${side}`, u, 1.65, 4.5, 3.3, 0.13, trim);
      f(front, `bay-door-${side}`, u, 1.6, 4.1, 3.2, 0.05, metal, 0.14);
      for (let row = 1; row < 6; row++)
        f(
          front,
          `shutter-${side}-${row}`,
          u,
          row * 0.5,
          4.1,
          0.06,
          0.03,
          dark,
          0.2,
        );
      f(front, `bay-marker-${side}`, u, 3.55, 1.2, 0.25, 0.1, accent);
    }
    f(front, "loading-canopy", 0, 3.85, frontWidth - 1, 0.2, 0.7, metal);
    // Shallow stepped ridge stays within the decorative roof-height allowance.
    box("roof", [x, top + 0.12, z], [hx + 0.15, 0.12, hz + 0.15], metal);
    box("roof-ridge", [x, top + 0.4, z], [hx * 0.6, 0.2, hz], trim);
  } else if (civic) {
    f(front, "entry-glass", 0, 1.65, 3.8, 3.3, 0.1, glass);
    for (const side of [-1, 1]) {
      f(front, `entry-column-${side}`, side * 2.4, 1.8, 0.5, 3.6, 0.45, trim);
      f(
        front,
        `entry-capital-${side}`,
        side * 2.4,
        3.55,
        0.75,
        0.25,
        0.55,
        trim,
      );
    }
    f(front, "portico", 0, 3.8, 6.1, 0.3, 0.7, trim);
    box("roof-crown", [x, top + 0.3, z], [hx + 0.15, 0.3, hz + 0.15], trim);
    box("roof-pavilion", [x, top + 1.1, z], [hx * 0.48, 0.5, hz * 0.48], metal);
    box("roof-cap", [x, top + 1.65, z], [hx * 0.5, 0.1, hz * 0.5], trim);
  } else if (market) {
    const shops = Math.max(2, Math.floor(frontWidth / 5));
    const pitch = (frontWidth - 0.8) / shops;
    for (let i = 0; i < shops; i++) {
      const u = (i + 0.5 - shops / 2) * pitch;
      f(front, `shop-frame-${i}`, u, 1.35, pitch - 0.15, 2.7, 0.1, trim);
      f(front, `shop-glass-${i}`, u, 1.3, pitch - 0.5, 2.5, 0.05, glass, 0.11);
      f(
        front,
        `shop-door-${i}`,
        u - pitch * 0.24,
        1.2,
        0.85,
        2.4,
        0.04,
        dark,
        0.17,
      );
      f(front, `shop-awning-${i}`, u, 2.7, pitch - 0.1, 0.16, 0.7, accent);
      f(front, `shop-sign-${i}`, u, 2.98, pitch - 0.5, 0.28, 0.12, accent);
    }
    box("roof", [x, top + 0.15, z], [hx + 0.1, 0.15, hz + 0.1], trim);
    box("roof-plant", [x - hx * 0.45, top + 0.6, z], [1.2, 0.45, 1.4], metal);
    box(
      "roof-tank",
      [x + hx * 0.45, top + 0.8, z + hz * 0.4],
      [0.8, 0.65, 0.8],
      accent,
    );
  } else {
    f(front, "lobby", 0, 1.35, 4.2, 2.7, 0.1, glass);
    f(front, "lobby-door", 0, 1.2, 1.4, 2.4, 0.06, dark, 0.11);
    f(front, "lobby-canopy", 0, 2.9, 5.2, 0.18, 0.7, trim);
    box("roof", [x, top + 0.15, z], [hx + 0.1, 0.15, hz + 0.1], trim);
    box("lift-house", [x, top + 0.85, z + hz * 0.4], [1.4, 0.7, 1.6], metal);
    // A slim mast adds variation without changing the collision-bearing shell.
    if (h % 2)
      box("aerial", [x, top + 1.8, z + hz * 0.4], [0.06, 0.65, 0.06], metal);
  }
}
