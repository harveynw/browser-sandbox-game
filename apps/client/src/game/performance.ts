/** Constant-memory timing distribution; upper bucket bounds conservatively include stalls. */
export class TimingHistogram {
  private readonly bins = new Float64Array(4096);
  private count = 0;
  private sum = 0;
  private max = 0;
  observe(ms: number): void {
    if (!Number.isFinite(ms) || ms < 0) return;
    const bucket = Math.min(4095, Math.floor(ms * 20));
    this.bins[bucket] = (this.bins[bucket] ?? 0) + 1;
    this.count++;
    this.sum += ms;
    this.max = Math.max(this.max, ms);
  }
  report() {
    const quantile = (p: number) => {
      if (!this.count) return 0;
      let count = 0;
      for (const [i, n] of this.bins.entries()) {
        count += n;
        if (count >= Math.ceil(this.count * p))
          return i === 4095 ? this.max : (i + 1) / 20;
      }
      return this.max;
    };
    return {
      samples: this.count,
      p50_ms: quantile(0.5),
      p95_ms: quantile(0.95),
      p99_ms: quantile(0.99),
      max_ms: this.max,
      mean_ms: this.count ? this.sum / this.count : 0,
      fps: this.sum ? (1000 * this.count) / this.sum : 0,
    };
  }
}
