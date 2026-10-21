import type {
  CharacterMotor,
  InputFrame,
  OwnerState,
  VehicleMotor,
} from "@port-meridian/protocol";
import type { VehicleTemplate } from "@port-meridian/content";
import type { PredictionWorld } from "./bridge";
import type { ServerClock } from "../net/clock";
export function actorMotor(
  motor: CharacterMotor | VehicleMotor,
): CharacterMotor {
  return "grounded" in motor
    ? motor
    : {
        position: motor.position,
        velocity: motor.velocity,
        yaw: motor.yaw,
        grounded: true,
        jump_cooldown_ticks: 0,
      };
}
const STEP_MS = 1000 / 30;
type Controls = Omit<InputFrame, "tick">;
type MotorWorld = Pick<PredictionWorld, "reset" | "step" | "currentState">;
const neutral = (tick: number, yaw: number): InputFrame => ({
  tick,
  move_x: 0,
  move_z: 0,
  look_yaw: yaw,
  look_pitch: 0,
  buttons: 0,
});
const mixYaw = (a: number, b: number, t: number) =>
  a + Math.atan2(Math.sin(b - a), Math.cos(b - a)) * t;
export function mixMotor(
  a: CharacterMotor,
  b: CharacterMotor,
  t: number,
): CharacterMotor {
  return {
    ...b,
    position: [0, 1, 2].map(
      (i) =>
        (a.position[i] ?? 0) +
        ((b.position[i] ?? 0) - (a.position[i] ?? 0)) * t,
    ) as [number, number, number],
    yaw: mixYaw(a.yaw, b.yaw, t),
  };
}
/** Owns the only local predicted motor; authority is never changed by presentation smoothing. */
export class LocalPrediction {
  readonly history = new Map<
    number,
    { input: InputFrame; motor: CharacterMotor }
  >();
  readonly corrections: number[] = [];
  correction = 0;
  replaySteps = 0;
  private owner: OwnerState | undefined;
  private future: OwnerState | undefined;
  private current: CharacterMotor | undefined;
  private previous: CharacterMotor | undefined;
  private offset: [number, number, number] = [0, 0, 0];
  private offsetAt = 0;
  private accumulated = STEP_MS;
  private lastAt = 0;
  private driftSince: number | undefined;
  stopped = true;
  constructor(
    private readonly world: MotorWorld,
    private readonly clock: ServerClock,
    private readonly resync: (
      reason: "stale" | "overflow" | "invalid_state",
    ) => void,
    private readonly templates: readonly VehicleTemplate[] = [],
    private readonly prepare: (
      tick: number,
      controlled: number,
    ) => void = () => {},
  ) {}
  private resetOwner(owner: OwnerState): void {
    this.prepare(owner.server_tick, owner.controlled_entity_id);
    const vehicle =
      owner.state.kind === "vehicle" ? owner.state.data.motor : undefined;
    const template = vehicle
      ? this.templates.find((t) => t.id === "compact")
      : undefined;
    if (vehicle && !template)
      throw new Error("Controlled vehicle template unavailable");
    this.world.reset({
      version: 1,
      tick: owner.server_tick,
      control_epoch: owner.control_epoch,
      controlled_entity_id: owner.controlled_entity_id,
      motor: actorMotor(owner.state.data.motor),
      ...(vehicle && template ? { vehicle: { motor: vehicle, template } } : {}),
    });
  }
  get tick(): number {
    return this.current ? this.world.currentState().tick : 0;
  }
  get epoch(): number {
    return this.owner?.control_epoch ?? 0;
  }
  get player(): number {
    return this.owner?.player_id ?? 0;
  }
  get p95(): number {
    const sorted = [...this.corrections].sort((a, b) => a - b);
    return sorted[Math.max(0, Math.ceil(sorted.length * 0.95) - 1)] ?? 0;
  }
  install(owner: OwnerState, now: number): void {
    this.resetOwner(owner);
    this.owner = owner;
    this.history.clear();
    this.offset = [0, 0, 0];
    this.current = actorMotor(owner.state.data.motor);
    this.previous = this.current;
    this.lastAt = now;
    this.accumulated = STEP_MS;
    this.driftSince = undefined;
    this.stopped = false;
    const first = Math.max(
      owner.server_tick + 1,
      Math.floor(this.clock.estimate(now)) + this.clock.lead,
    );
    if (first - owner.server_tick > 128) {
      this.fail("stale");
      return;
    }
    while (this.tick < first - 1)
      this.step(neutral(this.tick + 1, this.current.yaw));
    const cached = this.future;
    this.future = undefined;
    if (
      cached?.control_epoch === owner.control_epoch &&
      cached.server_tick > owner.server_tick
    )
      this.reconcile(cached, now);
  }
  stop(): void {
    this.stopped = true;
  }
  private fail(reason: "stale" | "overflow" | "invalid_state"): void {
    this.stop();
    this.resync(reason);
  }
  private step(input: InputFrame): void {
    this.prepare(input.tick, this.owner?.controlled_entity_id ?? 0);
    this.previous = this.current;
    this.current = this.world.step(input).motor;
    this.history.set(input.tick, { input, motor: this.current });
  }
  advance(now: number, sample: () => Controls): InputFrame[] {
    if (this.stopped || !this.current || !this.owner) return [];
    const elapsed = now - this.lastAt;
    this.lastAt = now;
    if (elapsed < 0 || elapsed > 250) {
      this.fail("stale");
      return [];
    }
    this.accumulated += elapsed;
    const desired = Math.floor(this.clock.estimate(now)) + this.clock.lead;
    const drift = this.tick - desired;
    if (Math.abs(drift) > 6) {
      this.fail("stale");
      return [];
    }
    let due = Math.min(5, Math.floor(this.accumulated / STEP_MS));
    if (Math.abs(drift) >= 1) this.driftSince ??= now;
    else this.driftSince = undefined;
    if (due && this.driftSince !== undefined && now - this.driftSince > 500) {
      if (drift > 0) {
        this.accumulated = Math.max(0, this.accumulated - STEP_MS);
        due--;
      } else due = Math.min(5, due + 1);
      this.driftSince = now;
    }
    const generated: InputFrame[] = [];
    for (let i = 0; i < due; i++) {
      if (this.history.size >= 128) {
        this.fail("overflow");
        return [];
      }
      const frame = { ...sample(), tick: this.tick + 1 };
      this.step(frame);
      generated.push(frame);
      this.accumulated = Math.max(0, this.accumulated - STEP_MS);
    }
    return generated;
  }
  batch(): InputFrame[] {
    return [...this.history.values()].slice(-3).map((h) => h.input);
  }
  reconcile(owner: OwnerState, now: number): void {
    if (!this.owner) return;
    if (owner.control_epoch > this.epoch) {
      if (
        !this.future ||
        owner.control_epoch > this.future.control_epoch ||
        owner.server_tick > this.future.server_tick
      )
        this.future = owner;
      return;
    }
    if (
      owner.control_epoch !== this.epoch ||
      owner.player_id !== this.player ||
      owner.controlled_entity_id !== this.owner.controlled_entity_id ||
      owner.server_tick <= this.owner.server_tick
    )
      return;
    if (owner.state.kind !== this.owner.state.kind) {
      this.fail("invalid_state");
      return;
    }
    const end = Math.max(this.tick, owner.server_tick);
    if (end - owner.server_tick > 128) {
      this.fail("overflow");
      return;
    }
    const oldMotor = this.current;
    const oldRender = this.presentation(now);
    const oldMode = this.owner.player_state.mode;
    this.resetOwner(owner);
    for (const tick of this.history.keys())
      if (tick <= owner.consumed_through_tick) this.history.delete(tick);
    this.owner = owner;
    this.previous = actorMotor(owner.state.data.motor);
    this.current = actorMotor(owner.state.data.motor);
    this.replaySteps = 0;
    for (let tick = owner.server_tick + 1; tick <= end; tick++) {
      this.step(
        this.history.get(tick)?.input ?? neutral(tick, this.current.yaw),
      );
      this.replaySteps++;
    }
    this.correction = oldMotor
      ? Math.hypot(
          ...oldMotor.position.map(
            (v, i) => v - (this.current?.position[i] ?? v),
          ),
        )
      : 0;
    this.corrections.push(this.correction);
    if (this.corrections.length > 256) this.corrections.shift();
    this.offset = [0, 0, 0];
    const nextRender = this.presentation(now);
    if (
      oldRender &&
      nextRender &&
      this.correction <= 2 &&
      oldMode === owner.player_state.mode &&
      owner.player_state.health > 0
    ) {
      this.offset = oldRender.position.map(
        (v, i) => v - (nextRender.position[i] ?? v),
      ) as [number, number, number];
    } else this.previous = this.current;
    this.offsetAt = now;
  }
  presentation(now: number): CharacterMotor | undefined {
    if (!this.current) return undefined;
    const motor = mixMotor(
      this.previous ?? this.current,
      this.current,
      Math.max(0, Math.min(1, this.accumulated / STEP_MS)),
    );
    const weight = Math.max(0, 1 - (now - this.offsetAt) / 100);
    motor.position = motor.position.map(
      (v, i) => v + (this.offset[i] ?? 0) * weight,
    ) as [number, number, number];
    return motor;
  }
}
