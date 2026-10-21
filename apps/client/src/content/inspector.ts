import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { findRoute, loadPackage } from "@port-meridian/content";
import type {
  LoadedPackage,
  Primitive,
  RoadGraph,
  Vec3,
} from "@port-meridian/content";

/** Standalone baked-content viewer. No networking, world entities or player movement. */
export function mountMapInspector(root: HTMLElement): () => void {
  const previousTitle = document.title;
  document.title = "Port Meridian — greybox inspector";
  document.body.classList.add("map-mode");
  root.className = "map-inspector";
  const header = document.createElement("aside");
  header.className = "map-hud";
  const eyebrow = document.createElement("p");
  eyebrow.className = "stage";
  eyebrow.textContent = "T04 · 256 m content slice";
  const title = document.createElement("h1");
  title.textContent = "Old Quay / Market Grid";
  const status = document.createElement("p");
  status.setAttribute("role", "status");
  status.textContent = "Verifying baked content…";
  const description = document.createElement("p");
  description.textContent =
    "Drag to orbit · right-drag to pan · scroll to zoom. Cyan: collision · amber: reservations · blue: vehicle lanes · green: sidewalks · white: courier route.";
  const actions = document.createElement("div");
  actions.className = "map-actions";
  const details = document.createElement("p");
  details.className = "map-details";
  const back = document.createElement("a");
  back.href = "/";
  back.textContent = "Back to game";
  const panel = document.createElement("details");
  panel.open = innerWidth >= 900;
  const summary = document.createElement("summary");
  summary.textContent = "Layers and inspection controls";
  panel.append(summary, description, actions, details, back);
  header.append(eyebrow, title, status, panel);
  root.replaceChildren(header);
  const labels = document.createElement("div");
  labels.className = "map-labels";
  root.append(labels);
  const abort = new AbortController();
  let disposed = false;
  let renderer: THREE.WebGLRenderer | undefined;
  let controls: OrbitControls | undefined;
  let frame = 0;
  const geometries = new Set<THREE.BufferGeometry>();
  const materials = new Set<THREE.Material>();
  const listeners: (() => void)[] = [];
  const buttons: HTMLButtonElement[] = [];
  const button = (text: string, handler: () => void) => {
    const el = document.createElement("button");
    el.textContent = text;
    el.onclick = handler;
    actions.append(el);
    buttons.push(el);
    return el;
  };
  const checkbox = (
    name: string,
    initial: boolean,
    handler: (checked: boolean) => void,
  ) => {
    const label = document.createElement("label"),
      input = document.createElement("input");
    input.type = "checkbox";
    input.checked = initial;
    input.onchange = () => handler(input.checked);
    label.append(input, ` ${name}`);
    actions.append(label);
    listeners.push(() => {
      input.onchange = null;
    });
  };
  const cleanupGraphics = () => {
    cancelAnimationFrame(frame);
    controls?.dispose();
    for (const geometry of geometries) geometry.dispose();
    for (const material of materials) material.dispose();
    renderer?.dispose();
    renderer?.domElement.remove();
  };
  function show(data: LoadedPackage): void {
    if (disposed) return;
    const { manifest, collision } = data;
    renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    renderer.setSize(innerWidth, innerHeight);
    renderer.setClearColor("#172b37");
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.domElement.setAttribute(
      "aria-label",
      "3D greybox map, inspect with orbit controls",
    );
    root.prepend(renderer.domElement);
    const scene = new THREE.Scene();
    scene.background = new THREE.Color("#172b37");
    const camera = new THREE.OrthographicCamera(-200, 200, 200, -200, 1, 2000);
    const fit = () => {
      const aspect = innerWidth / innerHeight;
      const width = 400 * Math.max(1, aspect),
        height = 400 * Math.max(1, 1 / aspect);
      camera.left = -width / 2;
      camera.right = width / 2;
      camera.top = height / 2;
      camera.bottom = -height / 2;
      camera.updateProjectionMatrix();
    };
    fit();
    camera.position.set(-330, 270, 380);
    controls = new OrbitControls(camera, renderer.domElement);
    controls.target.set(-128, 0, 128);
    controls.enableDamping = true;
    controls.minZoom = 0.4;
    controls.maxZoom = 20;
    controls.minDistance = 10;
    controls.maxDistance = 650;
    controls.maxPolarAngle = Math.PI / 2 - 0.03;
    controls.update();
    scene.add(new THREE.HemisphereLight("#eaf8ff", "#364434", 2.5));
    const sunlight = new THREE.DirectionalLight("#fff1cf", 3.0);
    sunlight.position.set(-60, 180, 90);
    scene.add(sunlight);
    const cube = new THREE.BoxGeometry(1, 1, 1);
    geometries.add(cube);
    const outlines = new THREE.EdgesGeometry(cube);
    geometries.add(outlines);
    const palette = new Map<string, THREE.MeshStandardMaterial>();
    const material = (color: string, layer = 0) => {
      const key = `${color}:${layer}`;
      let result = palette.get(key);
      if (!result) {
        result = new THREE.MeshStandardMaterial({
          color,
          roughness: 0.9,
          polygonOffset: layer > 0,
          polygonOffsetFactor: -layer,
          polygonOffsetUnits: -layer,
        });
        palette.set(key, result);
        materials.add(result);
      }
      return result;
    };
    const solids = new THREE.Group();
    scene.add(solids);
    function box(p: Primitive, parent: THREE.Group): void {
      const layer =
        p.role === "marking"
          ? 3
          : p.role === "road"
            ? 2
            : p.role === "sidewalk"
              ? 1
              : 0;
      const mesh = new THREE.Mesh(cube, material(p.color, layer));
      mesh.position.set(...p.center);
      mesh.scale.set(...(p.half_extents.map((n) => n * 2) as Vec3));
      parent.add(mesh);
    }
    for (const chunk of data.chunks)
      for (const p of chunk.primitives) box(p, solids);
    for (const p of manifest.parking) {
      const template = manifest.vehicle_templates.find(
        (t) => t.id === p.template_id,
      );
      const visual = data.vehicleVisuals.find(
        (v) => v.id === template?.visual_asset_id,
      );
      if (!template || !visual)
        throw new Error("Missing compact preview template/visual");
      const group = new THREE.Group();
      group.position.set(...p.position);
      group.rotation.y = p.yaw;
      for (const primitive of visual.primitives) box(primitive, group);
      scene.add(group);
    }
    const wire = (color: string) => {
      const m = new THREE.LineBasicMaterial({
        color,
        depthTest: false,
        transparent: true,
        opacity: 0.8,
      });
      materials.add(m);
      return m;
    };
    const collisionGroup = new THREE.Group();
    collisionGroup.visible = true;
    scene.add(collisionGroup);
    const collisionMaterial = wire("#4de4ef");
    for (const c of collision.colliders.filter((c) => c.role !== "ground")) {
      const edges = new THREE.LineSegments(outlines, collisionMaterial);
      edges.position.set(...c.center);
      edges.scale.set(...(c.half_extents.map((n) => n * 2 + 0.02) as Vec3));
      edges.renderOrder = 4;
      collisionGroup.add(edges);
    }
    const reservationGroup = new THREE.Group();
    scene.add(reservationGroup);
    reservationGroup.visible = false;
    const amber = wire("#ffbf5a");
    for (const r of data.debug.reservations) {
      const edges = new THREE.LineSegments(outlines, amber);
      edges.position.set(
        (r.rect.min[0] + r.rect.max[0]) / 2,
        0.4,
        (r.rect.min[1] + r.rect.max[1]) / 2,
      );
      edges.scale.set(
        r.rect.max[0] - r.rect.min[0],
        0.8,
        r.rect.max[1] - r.rect.min[1],
      );
      edges.renderOrder = 5;
      reservationGroup.add(edges);
    }
    const graphLines = (graph: RoadGraph, color: string): THREE.Group => {
      const group = new THREE.Group(),
        coords: number[] = [];
      const nodes = new Map(graph.nodes.map((n) => [n.id, n.position]));
      for (const e of graph.edges) {
        const a = nodes.get(e.from),
          b = nodes.get(e.to);
        if (!a || !b) throw new Error("Missing graph node");
        coords.push(a[0], 0.35, a[2], b[0], 0.35, b[2]);
        // Arrowheads distinguish one-way lanes; they are overlay data, never collision.
        const dx = b[0] - a[0],
          dz = b[2] - a[2],
          length = Math.hypot(dx, dz);
        const x = (a[0] + b[0]) / 2,
          z = (a[2] + b[2]) / 2,
          ux = dx / length,
          uz = dz / length;
        coords.push(
          x,
          0.35,
          z,
          x - ux * 2 - uz,
          0.35,
          z - uz * 2 + ux,
          x,
          0.35,
          z,
          x - ux * 2 + uz,
          0.35,
          z - uz * 2 - ux,
        );
      }
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute(
        "position",
        new THREE.Float32BufferAttribute(coords, 3),
      );
      geometries.add(geometry);
      const lines = new THREE.LineSegments(geometry, wire(color));
      lines.renderOrder = 6;
      group.add(lines);
      scene.add(group);
      group.visible = false;
      return group;
    };
    const roadGraph = graphLines(manifest.road_graph, "#70b8ff"),
      pedestrianGraph = graphLines(manifest.pedestrian_graph, "#7aee9b");
    const route = findRoute(
      manifest.road_graph,
      "terminal_access",
      "market_access",
    );
    const routeGeometry = new THREE.BufferGeometry().setFromPoints(
      route.map((p) => new THREE.Vector3(p[0], 0.65, p[2])),
    );
    geometries.add(routeGeometry);
    const routeLine = new THREE.Line(routeGeometry, wire("#ffffff"));
    routeLine.renderOrder = 7;
    scene.add(routeLine);
    const landmarks = [
      ...manifest.spawns.map((s) => ({
        text: s.id,
        position: s.position,
        color: "#67eddb",
      })),
      ...manifest.zones.map((z) => ({
        text: z.id,
        position: z.center,
        color: z.kind === "delivery" ? "#f9dc7b" : "#c4dcfa",
      })),
      ...manifest.parking.map((p) => ({
        text: p.id,
        position: p.position,
        color: "#a5dcf1",
      })),
    ].map((label) => {
      const el = document.createElement("span");
      el.textContent = label.text;
      el.style.color = label.color;
      labels.append(el);
      return {
        el,
        position: new THREE.Vector3(label.position[0], 4, label.position[2]),
      };
    });
    checkbox("Collision", true, (v) => {
      collisionGroup.visible = v;
    });
    checkbox("Reservations", false, (v) => {
      reservationGroup.visible = v;
    });
    checkbox("Vehicle graph", false, (v) => {
      roadGraph.visible = v;
    });
    checkbox("Sidewalk graph", false, (v) => {
      pedestrianGraph.visible = v;
    });
    checkbox("Courier route", true, (v) => {
      routeLine.visible = v;
    });
    checkbox("Labels", true, (v) => {
      labels.hidden = !v;
    });
    button("Isometric", () => {
      camera.zoom = 1;
      camera.updateProjectionMatrix();
      camera.position.set(-330, 270, 380);
      controls?.target.set(-128, 0, 128);
      controls?.update();
    });
    button("Top view", () => {
      camera.zoom = 1;
      camera.updateProjectionMatrix();
      camera.position.set(-128, 340, 128.1);
      controls?.target.set(-128, 0, 128);
      controls?.update();
    });
    status.textContent = `Verified · ${manifest.chunks.length} chunks · ${collision.colliders.length} colliders · no game server required`;
    details.textContent = `Package ${manifest.content_hash.slice(0, 16)}… · sim ${manifest.sim_config_hash.slice(0, 12)}… · 1 unit = 1 m · north = −Z. Cars show parking template dimensions; no physics is running.`;
    const resize = () => {
      if (!renderer) return;
      fit();
      renderer.setSize(innerWidth, innerHeight);
    };
    window.addEventListener("resize", resize);
    listeners.push(() => window.removeEventListener("resize", resize));
    const lost = (event: Event) => {
      event.preventDefault();
      cancelAnimationFrame(frame);
      status.textContent =
        "Graphics context lost. Reload the inspector to recover.";
    };
    renderer.domElement.addEventListener("webglcontextlost", lost);
    listeners.push(() =>
      renderer?.domElement.removeEventListener("webglcontextlost", lost),
    );
    const tick = () => {
      if (disposed || !renderer) return;
      controls?.update();
      const occupied: { x: number; y: number; w: number }[] = [];
      for (const label of landmarks) {
        const p = label.position.clone().project(camera);
        const x = ((p.x + 1) * innerWidth) / 2,
          y = ((1 - p.y) * innerHeight) / 2,
          w = label.el.textContent?.length
            ? label.el.textContent.length * 6.7 + 8
            : 0;
        const overlaps = occupied.some(
          (r) =>
            Math.abs(x - r.x) < (w + r.w) / 2 + 4 && Math.abs(y - r.y) < 22,
        );
        label.el.hidden = p.z < -1 || p.z > 1 || overlaps;
        label.el.style.transform = `translate(${x}px,${y}px) translate(-50%,-100%)`;
        if (!label.el.hidden) occupied.push({ x, y, w });
      }
      renderer.render(scene, camera);
      frame = requestAnimationFrame(tick);
    };
    tick();
  }
  void loadPackage(new URL("/content/slice.json", location.href), abort.signal)
    .then(show)
    .catch((error: unknown) => {
      if (disposed) return;
      cleanupGraphics();
      status.textContent = `Content inspection failed: ${error instanceof Error ? error.message : "unknown error"}`;
      button("Retry", () => location.reload());
    });
  return () => {
    disposed = true;
    abort.abort();
    cleanupGraphics();
    for (const off of listeners) off();
    for (const b of buttons) b.onclick = null;
    document.title = previousTitle;
    document.body.classList.remove("map-mode");
    root.className = "";
  };
}
