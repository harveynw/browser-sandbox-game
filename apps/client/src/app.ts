import { DiagnosticSession } from "./net/diagnostic";

export function mountDiagnosticPage(root: HTMLElement): () => void {
  const eyebrow = document.createElement("p");
  eyebrow.className = "eyebrow";
  eyebrow.textContent = "Connection diagnostics";
  const title = document.createElement("h1");
  title.textContent = "Port Meridian";
  const description = document.createElement("p");
  description.textContent =
    "Join a localhost guest session, verify WS and RTC, and inspect authoritative server ticks. Another tab using the same guest replaces this session. Use the movement probe to test server inputs; use the game page for third-person controls.";
  const status = document.createElement("p");
  status.className = "status";
  status.setAttribute("role", "status");
  status.textContent = "Disconnected";
  const actions = document.createElement("div");
  actions.className = "actions";
  const connect = document.createElement("button");
  connect.textContent = "Connect";
  const disconnect = document.createElement("button");
  disconnect.textContent = "Disconnect";
  disconnect.disabled = true;
  const serverClose = document.createElement("button");
  const movement = document.createElement("button");
  movement.textContent = "Movement probe";
  movement.onclick = () => session?.probeMovement();
  serverClose.textContent = "Resync baseline";
  serverClose.onclick = () => session?.requestResync();
  const log = document.createElement("pre");
  log.setAttribute("aria-label", "Connection log");
  let session: DiagnosticSession | undefined;
  const lines: string[] = [];
  const append = (text: string) => {
    lines.push(text);
    if (lines.length > 40) lines.shift();
    log.textContent = lines.join("\n");
    log.scrollTop = log.scrollHeight;
  };
  connect.onclick = () => {
    session?.close();
    connect.disabled = true;
    disconnect.disabled = false;
    session = new DiagnosticSession(append, (text) => {
      status.textContent = text;
      if (text.startsWith("Closed:")) {
        connect.disabled = false;
        disconnect.disabled = true;
      }
    });
    void session.connect();
  };
  disconnect.onclick = () => session?.close();
  actions.append(connect, disconnect, serverClose, movement);
  const game = document.createElement("a");
  game.href = "/";
  game.textContent = "Back to game";
  const map = document.createElement("a");
  map.href = "/map";
  map.textContent = "Inspect the city map";
  root.replaceChildren(
    eyebrow,
    title,
    description,
    status,
    actions,
    log,
    game,
    " · ",
    map,
  );
  const dispose = () => session?.close("Page closed");
  window.addEventListener("pagehide", dispose);
  return () => {
    dispose();
    window.removeEventListener("pagehide", dispose);
    connect.onclick =
      disconnect.onclick =
      serverClose.onclick =
      movement.onclick =
        null;
  };
}
