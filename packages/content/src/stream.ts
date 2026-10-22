import type { ChunkAsset, VisualChunk } from "./generated/types";
export function chunkDistance(
  chunk: ChunkAsset,
  position: readonly number[],
): number {
  return Math.hypot(
    Math.max(
      chunk.bounds.min[0] - (position[0] ?? 0),
      0,
      (position[0] ?? 0) - chunk.bounds.max[0],
    ),
    Math.max(
      chunk.bounds.min[2] - (position[2] ?? 0),
      0,
      (position[2] ?? 0) - chunk.bounds.max[2],
    ),
  );
}
/** A bounded visual working set. This class never owns, replaces or unloads physics. */
export class ChunkStream {
  private readonly resident = new Set<string>();
  private readonly pending = new Map<string, AbortController>();
  private readonly failed = new Set<string>();
  private position: readonly number[] = [0, 0, 0];
  private disposed = false;
  constructor(
    private readonly chunks: readonly ChunkAsset[],
    initial: readonly string[],
    private readonly load: (
      id: string,
      signal: AbortSignal,
    ) => Promise<VisualChunk>,
    private readonly show: (chunk: VisualChunk) => void,
    private readonly hide: (id: string) => void,
  ) {
    initial.forEach((id) => this.resident.add(id));
  }
  update(position: readonly number[]): void {
    if (this.disposed) return;
    this.position = position;
    for (const chunk of this.chunks)
      if (chunkDistance(chunk, position) > 256) {
        if (this.resident.delete(chunk.id)) this.hide(chunk.id);
        this.failed.delete(chunk.id);
        this.pending.get(chunk.id)?.abort();
      }
    this.enforceBudget();
    this.pump();
  }
  private enforceBudget(): void {
    const farthest = this.chunks
      .filter((c) => this.resident.has(c.id))
      .sort(
        (a, b) =>
          chunkDistance(b, this.position) - chunkDistance(a, this.position),
      );
    for (const chunk of farthest) {
      if (this.resident.size <= 24) break;
      this.resident.delete(chunk.id);
      this.hide(chunk.id);
    }
  }
  private pump(): void {
    if (this.disposed) return;
    const desired = this.chunks
      .filter(
        (c) =>
          chunkDistance(c, this.position) <= 192 &&
          !this.resident.has(c.id) &&
          !this.pending.has(c.id) &&
          !this.failed.has(c.id),
      )
      .sort(
        (a, b) =>
          chunkDistance(a, this.position) - chunkDistance(b, this.position),
      );
    for (const chunk of desired) {
      if (this.pending.size >= 4) break;
      const controller = new AbortController();
      this.pending.set(chunk.id, controller);
      void this.fetch(chunk.id, controller);
    }
  }
  private async fetch(id: string, controller: AbortController): Promise<void> {
    try {
      for (let attempt = 0; attempt < 3; attempt++) {
        try {
          const data = await this.load(id, controller.signal);
          if (!this.disposed && !controller.signal.aborted) {
            this.show(data);
            this.resident.add(id);
            this.enforceBudget();
          }
          return;
        } catch (error) {
          if (controller.signal.aborted || this.disposed) return;
          if (attempt === 2) throw error;
          await new Promise<void>((resolve) => {
            const finish = () => {
              clearTimeout(timer);
              controller.signal.removeEventListener("abort", finish);
              resolve();
            };
            const timer = setTimeout(finish, 250 * (attempt + 1));
            controller.signal.addEventListener("abort", finish, { once: true });
          });
        }
      }
    } catch {
      if (!this.disposed) this.failed.add(id);
    } finally {
      this.pending.delete(id);
      this.pump();
    }
  }
  get metrics() {
    return {
      resident: this.resident.size,
      pending: this.pending.size,
      failed: this.failed.size,
    };
  }
  dispose(): void {
    this.disposed = true;
    for (const controller of this.pending.values()) controller.abort();
    this.pending.clear();
    for (const id of this.resident) this.hide(id);
    this.resident.clear();
    this.failed.clear();
  }
}
