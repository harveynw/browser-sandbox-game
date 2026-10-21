import "./style.css";
const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("The application root #app is missing.");
let dispose: (() => void) | undefined;
let disposed = false;
const mount =
  location.pathname === "/simulation"
    ? import("./simulation/page").then(
        ({ mountSimulationPage }) => mountSimulationPage,
      )
    : location.pathname === "/map"
      ? import("./content/inspector").then(
          ({ mountMapInspector }) => mountMapInspector,
        )
      : location.pathname === "/diagnostic"
        ? import("./app").then(({ mountDiagnosticPage }) => mountDiagnosticPage)
        : import("./game/app").then(({ mountGamePage }) => mountGamePage);
void mount
  .then((render) => {
    if (disposed) return;
    dispose = render(root);
  })
  .catch((error: unknown) => {
    if (!disposed)
      root.textContent = `Unable to start: ${error instanceof Error ? error.message : "unknown error"}`;
  });
if (import.meta.hot)
  import.meta.hot.dispose(() => {
    disposed = true;
    dispose?.();
  });
