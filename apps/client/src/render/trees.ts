import * as THREE from "three";
import type { Primitive } from "@port-meridian/content";

export function treeShape(p: Primitive): "box" | "trunk" | "crown" | "conifer" {
  if (p.role !== "vegetation") return "box";
  if (p.id.endsWith("_trunk")) return "trunk";
  return /^tree_(?:park|boulevard)_\d+_\d+_2_/.test(p.id) ? "conifer" : "crown";
}
/** Unit envelopes match baked extents; all trees reuse these three meshes. */
export function treeGeometries() {
  return {
    trunk: new THREE.CylinderGeometry(0.38, 0.5, 1, 6),
    crown: new THREE.IcosahedronGeometry(0.5, 1),
    conifer: new THREE.ConeGeometry(0.5, 1, 7),
  };
}
