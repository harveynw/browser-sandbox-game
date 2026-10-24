import { expect, it, vi } from "vitest";
import { GameAudio, VoicePool } from "./sound";
it("caps sources and preserves high-priority local/loop feedback under remote bursts", () => {
  const p = new VoicePool(3);
  expect(p.acquire(4, 0, Infinity)).toBe(0);
  p.acquire(1, 0, 1);
  p.acquire(1, 0, 1);
  expect(p.acquire(0, 0, 1)).toBeNull();
  expect(p.acquire(3, 0, 1)).toBe(1);
  expect(p.acquire(3, 0, 1)).toBe(2);
  expect(p.acquire(1, 0, 1)).toBeNull();
  expect(p.acquire(1, 2, 1)).toBe(1);
});
it("does not create audio before a gesture and applies mute/pause on the master bus", async () => {
  const param = () => ({ value: 1, setTargetAtTime: vi.fn() });
  const master = { gain: param(), connect: vi.fn() };
  const effects = { gain: param(), connect: vi.fn() };
  const c = {
    sampleRate: 8,
    currentTime: 0,
    destination: {},
    createGain: vi
      .fn()
      .mockReturnValueOnce(master)
      .mockReturnValueOnce(effects),
    createBuffer: () => ({ getChannelData: () => new Float32Array(8) }),
    resume: vi.fn().mockResolvedValue(undefined),
    suspend: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
  };
  const constructor = vi.fn(function () {
    return c;
  });
  vi.stubGlobal("AudioContext", constructor);
  try {
    const a = new GameAudio();
    a.cue("shot");
    expect(constructor).not.toHaveBeenCalled();
    await a.unlock();
    expect(constructor).toHaveBeenCalledTimes(1);
    expect(c.suspend).toHaveBeenCalled();
    a.pause(false);
    a.configure(0.5, 0.2, true);
    expect(master.gain.setTargetAtTime).toHaveBeenLastCalledWith(0, 0, 0.02);
    a.pause(true);
    a.dispose();
    expect(c.close).toHaveBeenCalled();
  } finally {
    vi.unstubAllGlobals();
  }
});
