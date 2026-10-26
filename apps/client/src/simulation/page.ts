import { runCharacterTrace } from "./trace";

export function mountSimulationPage(root: HTMLElement): () => void {
  root.innerHTML = `<main><span class="eyebrow">Shared Rust simulation</span><h1>Browser motor check</h1><p>Replay the frozen native trace through the WASM byte bridge: 1,800 ticks, 60 simulated seconds, 13 checkpoints. Position and velocity must agree within 1 cm.</p><div class="actions"><button type="button">Run trace</button></div><p class="status" role="status">Ready to load WASM</p><pre aria-label="Trace results">Static collision is loaded once per run. Play with keyboard and mouse at the game page.</pre><p class="tools"><a href="/map">Map inspector</a> · <a href="/">Back to game</a> · <a href="/diagnostic">Connection diagnostic</a></p></main>`;
  const button = root.querySelector("button");
  const status = root.querySelector("[role=status]");
  const output = root.querySelector("pre");
  if (!button || !status || !output)
    throw new Error("Missing simulation diagnostic elements");
  const controller = new AbortController();
  let running = false;
  const run = async (): Promise<void> => {
    if (running) return;
    running = true;
    button.disabled = true;
    status.textContent = "Loading WASM and replaying…";
    try {
      const result = await runCharacterTrace(controller.signal);
      if (controller.signal.aborted) return;
      status.textContent = `PASS · ${result.ticks} ticks · ${result.checkpoints} checkpoints`;
      output.textContent = `Maximum position error: ${result.maxPositionError.toFixed(6)} m\nMaximum velocity error: ${result.maxVelocityError.toFixed(6)} m/s\nMean bridge + simulation: ${result.millisecondsPerStep.toFixed(3)} ms/tick\nStatic ray query: PASS\nFinal tick: ${result.finalFrame.tick}\nFinal feet: ${result.finalFrame.motor.position.map((v) => v.toFixed(5)).join(", ")}\n\nWASM world freed after completion.`;
    } catch (error) {
      if (controller.signal.aborted) return;
      status.textContent = "FAIL · retry available";
      output.textContent =
        error instanceof Error ? error.message : String(error);
    } finally {
      running = false;
      if (!controller.signal.aborted) button.disabled = false;
    }
  };
  const onClick = (): void => {
    void run();
  };
  button.addEventListener("click", onClick);
  return () => {
    controller.abort();
    button.removeEventListener("click", onClick);
  };
}
