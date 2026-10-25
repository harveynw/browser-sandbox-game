import { expect, test } from "vitest";
import { TimingHistogram } from "./performance";

test("frame percentiles retain long stalls and overflow without storing raw samples", () => {
  const h = new TimingHistogram();
  for (let i = 0; i < 98; i++) h.observe(16);
  h.observe(50);
  h.observe(2000);
  const r = h.report();
  expect(r.samples).toBe(100);
  expect(r.p95_ms).toBe(16.05);
  expect(r.p99_ms).toBe(50.05);
  expect(r.max_ms).toBe(2000);
  expect(r.fps).toBeLessThan(30);
});
