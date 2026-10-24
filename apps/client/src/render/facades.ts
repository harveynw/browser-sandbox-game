import type { Primitive } from "@port-meridian/content";
/** Eight original modular facade patterns, instanced from the unchanged verified footprints. */
export function facadeDetails(buildings: readonly Primitive[]): Primitive[] {
  return buildings
    .filter((p) => p.role === "building")
    .flatMap((p) => {
      const style =
        [...p.id].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 0) % 8;
      const [x, y, z] = p.center,
        [hx, hy, hz] = p.half_extents;
      const piece = (
        id: string,
        center: [number, number, number],
        half_extents: [number, number, number],
        color: string,
      ): Primitive => ({
        id: `${p.id}-${id}`,
        role: "building",
        center,
        half_extents,
        color,
        collider_id: null,
      });
      const trim = "#dec8a6",
        glass = "#405e68";
      const result = [
        piece(
          "roof",
          [x, y + hy + 0.12, z],
          [hx + 0.12, 0.12, hz + 0.12],
          trim,
        ),
        piece("plinth", [x, 0.35, z], [hx + 0.04, 0.25, hz + 0.04], trim),
      ];
      const count = style % 2 === 0 ? 2 : 3;
      for (let row = 0; row < count; row++) {
        const height = 2 + ((hy * 2 - 3) * (row + 0.5)) / count;
        const slit = style >= 4 ? 0.45 : 0.7;
        result.push(
          piece(
            `front-${row}`,
            [x, height, z - hz - 0.035],
            [hx * 0.72, slit, 0.03],
            glass,
          ),
          piece(
            `side-${row}`,
            [x + hx + 0.035, height, z],
            [0.03, slit, hz * 0.72],
            glass,
          ),
        );
      }
      if (style % 4 === 0)
        result.push(
          piece("canopy", [x, 2.4, z - hz - 0.4], [hx * 0.7, 0.13, 0.65], trim),
        );
      if (style % 4 === 1)
        result.push(
          piece("band", [x, hy, z], [hx + 0.08, 0.14, hz + 0.08], trim),
        );
      if (style % 4 === 2)
        result.push(
          piece(
            "pilaster",
            [x - hx + 0.25, y, z - hz - 0.06],
            [0.28, hy, 0.1],
            trim,
          ),
        );
      if (style % 4 === 3)
        result.push(
          piece(
            "cornice",
            [x, y + hy - 0.5, z],
            [hx + 0.15, 0.15, hz + 0.15],
            trim,
          ),
        );
      return result;
    });
}
