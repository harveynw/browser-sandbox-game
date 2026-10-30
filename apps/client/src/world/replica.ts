import type {
  CharacterMotor,
  EntityDescriptor,
  EntityShow,
  EntityHide,
  EntityState,
  Snapshot,
} from "@port-meridian/protocol";
import { mixMotor, actorMotor } from "../simulation/prediction";
export interface RenderActor {
  id: number;
  descriptor: EntityDescriptor;
  motor: CharacterMotor;
  stale: boolean;
  steering?: number;
  state: EntityState;
}
type Sample = { tick: number; state: EntityState; snap: boolean };
type Replica = {
  descriptor: EntityDescriptor;
  visibility: number;
  samples: Sample[];
  received: number;
};
const motor = (state: EntityState) => state.data.motor;
function discontinuity(a: EntityState, b: EntityState, ticks: number): boolean {
  if (a.kind !== b.kind) return true;
  if (
    a.kind === "character" &&
    b.kind === "character" &&
    (a.data.mode !== b.data.mode ||
      a.data.vehicle_id !== b.data.vehicle_id ||
      a.data.health <= 0 !== b.data.health <= 0)
  )
    return true;
  return (
    Math.hypot(
      ...motor(a).position.map((v, i) => v - (motor(b).position[i] ?? v)),
    ) >
    2 + (Math.max(0, ticks) / 30) * 35
  );
}
export class ReplicaStore {
  private readonly visible = new Map<number, Replica>();
  private readonly tombstones = new Map<number, number>();
  private frozen: RenderActor[] = [];
  private loading = false;
  ignored = 0;
  template(id: number): string | undefined {
    return this.visible.get(id)?.descriptor.template_id;
  }
  get size(): number {
    return this.visible.size;
  }
  beginBaseline(renderTick: number, now: number): void {
    this.frozen = this.sampleAll(renderTick, now);
    this.visible.clear();
    this.tombstones.clear();
    this.loading = true;
  }
  finishBaseline(): void {
    this.loading = false;
    this.frozen = [];
  }
  clear(): void {
    this.visible.clear();
    this.tombstones.clear();
    this.frozen = [];
    this.loading = false;
  }
  show(show: EntityShow, now: number): void {
    const id = show.descriptor.id;
    const previous = this.visible.get(id);
    if (
      show.visibility_id <= (this.tombstones.get(id) ?? 0) ||
      (previous && show.visibility_id <= previous.visibility)
    )
      return;
    if (!previous && this.visible.size >= 96)
      throw new Error("Replica visibility overflow");
    this.visible.set(id, {
      descriptor: show.descriptor,
      visibility: show.visibility_id,
      samples: [{ tick: show.state_tick, state: show.state, snap: true }],
      received: now,
    });
  }
  hide(hide: EntityHide): void {
    const visible = this.visible.get(hide.entity_id);
    if (visible?.visibility === hide.visibility_id)
      this.visible.delete(hide.entity_id);
    const retired = this.tombstones.get(hide.entity_id) ?? 0;
    this.tombstones.delete(hide.entity_id);
    this.tombstones.set(
      hide.entity_id,
      Math.max(retired, hide.visibility_id, visible?.visibility ?? 0),
    );
    while (this.tombstones.size > 256) {
      const first = this.tombstones.keys().next().value;
      if (first !== undefined) this.tombstones.delete(first);
    }
  }
  snapshot(snapshot: Snapshot, now: number): void {
    if (this.loading) return;
    for (const record of snapshot.records) {
      const replica = this.visible.get(record.entity_id),
        last = replica?.samples.at(-1);
      if (
        !replica ||
        !last ||
        replica.visibility !== record.visibility_id ||
        snapshot.server_tick <= last.tick
      ) {
        this.ignored++;
        continue;
      }
      replica.samples.push({
        tick: snapshot.server_tick,
        state: record.state,
        snap: discontinuity(
          last.state,
          record.state,
          snapshot.server_tick - last.tick,
        ),
      });
      if (replica.samples.length > 32) replica.samples.shift();
      replica.received = now;
    }
  }
  sampleAll(renderTick: number, now: number): RenderActor[] {
    if (this.loading) return this.frozen;
    const result: RenderActor[] = [];
    for (const [id, r] of this.visible) {
      const first = r.samples[0],
        latest = r.samples.at(-1);
      if (!first || !latest) continue;
      let state = actorMotor(latest.state.data.motor);
      let sampledState = latest.state;
      if (renderTick < first.tick) {
        state = actorMotor(first.state.data.motor);
        sampledState = first.state;
      } else if (renderTick < latest.tick) {
        const upper = r.samples.findIndex((s) => s.tick >= renderTick);
        const b = r.samples[upper],
          a = r.samples[upper - 1];
        if (a && b) {
          sampledState = b.snap ? b.state : a.state;
          state = b.snap
            ? actorMotor(b.state.data.motor)
            : mixMotor(
                actorMotor(a.state.data.motor),
                actorMotor(b.state.data.motor),
                (renderTick - a.tick) / Math.max(1, b.tick - a.tick),
              );
        }
      } else {
        const seconds = Math.max(0, Math.min(3, renderTick - latest.tick)) / 30;
        state = {
          ...state,
          position: state.position.map(
            (v, i) => v + (state.velocity[i] ?? 0) * seconds,
          ) as [number, number, number],
        };
      }
      result.push({
        id,
        descriptor: r.descriptor,
        motor: state,
        state: sampledState,
        stale: now - r.received > 500,
      });
    }
    return result;
  }
}
