import * as THREE from "three";
import type { Primitive } from "@port-meridian/content";

export function isSurroundings(p: Primitive): boolean {
  return p.id.startsWith("surroundings_");
}
export function surroundingsShape(
  p: Primitive,
): "peak" | "snowyRock" | "hill" | null {
  if (/^surroundings_peak-\d+-1-/.test(p.id)) return "snowyRock";
  if (/^surroundings_(peak|snow)-/.test(p.id)) return "peak";
  if (p.id.startsWith("surroundings_hill-")) return "hill";
  return null;
}
export function surroundingsGeometries() {
  // Hard face normals give each broad triangle its own lighting, rather than
  // smoothing these deliberately low-poly ridges into circular cones.
  const indexed = new THREE.ConeGeometry(0.5, 1, 5);
  const peak = indexed.toNonIndexed();
  indexed.dispose();
  peak.computeVertexNormals();
  // The top radius meets the 30% snow cone exactly. Open ends avoid a
  // duplicate horizontal surface at the join.
  const rockIndexed = new THREE.CylinderGeometry(0.15, 0.5, 1, 5, 1, true);
  const snowyRock = rockIndexed.toNonIndexed();
  rockIndexed.dispose();
  snowyRock.computeVertexNormals();
  return { peak, snowyRock, hill: new THREE.IcosahedronGeometry(0.5, 0) };
}
