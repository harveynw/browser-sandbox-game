import type { WeaponId } from "@port-meridian/protocol";
/** Original procedural audio; a shared bus and bounded voices, with no downloaded media. */
export type Cue =
  | "step"
  | "door"
  | "shot"
  | "impact"
  | "success"
  | "failure"
  | "reload"
  | "dry"
  | "skid";
export class VoicePool {
  private readonly slots: { priority: number; until: number }[] = [];
  constructor(readonly limit = 32) {}
  acquire(priority: number, now: number, duration: number): number | null {
    let slot = this.slots.findIndex((s) => s.until <= now);
    if (slot < 0 && this.slots.length < this.limit) slot = this.slots.length;
    if (slot < 0) {
      let lowest = 0;
      this.slots.forEach((s, i) => {
        if (s.priority < (this.slots[lowest]?.priority ?? Infinity)) lowest = i;
      });
      if (priority < (this.slots[lowest]?.priority ?? Infinity)) return null;
      slot = lowest;
    }
    this.slots[slot] = { priority, until: now + duration };
    return slot;
  }
  release(slot: number): void {
    const s = this.slots[slot];
    if (s) s.until = 0;
  }
}
type Voice = {
  source: AudioScheduledSourceNode;
  gain: GainNode;
  panner?: PannerNode;
  filter?: BiquadFilterNode;
};
export class GameAudio {
  private context: AudioContext | undefined;
  private master: GainNode | undefined;
  private effects: GainNode | undefined;
  private noise: AudioBuffer | undefined;
  private readonly pool = new VoicePool();
  private readonly voices = new Map<number, Voice>();
  private loops = new Map<
    string,
    { slot: number; source: OscillatorNode; gain: GainNode; panner: PannerNode }
  >();
  private muted = false;
  private paused = true;
  private volume = 0.6;
  private effectVolume = 0.8;
  private stride = 0;
  private skidAt = 0;
  private listener: readonly number[] = [0, 0, 0];
  private disposed = false;
  get status(): string {
    return this.muted
      ? "Muted"
      : !this.context
        ? "Sound starts with Play"
        : this.paused
          ? "Sound paused"
          : "Sound on";
  }
  /** Called only from Play/Resume gestures, never from a network packet or page load. */
  async unlock(): Promise<void> {
    if (this.disposed) return;
    try {
      if (!this.context) {
        this.context = new AudioContext();
        this.master = this.context.createGain();
        this.effects = this.context.createGain();
        this.effects.connect(this.master);
        this.master.connect(this.context.destination);
        this.noise = this.context.createBuffer(
          1,
          this.context.sampleRate,
          this.context.sampleRate,
        );
        const samples = this.noise.getChannelData(0);
        let seed = 89173;
        for (let i = 0; i < samples.length; i++) {
          seed = (seed * 1664525 + 1013904223) >>> 0;
          samples[i] = (seed / 4294967296) * 2 - 1;
        }
        this.applyVolume();
      }
      await this.context.resume();
      if (this.paused || this.muted || this.disposed)
        await this.context.suspend();
    } catch {
      /* Silent play remains available if audio is unsupported or denied. */
    }
  }
  configure(master: number, effects: number, muted: boolean): void {
    this.volume = master;
    this.effectVolume = effects;
    this.muted = muted;
    this.applyVolume();
    if (muted) this.silence();
    else if (!this.paused) void this.context?.resume().catch(() => {});
  }
  private applyVolume(): void {
    const c = this.context;
    if (!c) return;
    this.master?.gain.setTargetAtTime(
      this.muted || this.paused ? 0 : this.volume,
      c.currentTime,
      0.02,
    );
    this.effects?.gain.setTargetAtTime(this.effectVolume, c.currentTime, 0.02);
  }
  pause(paused: boolean): void {
    if (this.paused === paused) return;
    this.paused = paused;
    this.applyVolume();
    if (paused) {
      this.silence();
      void this.context?.suspend().catch(() => {});
    } else if (!this.muted) void this.context?.resume().catch(() => {});
  }
  private spatial(position: readonly number[], c: AudioContext): PannerNode {
    const p = c.createPanner();
    p.panningModel = "equalpower";
    p.distanceModel = "inverse";
    p.refDistance = 4;
    p.maxDistance = 70;
    p.rolloffFactor = 1.4;
    p.positionX.value = position[0] ?? 0;
    p.positionY.value = position[1] ?? 0;
    p.positionZ.value = position[2] ?? 0;
    return p;
  }
  private stop(slot: number): void {
    const v = this.voices.get(slot);
    if (!v) return;
    this.voices.delete(slot);
    try {
      v.source.stop();
    } catch {
      /* already ended */
    }
    v.source.disconnect();
    v.gain.disconnect();
    v.panner?.disconnect();
    v.filter?.disconnect();
    this.pool.release(slot);
  }
  cue(
    cue: Cue,
    position: readonly number[] = this.listener,
    priority = 3,
    weapon: WeaponId = "pistol",
  ): void {
    const c = this.context,
      effects = this.effects;
    if (
      !c ||
      !effects ||
      !this.noise ||
      c.state !== "running" ||
      this.muted ||
      this.paused
    )
      return;
    if (Math.hypot(...position.map((v, i) => v - (this.listener[i] ?? 0))) > 70)
      return;
    const durations: Record<Cue, number> = {
      step: 0.09,
      door: 0.16,
      shot: weapon === "rifle" ? 0.26 : weapon === "machine_gun" ? 0.13 : 0.17,
      impact: 0.2,
      success: 0.6,
      failure: 0.4,
      reload: weapon === "pistol" ? 0.3 : 0.45,
      dry: 0.065,
      skid: 0.18,
    };
    const duration = durations[cue],
      slot = this.pool.acquire(priority, c.currentTime, duration);
    if (slot === null) return;
    // acquire already reserves this slot; stopping the old source must not release the new lease.
    const old = this.voices.get(slot);
    if (old) {
      try {
        old.source.stop();
      } catch {
        /* ended */
      }
      old.source.disconnect();
      old.gain.disconnect();
      old.panner?.disconnect();
      old.filter?.disconnect();
    }
    const gain = c.createGain(),
      panner = this.spatial(position, c);
    const noisy = ["step", "shot", "impact", "skid"].includes(cue);
    let source: AudioScheduledSourceNode;
    if (noisy) {
      const s = c.createBufferSource();
      s.buffer = this.noise ?? null;
      s.playbackRate.value = cue === "step" ? 0.55 : cue === "skid" ? 1.8 : 1;
      source = s;
    } else {
      const s = c.createOscillator();
      s.type =
        cue === "door"
          ? "triangle"
          : cue === "reload" || cue === "dry"
            ? "square"
            : "sine";
      s.frequency.setValueAtTime(
        cue === "success"
          ? 440
          : cue === "failure"
            ? 220
            : cue === "dry"
              ? weapon === "pistol"
                ? 260
                : 180
              : weapon === "rifle"
                ? 210
                : weapon === "machine_gun"
                  ? 110
                  : 160,
        c.currentTime,
      );
      s.frequency.exponentialRampToValueAtTime(
        cue === "success" ? 880 : cue === "failure" ? 110 : 80,
        c.currentTime + duration,
      );
      source = s;
    }
    const peak =
      cue === "shot" ? 0.12 : cue === "step" || cue === "dry" ? 0.025 : 0.07;
    gain.gain.setValueAtTime(0, c.currentTime);
    gain.gain.linearRampToValueAtTime(peak, c.currentTime + 0.006);
    gain.gain.exponentialRampToValueAtTime(0.0001, c.currentTime + duration);
    const filter = c.createBiquadFilter();
    filter.type = cue === "shot" ? "lowpass" : "bandpass";
    filter.frequency.value =
      cue === "shot"
        ? weapon === "rifle"
          ? 1800
          : weapon === "machine_gun"
            ? 1100
            : 3200
        : 900;
    filter.Q.value = 0.7;
    source.connect(filter);
    filter.connect(gain);
    gain.connect(panner);
    panner.connect(effects);
    const voice = { source, gain, panner, filter };
    this.voices.set(slot, voice);
    source.onended = () => {
      if (this.voices.get(slot) === voice) {
        this.voices.delete(slot);
        this.pool.release(slot);
        source.disconnect();
        gain.disconnect();
        panner.disconnect();
        filter.disconnect();
      }
    };
    source.start();
    source.stop(c.currentTime + duration);
  }
  private loop(
    name: string,
    frequency: number,
    volume: number,
    position: readonly number[],
  ): void {
    const c = this.context,
      effects = this.effects;
    if (!c || !effects) return;
    let loop = this.loops.get(name);
    if (!loop) {
      const slot = this.pool.acquire(4, c.currentTime, Infinity);
      if (slot === null) return;
      const old = this.voices.get(slot);
      if (old) {
        try {
          old.source.stop();
        } catch {
          /* ended */
        }
        old.source.disconnect();
        old.gain.disconnect();
        old.panner?.disconnect();
        old.filter?.disconnect();
      }
      const source = c.createOscillator(),
        gain = c.createGain(),
        panner = this.spatial(position, c);
      source.type = name === "engine" ? "triangle" : "sine";
      source.connect(gain);
      gain.connect(panner);
      panner.connect(effects);
      gain.gain.value = 0;
      source.start();
      loop = { slot, source, gain, panner };
      this.loops.set(name, loop);
      this.voices.set(slot, { source, gain, panner });
    }
    loop.source.frequency.setTargetAtTime(frequency, c.currentTime, 0.08);
    loop.gain.gain.setTargetAtTime(volume, c.currentTime, 0.08);
    loop.panner.positionX.value = position[0] ?? 0;
    loop.panner.positionY.value = position[1] ?? 0;
    loop.panner.positionZ.value = position[2] ?? 0;
  }
  private endLoop(name: string): void {
    const loop = this.loops.get(name);
    if (loop) {
      this.stop(loop.slot);
      this.loops.delete(name);
    }
  }
  update(
    position: readonly number[],
    yaw: number,
    speed: number,
    grounded: boolean,
    driving: boolean,
    skidding: boolean,
    siren: readonly number[] | undefined,
    dt: number,
    topSpeed = 36,
  ): void {
    this.listener = position;
    const c = this.context;
    if (!c || this.paused || this.muted || c.state !== "running") return;
    c.listener.positionX.value = position[0] ?? 0;
    c.listener.positionY.value = (position[1] ?? 0) + 1.5;
    c.listener.positionZ.value = position[2] ?? 0;
    c.listener.forwardX.value = -Math.sin(yaw);
    c.listener.forwardY.value = 0;
    c.listener.forwardZ.value = -Math.cos(yaw);
    c.listener.upY.value = 1;
    if (driving)
      this.loop(
        "engine",
        48 + Math.min(speed / Math.max(1, topSpeed), 1) * 180,
        0.06,
        position,
      );
    else this.endLoop("engine");
    if (siren)
      this.loop("siren", 650 + Math.sin(c.currentTime * 5) * 210, 0.035, siren);
    else this.endLoop("siren");
    this.stride += !driving && grounded && speed > 0.5 ? dt * speed : 0;
    if (this.stride > 1.7) {
      this.stride = 0;
      this.cue("step");
    }
    if (driving && skidding && speed > 4 && c.currentTime > this.skidAt) {
      this.skidAt = c.currentTime + 0.3;
      this.cue("skid");
    }
  }
  private silence(): void {
    for (const slot of [...this.voices.keys()]) this.stop(slot);
    this.loops.clear();
    this.stride = 0;
  }
  dispose(): void {
    this.disposed = true;
    this.silence();
    void this.context?.close().catch(() => {});
    this.context = undefined;
  }
}
