const STEP_MS = 1000 / 30;
/** Server ticks are monotonic simulation time; ping timestamps are local monotonic time. */
export class ServerClock {
  private anchor = { tick: 0, at: 0 };
  private samples: { tick: number; at: number; rtt: number }[] = [];
  observe(tick: number, now: number): void {
    if (!this.samples.length && tick >= this.anchor.tick)
      this.anchor = { tick, at: now };
  }
  pong(tick: number, sent: number, received: number): void {
    const rtt = received - sent;
    if (!Number.isFinite(rtt) || rtt < 0 || rtt > 10_000) return;
    this.samples.push({ tick, at: received, rtt });
    if (this.samples.length > 8) this.samples.shift();
    const best = [...this.samples].sort(
      (a, b) => a.rtt - b.rtt || b.at - a.at,
    )[0];
    if (best)
      this.anchor = { tick: best.tick + best.rtt / 2 / STEP_MS, at: best.at };
  }
  estimate(now: number): number {
    return this.anchor.tick + Math.max(0, now - this.anchor.at) / STEP_MS;
  }
  get rtt(): number | null {
    return this.samples.length
      ? Math.min(...this.samples.map((s) => s.rtt))
      : null;
  }
  get lead(): number {
    return this.rtt === null
      ? 3
      : Math.max(2, Math.min(6, Math.ceil(this.rtt / 2 / STEP_MS) + 2));
  }
}
