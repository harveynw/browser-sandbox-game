import type { RoadGraph, Vec3 } from "./generated/types";
/** Distance/speed Dijkstra on directed baked edges; no straight-line building shortcuts. */
export function findRoute(
  graph: RoadGraph,
  fromAccess: string,
  toAccess: string,
): Vec3[] {
  const start = graph.accesses.find((a) => a.access_id === fromAccess)?.node_id;
  const goal = graph.accesses.find((a) => a.access_id === toAccess)?.node_id;
  if (start === undefined || goal === undefined)
    throw new Error("Missing route access");
  const positions = new Map(graph.nodes.map((n) => [n.id, n.position]));
  const distances = new Map([[start, 0]]);
  const previous = new Map<number, number>();
  const open = new Set([start]);
  while (open.size) {
    let current: number | undefined;
    for (const id of open)
      if (
        current === undefined ||
        (distances.get(id) ?? Infinity) < (distances.get(current) ?? Infinity)
      )
        current = id;
    if (current === undefined) break;
    open.delete(current);
    if (current === goal) break;
    for (const edge of graph.edges.filter((e) => e.from === current)) {
      const a = positions.get(edge.from),
        b = positions.get(edge.to);
      if (!a || !b || edge.speed_limit <= 0)
        throw new Error("Invalid route edge");
      const cost =
        (distances.get(current) ?? Infinity) +
        Math.hypot(b[0] - a[0], b[2] - a[2]) / edge.speed_limit;
      if (cost < (distances.get(edge.to) ?? Infinity)) {
        distances.set(edge.to, cost);
        previous.set(edge.to, current);
        open.add(edge.to);
      }
    }
  }
  if (!distances.has(goal)) throw new Error("No connected route");
  const route: Vec3[] = [];
  let current = goal;
  for (;;) {
    const point = positions.get(current);
    if (!point) throw new Error("Missing route node");
    route.push(point);
    if (current === start) break;
    const parent = previous.get(current);
    if (parent === undefined) throw new Error("Broken route");
    current = parent;
  }
  return route.reverse();
}

/** Route to the assigned courier zone, including its off-road delivery entrance. */
export function courierRoute(
  manifest: Pick<
    import("./generated/types").WorldManifest,
    "zones" | "road_graph"
  >,
  origin: readonly number[],
  target: readonly number[],
): Vec3[] {
  const distance = (a: readonly number[], b: readonly number[]) =>
    Math.hypot((a[0] ?? 0) - (b[0] ?? 0), (a[2] ?? 0) - (b[2] ?? 0));
  const destination = manifest.zones.find(
    (zone) => zone.kind === "delivery" && distance(zone.center, target) < 0.1,
  );
  const terminal = manifest.zones
    .filter((zone) => zone.kind === "terminal")
    .sort((a, b) => distance(a.center, origin) - distance(b.center, origin))[0];
  if (!destination || !terminal) return [];
  const route = findRoute(
    manifest.road_graph,
    terminal.access_id,
    destination.access_id,
  );
  const end: Vec3 = [target[0] ?? 0, target[1] ?? 0, target[2] ?? 0];
  if (!route.length || distance(route[route.length - 1] ?? [], end) > 0.01)
    route.push(end);
  return route;
}
