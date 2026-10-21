import { expect, it } from "vitest";
import { ServerClock } from "./clock";
it("uses the lowest RTT of the last eight samples and bounded one-way input lead", () => {
  const clock = new ServerClock();
  clock.observe(30, 1000);
  expect(clock.lead).toBe(3);
  expect(clock.estimate(2000)).toBeCloseTo(60);
  clock.pong(60, 1950, 2050);
  expect(clock.lead).toBe(4);
  expect(clock.estimate(2050)).toBeCloseTo(61.5);
  clock.pong(63, 1900, 2300);
  expect(clock.rtt).toBe(100);
  for (let i = 0; i < 8; i++)
    clock.pong(90 + i, 3000 + i * 100, 3500 + i * 100);
  expect(clock.rtt).toBe(500);
  expect(clock.lead).toBe(6);
  clock.pong(120, 4000, 4000);
  expect(clock.lead).toBe(2);
  clock.pong(1, 5000, 4000);
  expect(clock.rtt).toBe(0);
});
