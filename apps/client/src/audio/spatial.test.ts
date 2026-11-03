import { expect, it, vi } from "vitest";
import { setAudioPosition, setAudioOrientation } from "./spatial";
it("uses legacy listener methods when Firefox AudioParams are absent", () => {
  const listener = { setPosition: vi.fn(), setOrientation: vi.fn() };
  setAudioPosition(listener, [3, 4.5, 8]);
  setAudioOrientation(listener, Math.PI / 2);
  expect(listener.setPosition).toHaveBeenCalledWith(3, 4.5, 8);
  const args = listener.setOrientation.mock.calls[0] ?? [];
  expect(args[0]).toBeCloseTo(-1);
  expect(args[2]).toBeCloseTo(0);
  expect(args.slice(3)).toEqual([0, 1, 0]);
});
it("sets modern spatial parameters without invoking legacy position methods", () => {
  const node = {
    positionX: { value: 0 },
    positionY: { value: 0 },
    positionZ: { value: 0 },
    setPosition: vi.fn(),
  };
  setAudioPosition(node, [1, 2, 3]);
  expect([
    node.positionX.value,
    node.positionY.value,
    node.positionZ.value,
  ]).toEqual([1, 2, 3]);
  expect(node.setPosition).not.toHaveBeenCalled();
});
it("degrades safely if spatial APIs are unavailable", () => {
  expect(() => {
    setAudioPosition({}, [0, 0, 0]);
    setAudioOrientation({}, 0);
  }).not.toThrow();
});
