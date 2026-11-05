export function mountPoliceTool(root: HTMLElement): () => void {
  root.innerHTML = `<h1>Police pursuit monitor</h1>
    <p>Live server state · refreshes every second</p>
    <label>Playtest scenario <select>
      <option>Chase — alert a patrol, then drive a compact along a clear road</option>
      <option>Interception — reach three stars, then cross several junctions</option>
      <option>Recovery — block a patrol's route, then clear it</option>
      <option>Firefight — shoot a patrol, stay on foot, then hold G to surrender</option>
      <option>Escape — break sight around a building and stay hidden</option>
    </select></label>
    <p role="status">Connecting…</p>
    <div style="overflow:auto;max-height:65vh"><pre></pre></div>`;
  const status = root.querySelector<HTMLElement>('[role="status"]');
  const output = root.querySelector("pre");
  if (!status || !output)
    throw new Error("Police monitor elements are missing");
  const controller = new AbortController();
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const refresh = async (): Promise<void> => {
    try {
      const response = await fetch("/api/metrics", {
        signal: controller.signal,
      });
      if (!response.ok) throw new Error(`Server returned ${response.status}`);
      const data = (await response.json()) as Record<string, unknown>;
      status.textContent = `Live · tick ${String(data.tick)}`;
      output.textContent = JSON.stringify(
        {
          population: data.population,
          police: data.police,
          tick_work: data.tick_work,
          network: data.world,
        },
        null,
        2,
      );
    } catch (error) {
      if (!stopped) status.textContent = `Unavailable · ${String(error)}`;
    }
    if (!stopped)
      timer = setTimeout(() => {
        void refresh();
      }, 1000);
  };
  void refresh();
  return () => {
    stopped = true;
    controller.abort();
    clearTimeout(timer);
    root.replaceChildren();
  };
}
