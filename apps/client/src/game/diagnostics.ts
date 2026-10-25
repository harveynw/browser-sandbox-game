import { TimingHistogram } from "./performance";
import type { GameSession } from "../net/session";
import type { RenderActor } from "../world/replica";
import type { CharacterMotor } from "@port-meridian/protocol";
import type { GameRenderer } from "../render/renderer";

type Sample = {
  now: number;
  connection: string;
  net: ReturnType<GameSession["metrics"]> | undefined;
  streaming: GameRenderer["streamingMetrics"] | undefined;
  wasmBytes: number;
  ownerTick: number;
  motor: CharacterMotor | undefined;
  yaw: number;
  pitch: number;
  camera: number;
  remotes: readonly RenderActor[];
};
const svgNS = "http://www.w3.org/2000/svg";
const number = (n: number | undefined, digits = 0) =>
  n === undefined ? "—" : n.toFixed(digits);

/** Bounded client telemetry: 12 seconds at the HUD's 10 Hz sampling rate. */
export class Diagnostics {
  readonly element = document.createElement("div");
  private readonly fields = new Map<string, HTMLElement>();
  private readonly connection = document.createElement("div");
  private readonly actors = document.createElement("details");
  private readonly actorSummary = document.createElement("summary");
  private readonly body = document.createElement("tbody");
  private readonly graphs: {
    samples: { time: number; value: number }[];
    line: SVGPolylineElement;
    value: HTMLElement;
    scale: HTMLElement;
    unit: string;
    minimum: number;
  }[] = [];
  private capture:
    | {
        started: number;
        frames: TimingHistogram;
        prediction: TimingHistogram;
        excluded: number;
        memory: unknown[];
        lastMemory: number;
      }
    | undefined;
  private readonly captureStatus = document.createElement("p");
  private readonly reportText = document.createElement("textarea");
  private frameTime = 0;
  private frameCount = 0;
  constructor() {
    this.element.className = "game-debug";
    this.element.setAttribute("aria-label", "Client diagnostics");
    this.connection.className = "debug-connection";
    this.connection.textContent = "Waiting for a game session";
    const charts = document.createElement("div");
    charts.className = "debug-charts";
    for (const [title, unit, minimum] of [
      ["Frame avg", "ms", 33.3],
      ["Round trip", "ms", 50],
      ["Correction", "m", 0.05],
    ] as const) {
      const figure = document.createElement("figure"),
        caption = document.createElement("figcaption"),
        value = document.createElement("strong"),
        scale = document.createElement("span");
      caption.append(title, value);
      const svg = document.createElementNS(svgNS, "svg"),
        line = document.createElementNS(svgNS, "polyline");
      svg.setAttribute("viewBox", "0 0 120 32");
      svg.setAttribute("preserveAspectRatio", "none");
      svg.setAttribute("role", "img");
      svg.setAttribute("aria-label", `${title}, last 12 seconds`);
      line.setAttribute("fill", "none");
      line.setAttribute("stroke", "currentColor");
      line.setAttribute("stroke-width", "1.25");
      line.setAttribute("vector-effect", "non-scaling-stroke");
      svg.append(line);
      scale.className = "debug-scale";
      figure.append(caption, svg, scale);
      charts.append(figure);
      this.graphs.push({ samples: [], line, value, scale, unit, minimum });
    }
    const sections = document.createElement("div");
    sections.className = "debug-sections";
    for (const [title, labels] of [
      [
        "Network",
        [
          "Player / epoch",
          "Server / owner",
          "Up / down",
          "Input lead",
          "Cached join",
        ],
      ],
      [
        "Prediction",
        ["Predicted tick", "Correction / p95", "Replay steps", "History ring"],
      ],
      [
        "Rendering",
        ["FPS / frame avg", "Draws / triangles", "Geometries", "WASM memory"],
      ],
      [
        "Streaming",
        [
          "Resident chunks",
          "Pending / fallback",
          "Instance matrices",
          "Remote entities",
        ],
      ],
    ] as const) {
      const section = document.createElement("section"),
        heading = document.createElement("h2"),
        list = document.createElement("dl");
      heading.textContent = title;
      for (const label of labels) {
        const term = document.createElement("dt"),
          value = document.createElement("dd");
        term.textContent = label;
        value.textContent = "—";
        this.fields.set(label, value);
        list.append(term, value);
      }
      section.append(heading, list);
      sections.append(section);
    }
    const pose = document.createElement("div");
    pose.className = "debug-pose";
    for (const label of ["Position XYZ", "Look / camera"]) {
      const row = document.createElement("div"),
        title = document.createElement("span"),
        value = document.createElement("span");
      title.textContent = label;
      row.append(title, value);
      pose.append(row);
      this.fields.set(label, value);
    }
    this.actors.className = "debug-actors";
    const table = document.createElement("table"),
      head = document.createElement("thead"),
      row = document.createElement("tr");
    for (const label of ["ID", "Kind", "X", "Y", "Z", "State"]) {
      const cell = document.createElement("th");
      cell.scope = "col";
      cell.textContent = label;
      row.append(cell);
    }
    head.append(row);
    table.append(head, this.body);
    const actorList = document.createElement("div");
    actorList.className = "debug-actor-list";
    actorList.append(table);
    this.actors.append(this.actorSummary, actorList);
    const capturePanel = document.createElement("details"),
      captureTitle = document.createElement("summary");
    captureTitle.textContent = "Performance capture";
    const start = document.createElement("button"),
      stop = document.createElement("button");
    start.textContent = "Start performance capture";
    stop.textContent = "Stop and show report";
    this.reportText.readOnly = true;
    this.reportText.rows = 8;
    this.reportText.setAttribute("aria-label", "Performance report JSON");
    start.onclick = () => {
      this.capture = {
        started: performance.now(),
        frames: new TimingHistogram(),
        prediction: new TimingHistogram(),
        excluded: 0,
        memory: [],
        lastMemory: -Infinity,
      };
      this.reportText.value = "";
      this.captureStatus.textContent =
        "Recording visible playing frames, including slow frames. Stop to view JSON.";
    };
    stop.onclick = () => {
      const c = this.capture;
      if (!c) return;
      this.reportText.value = JSON.stringify(
        {
          duration_s: (performance.now() - c.started) / 1000,
          frames: c.frames.report(),
          prediction_and_reconciliation_per_frame: c.prediction.report(),
          excluded_hidden_or_not_playing_frames: c.excluded,
          memory: c.memory,
          user_agent: navigator.userAgent,
          logical_cpus: navigator.hardwareConcurrency,
          viewport: [innerWidth, innerHeight],
          pixel_ratio: devicePixelRatio,
          scope:
            "Visible playing frames; prediction includes WASM, proxy preparation, reset/replay and input sampling. Heap only when supplied by browser. Memory samples bounded to 720.",
        },
        null,
        2,
      );
      this.capture = undefined;
      this.captureStatus.textContent =
        "Capture stopped. Copy JSON to save the report.";
    };
    capturePanel.append(
      captureTitle,
      start,
      stop,
      this.captureStatus,
      this.reportText,
    );
    this.element.append(
      this.connection,
      charts,
      sections,
      pose,
      this.actors,
      capturePanel,
    );
  }
  frame(ms: number, predictionMs = 0, visiblePlaying = false): void {
    if (this.capture) {
      if (visiblePlaying) {
        this.capture.frames.observe(ms);
        this.capture.prediction.observe(predictionMs);
      } else {
        this.capture.excluded++;
      }
    }
    if (ms >= 0 && ms < 1000) {
      this.frameTime += ms;
      this.frameCount++;
    }
  }
  private set(label: string, value: string): void {
    const node = this.fields.get(label);
    if (node && node.textContent !== value) node.textContent = value;
  }
  update(s: Sample): void {
    const c = this.capture;
    if (c && s.now - c.lastMemory >= 10000 && c.memory.length < 720) {
      const heap =
        (performance as Performance & { memory?: { usedJSHeapSize: number } })
          .memory?.usedJSHeapSize ?? null;
      c.memory.push({
        elapsed_s: (s.now - c.started) / 1000,
        heap_bytes: heap,
        wasm_bytes: s.wasmBytes,
        resident_chunks: s.streaming?.resident,
        geometries: s.streaming?.geometries,
        matrix_bytes: s.streaming?.matrixBytes,
      });
      c.lastMemory = s.now;
      this.captureStatus.textContent = `Recording ${((s.now - c.started) / 1000).toFixed(0)}s · frame p95 ${c.frames.report().p95_ms.toFixed(2)}ms · prediction p95 ${c.prediction.report().p95_ms.toFixed(2)}ms`;
    }
    const n = s.net,
      r = s.streaming;
    const frame = this.frameCount
      ? this.frameTime / this.frameCount
      : undefined;
    this.frameTime = this.frameCount = 0;
    this.connection.textContent = s.connection || "Waiting for a game session";
    this.connection.dataset.stale = String(n?.stale ?? false);
    const values = [frame, n?.rtt ?? undefined, n?.correction];
    for (const [i, graph] of this.graphs.entries()) {
      const value = values[i];
      graph.value.textContent = `${number(value, i === 2 ? 3 : 1)} ${graph.unit}`;
      if (value !== undefined) {
        graph.samples.push({ time: s.now, value });
        if (graph.samples.length > 120) graph.samples.shift();
      }
      graph.samples = graph.samples.filter(
        (point) => s.now - point.time <= 12000,
      );
      const max = Math.max(
        graph.minimum,
        ...graph.samples.map((point) => point.value),
      );
      graph.line.setAttribute(
        "points",
        graph.samples
          .map(
            (point) =>
              `${119 - ((s.now - point.time) / 12000) * 119},${31 - (point.value / max) * 30}`,
          )
          .join(" "),
      );
      graph.scale.textContent = `12s · 0–${number(max, i === 2 ? 3 : 0)} ${graph.unit}`;
    }
    this.set("Player / epoch", `${number(n?.player)} / ${number(n?.epoch)}`);
    this.set("Server / owner", `${number(n?.tick)} / ${s.ownerTick}`);
    this.set(
      "Up / down",
      `${number(n ? n.up / 1024 : undefined, 1)} / ${number(n ? n.down / 1024 : undefined, 1)} KiB/s`,
    );
    this.set("Cached join", `${number(n?.joinMs)} ms`);
    this.set("Input lead", `${number(n?.lead)} ticks`);
    this.set("Predicted tick", number(n?.predicted));
    this.set(
      "Correction / p95",
      `${number(n?.correction, 3)} / ${number(n?.p95, 3)} m`,
    );
    this.set("Replay steps", number(n?.replay));
    this.set("History ring", `${number(n?.history)} / 128`);
    this.set(
      "FPS / frame avg",
      `${number(frame && frame > 0 ? 1000 / frame : undefined)} / ${number(frame, 1)} ms`,
    );
    this.set(
      "Draws / triangles",
      `${number(r?.drawCalls)} / ${r?.triangles.toLocaleString() ?? "—"}`,
    );
    this.set("Geometries", number(r?.geometries));
    this.set("WASM memory", `${number(s.wasmBytes / 1048576, 1)} MiB`);
    this.set("Resident chunks", `${number(r?.resident)} / 36`);
    this.set(
      "Pending / fallback",
      `${number(r?.pending)} / ${number(r?.failed)}`,
    );
    this.set(
      "Instance matrices",
      `${number(r ? r.matrixBytes / 1024 : undefined)} KiB`,
    );
    this.set("Remote entities", String(s.remotes.length));
    this.set(
      "Position XYZ",
      s.motor?.position.map((v) => v.toFixed(2)).join("  ") ?? "—",
    );
    this.set(
      "Look / camera",
      `${s.yaw.toFixed(3)} / ${s.pitch.toFixed(3)} rad · ${s.camera.toFixed(2)} m`,
    );
    const stale = s.remotes.filter((a) => a.stale).length;
    this.actorSummary.textContent = `Entities · ${s.remotes.length} remote · ${stale} stale`;
    if (this.actors.open) {
      this.body.replaceChildren(
        ...s.remotes.map((actor) => {
          const row = document.createElement("tr");
          for (const value of [
            String(actor.id),
            actor.descriptor.entity_kind,
            ...actor.motor.position.map((v) => v.toFixed(1)),
            actor.stale ? "stale" : "live",
          ]) {
            const cell = document.createElement("td");
            cell.textContent = value;
            row.append(cell);
          }
          row.dataset.stale = String(actor.stale);
          return row;
        }),
      );
    }
  }
}
