import { expect, it, vi } from "vitest";
const { initialize } = vi.hoisted(() => ({ initialize: vi.fn() }));
vi.mock("../../sim-wasm/sim_wasm", () => ({
  default: initialize,
  WasmPredictionWorld: vi.fn(),
}));
import {
  initializeSimulation,
  BridgeError,
} from "../../../apps/client/src/simulation/bridge";

it("shares asynchronous WASM initialization and permits retry after a load failure", async () => {
  initialize.mockRejectedValueOnce(new Error("asset unavailable"));
  const first = initializeSimulation();
  const concurrent = initializeSimulation();
  await expect(first).rejects.toThrow(BridgeError);
  await expect(concurrent).rejects.toThrow(
    "WASM load failed: Error: asset unavailable",
  );
  expect(initialize).toHaveBeenCalledTimes(1);
  initialize.mockResolvedValueOnce({});
  await initializeSimulation();
  await initializeSimulation();
  expect(initialize).toHaveBeenCalledTimes(2);
});
