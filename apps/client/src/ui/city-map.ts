import {
  findRoute,
  type LoadedPackage,
  type Primitive,
} from "@port-meridian/content";
import type { CharacterMotor } from "@port-meridian/protocol";
import type { RenderActor } from "../world/replica";
export interface MapView {
  x: number;
  z: number;
  zoom: number;
}
export function mapPoint(
  x: number,
  z: number,
  view: MapView,
  width: number,
  height: number,
): [number, number] {
  const scale = (Math.min(width, height) * view.zoom) / 768;
  return [(x - view.x) * scale + width / 2, (z - view.z) * scale + height / 2];
}
export function zoomMap(view: MapView, factor: number): MapView {
  return { ...view, zoom: Math.max(1, Math.min(8, view.zoom * factor)) };
}
/** Match the world's surface priority independently of street authoring order. */
export function mapDrawOrder(primitives: readonly Primitive[]): Primitive[] {
  const layer = (p: Primitive) => {
    switch (p.role) {
      case "ground":
        return 0;
      case "sidewalk":
        return 1;
      case "road":
        return 2;
      case "marking":
        return 3;
      default:
        return 4;
    }
  };
  return [...primitives].sort((a, b) => layer(a) - layer(b));
}
/** Local presentation only. Markers come from authored zones or current public replicas. */
export class CityMap {
  readonly element = document.createElement("section");
  private readonly canvas = document.createElement("canvas");
  private readonly overview = document.createElement("canvas");
  private readonly description = document.createElement("p");
  private view: MapView = { x: 0, z: 0, zoom: 1 };
  private motor: CharacterMotor | undefined;
  private actors: readonly RenderActor[] = [];
  private target: readonly number[] | null | undefined;
  private courier = false;
  private destinationButton: HTMLButtonElement | undefined;
  private route: readonly (readonly number[])[] = [];
  private readonly abort = new AbortController();
  private drag: { x: number; y: number } | undefined;
  constructor(
    private readonly assets: LoadedPackage,
    close: () => void,
  ) {
    this.element.className = "city-map-panel";
    this.element.hidden = true;
    this.element.setAttribute("role", "dialog");
    this.element.setAttribute("aria-label", "City map");
    this.element.setAttribute("aria-modal", "true");
    const header = document.createElement("header"),
      title = document.createElement("h2"),
      dismiss = document.createElement("button");
    title.textContent = "Port Meridian / City map";
    dismiss.textContent = "Close map";
    dismiss.onclick = close;
    header.append(title, dismiss);
    const legend = document.createElement("p");
    legend.textContent =
      "NORTH ↑ · J Jobs · G Garage · R Repair · Gold: destination · White arrow: you · Dots: nearby cars. Drag or use arrows to pan; scroll or use + / − to zoom. The world continues.";
    this.canvas.setAttribute(
      "aria-label",
      "City map, north up, current location and job destination",
    );
    this.canvas.tabIndex = 0;
    const actions = document.createElement("div");
    actions.className = "map-buttons";
    const button = (label: string, fn: () => void) => {
      const b = document.createElement("button");
      b.textContent = label;
      b.onclick = () => {
        fn();
        this.drawFull();
      };
      actions.append(b);
      return b;
    };
    button("Zoom +", () => {
      this.view = zoomMap(this.view, 1.4);
    });
    button("Zoom −", () => {
      this.view = zoomMap(this.view, 1 / 1.4);
    });
    button("Whole city", () => {
      this.view = { x: 0, z: 0, zoom: 1 };
    });
    button("Find me", () => {
      if (this.motor)
        this.view = {
          x: this.motor.position[0],
          z: this.motor.position[2],
          zoom: 3,
        };
    });
    this.destinationButton = button("Destination", () => {
      if (this.target)
        this.view = { x: this.target[0] ?? 0, z: this.target[2] ?? 0, zoom: 3 };
    });
    for (const zone of assets.manifest.zones.filter((z) =>
      ["terminal", "repair", "garage"].includes(z.kind),
    )) {
      const name = zone.id
        .replace(/^(terminal|repair|garage)_/, "")
        .replaceAll("_", " ");
      button(`${name} ${zone.kind}`, () => {
        this.view = { x: zone.center[0], z: zone.center[2], zoom: 3 };
      });
    }
    this.element.append(header, legend, this.canvas, this.description, actions);
    const options = { signal: this.abort.signal };
    this.element.addEventListener(
      "keydown",
      (e) => {
        if (e.code !== "Tab") return;
        const focusable = [
          ...this.element.querySelectorAll<HTMLElement>(
            "button:not(:disabled), canvas",
          ),
        ];
        const first = focusable[0],
          last = focusable.at(-1);
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last?.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first?.focus();
        }
      },
      options,
    );
    this.canvas.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        this.view = zoomMap(this.view, e.deltaY < 0 ? 1.15 : 1 / 1.15);
        this.drawFull();
      },
      { ...options, passive: false },
    );
    this.canvas.addEventListener(
      "keydown",
      (e) => {
        const directions: Record<string, [number, number]> = {
          ArrowUp: [0, -1],
          ArrowDown: [0, 1],
          ArrowLeft: [-1, 0],
          ArrowRight: [1, 0],
        };
        const d = directions[e.code];
        if (d) {
          e.preventDefault();
          this.view.x = Math.max(
            -384,
            Math.min(384, this.view.x + (d[0] * 40) / this.view.zoom),
          );
          this.view.z = Math.max(
            -384,
            Math.min(384, this.view.z + (d[1] * 40) / this.view.zoom),
          );
          this.drawFull();
        }
      },
      options,
    );
    this.canvas.addEventListener(
      "pointerdown",
      (e) => {
        this.drag = { x: e.clientX, y: e.clientY };
        this.canvas.setPointerCapture(e.pointerId);
      },
      options,
    );
    this.canvas.addEventListener(
      "pointermove",
      (e) => {
        if (!this.drag) return;
        const scale =
          (Math.min(this.canvas.clientWidth, this.canvas.clientHeight) *
            this.view.zoom) /
          768;
        this.view.x -= (e.clientX - this.drag.x) / scale;
        this.view.z -= (e.clientY - this.drag.y) / scale;
        this.view.x = Math.max(-384, Math.min(384, this.view.x));
        this.view.z = Math.max(-384, Math.min(384, this.view.z));
        this.drag = { x: e.clientX, y: e.clientY };
        this.drawFull();
      },
      options,
    );
    this.canvas.addEventListener(
      "pointerup",
      () => {
        this.drag = undefined;
      },
      options,
    );
    this.canvas.addEventListener(
      "pointercancel",
      () => {
        this.drag = undefined;
      },
      options,
    );
    this.overview.width = 1024;
    this.overview.height = 1024;
    const ctx = this.overview.getContext("2d");
    if (ctx) {
      ctx.fillStyle = "#18343c";
      ctx.fillRect(0, 0, 1024, 1024);
      for (const p of mapDrawOrder(assets.minimap.primitives)) {
        ctx.fillStyle =
          p.role === "building"
            ? "#b9a38d"
            : p.role === "road"
              ? "#263c45"
              : p.role === "marking"
                ? "#758681"
                : p.color;
        ctx.fillRect(
          ((p.center[0] - p.half_extents[0] + 384) * 1024) / 768,
          ((p.center[2] - p.half_extents[2] + 384) * 1024) / 768,
          (p.half_extents[0] * 2 * 1024) / 768,
          (p.half_extents[2] * 2 * 1024) / 768,
        );
      }
    }
    try {
      this.route = findRoute(
        assets.manifest.road_graph,
        "terminal_access",
        "market_access",
      );
    } catch {
      /* authored fallback map still works */
    }
  }
  get open(): boolean {
    return !this.element.hidden;
  }
  show(): void {
    this.element.hidden = false;
    this.drawFull();
    this.element.querySelector("button")?.focus();
  }
  hide(): void {
    this.element.hidden = true;
    this.drag = undefined;
  }
  update(
    motor: CharacterMotor,
    actors: readonly RenderActor[],
    target: readonly number[] | null | undefined,
    courier: boolean,
    minimap: HTMLCanvasElement,
  ): void {
    this.courier = courier;
    if (this.destinationButton) this.destinationButton.disabled = !target;
    this.motor = motor;
    this.actors = actors;
    this.target = target;
    this.draw(
      minimap,
      { x: motor.position[0], z: motor.position[2], zoom: 3.4 },
      false,
      courier,
    );
    if (this.open) this.drawFull(courier);
  }
  private drawFull(courier = this.courier): void {
    const box = this.canvas.getBoundingClientRect();
    this.canvas.width = Math.max(320, Math.round(box.width));
    this.canvas.height = Math.max(260, Math.round(box.height));
    this.draw(this.canvas, this.view, true, courier);
    this.description.textContent = this.motor
      ? `You: ${Math.round(this.motor.position[0])}, ${Math.round(this.motor.position[2])}${this.target ? ` · Destination ${Math.round(Math.hypot((this.target[0] ?? 0) - this.motor.position[0], (this.target[2] ?? 0) - this.motor.position[2]))} m away` : " · No active job"}`
      : "Join the city to show your location.";
  }
  private draw(
    canvas: HTMLCanvasElement,
    view: MapView,
    full: boolean,
    courier: boolean,
  ): void {
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const w = canvas.width,
      h = canvas.height,
      scale = (Math.min(w, h) * view.zoom) / 768;
    ctx.fillStyle = "#10252e";
    ctx.fillRect(0, 0, w, h);
    const [left, top] = mapPoint(-384, -384, view, w, h);
    ctx.drawImage(this.overview, left, top, 768 * scale, 768 * scale);
    const point = (x: number, z: number) => mapPoint(x, z, view, w, h);
    if (courier && this.target) {
      ctx.strokeStyle = "#f4c563";
      ctx.lineWidth = full ? 4 : 2;
      ctx.beginPath();
      this.route.forEach((p, i) => {
        const [x, y] = point(p[0] ?? 0, p[2] ?? 0);
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      });
      ctx.stroke();
    }
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    if (full) {
      ctx.font = "600 13px system-ui";
      for (const d of this.assets.manifest.districts) {
        const [x, y] = point(
          (d.rect.min[0] + d.rect.max[0]) / 2,
          (d.rect.min[1] + d.rect.max[1]) / 2,
        );
        ctx.fillStyle = "#10252ed9";
        ctx.fillRect(x - 62, y - 10, 124, 20);
        ctx.fillStyle = "#f0e8d9";
        ctx.fillText(d.name, x, y);
      }
    }
    for (const zone of this.assets.manifest.zones.filter((z) =>
      ["terminal", "garage", "repair"].includes(z.kind),
    )) {
      const [x, y] = point(zone.center[0], zone.center[2]);
      ctx.fillStyle =
        zone.kind === "terminal"
          ? "#66dfcb"
          : zone.kind === "garage"
            ? "#a9c9ff"
            : "#b8ee88";
      ctx.fillRect(x - 7, y - 7, 14, 14);
      ctx.fillStyle = "#10252e";
      ctx.font = "bold 10px system-ui";
      ctx.fillText(
        zone.kind === "terminal" ? "J" : zone.kind === "garage" ? "G" : "R",
        x,
        y,
      );
    }
    if (this.motor) {
      for (const actor of this.actors) {
        if (
          actor.state.kind !== "vehicle" ||
          actor.stale ||
          Math.hypot(
            actor.motor.position[0] - this.motor.position[0],
            actor.motor.position[2] - this.motor.position[2],
          ) > 96
        )
          continue;
        const [x, y] = point(actor.motor.position[0], actor.motor.position[2]);
        ctx.fillStyle =
          actor.descriptor.entity_kind === "police" ? "#91baff" : "#d1d8d5";
        ctx.fillRect(x - 2, y - 2, 4, 4);
      }
    }
    if (this.target) {
      const [x, y] = point(this.target[0] ?? 0, this.target[2] ?? 0);
      ctx.strokeStyle = "#f4c563";
      ctx.lineWidth = 3;
      ctx.beginPath();
      ctx.arc(x, y, full ? 9 : 6, 0, Math.PI * 2);
      ctx.stroke();
      ctx.fillStyle = "#f4c563";
      ctx.fillRect(x - 2, y - 2, 4, 4);
    }
    if (this.motor) {
      const [x, y] = point(this.motor.position[0], this.motor.position[2]);
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(-this.motor.yaw);
      ctx.fillStyle = "#fff7e5";
      ctx.strokeStyle = "#10252e";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(0, -9);
      ctx.lineTo(-6, 6);
      ctx.lineTo(0, 3);
      ctx.lineTo(6, 6);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
      ctx.restore();
    }
    ctx.font = "bold 13px system-ui";
    ctx.textAlign = "left";
    ctx.fillStyle = "#fff7e5";
    ctx.fillText("N ↑", 8, 14);
    canvas.setAttribute(
      "aria-label",
      full
        ? "City map, north up"
        : "Local minimap, north up. Open city map for locations.",
    );
  }
  dispose(): void {
    this.abort.abort();
    this.element.remove();
  }
}
