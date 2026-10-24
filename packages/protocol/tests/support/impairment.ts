/** Deterministic application delivery only; actual UDP impairment has a separate native suite. */
export const profiles = [
  { name: "loopback", rtt: 2, jitter: 0, loss: 0, reorder: 0, duplicate: 0 },
  {
    name: "broadband",
    rtt: 50,
    jitter: 10,
    loss: 0.01,
    reorder: 0.01,
    duplicate: 0.01,
  },
  {
    name: "supported",
    rtt: 100,
    jitter: 20,
    loss: 0.02,
    reorder: 0.05,
    duplicate: 0.01,
  },
  {
    name: "stressed",
    rtt: 200,
    jitter: 50,
    loss: 0.05,
    reorder: 0.1,
    duplicate: 0,
  },
  {
    name: "severe",
    rtt: 500,
    jitter: 100,
    loss: 0.2,
    reorder: 0.2,
    duplicate: 0.02,
    burst: true,
  },
  { name: "disconnect", rtt: 0, jitter: 0, loss: 1, reorder: 0, duplicate: 0 },
] as const;
export type Profile = {
  name: string;
  rtt: number;
  jitter: number;
  loss: number;
  reorder: number;
  duplicate: number;
  burst?: boolean;
};
export class Scheduler<T> {
  private queue: { at: number; order: number; packet: T }[] = [];
  private sequence = 0;
  private reliableAt = 0;
  private seed: number;
  readonly stats = {
    sent: 0,
    dropped: 0,
    duplicated: 0,
    reordered: 0,
    delivered: 0,
    peak: 0,
  };
  constructor(
    readonly profile: Profile,
    seed = 0x19c0ffee,
  ) {
    this.seed = seed;
  }
  random(): number {
    this.seed = (Math.imul(this.seed, 1664525) + 1013904223) >>> 0;
    return this.seed / 4294967296;
  }
  send(packet: T, now: number, reliable = false): void {
    this.stats.sent++;
    if (
      this.profile.name === "disconnect" ||
      (!reliable &&
        (this.random() < this.profile.loss ||
          (this.profile.burst && now % 3000 < 300)))
    ) {
      this.stats.dropped++;
      return;
    }
    let at =
      now +
      Math.max(
        0,
        this.profile.rtt / 2 + (this.random() * 2 - 1) * this.profile.jitter,
      );
    if (!reliable && this.random() < this.profile.reorder) {
      at += this.profile.rtt + 40;
      this.stats.reordered++;
    }
    if (reliable) {
      at = Math.max(at, this.reliableAt);
      this.reliableAt = at;
    }
    this.queue.push({ at, order: this.sequence++, packet });
    if (!reliable && this.random() < this.profile.duplicate) {
      this.queue.push({
        at: at + this.random() * 80,
        order: this.sequence++,
        packet,
      });
      this.stats.duplicated++;
    }
    this.stats.peak = Math.max(this.stats.peak, this.queue.length);
    if (this.queue.length > 512)
      throw new Error("Test scheduler queue exceeded 512 packets");
  }
  drain(now: number): T[] {
    this.queue.sort((a, b) => a.at - b.at || a.order - b.order);
    let count = 0;
    while (
      this.queue[count]?.at !== undefined &&
      (this.queue[count]?.at ?? Infinity) <= now
    )
      count++;
    const ready = this.queue.splice(0, count);
    this.stats.delivered += ready.length;
    return ready.map((p) => p.packet);
  }
  get pending(): number {
    return this.queue.length;
  }
}
