import classic from "98.css/dist/98.css?raw";
import regular from "98.css/dist/ms_sans_serif.woff?url";
import regular2 from "98.css/dist/ms_sans_serif.woff2?url";
import bold from "98.css/dist/ms_sans_serif_bold.woff?url";
import bold2 from "98.css/dist/ms_sans_serif_bold.woff2?url";
import desktop from "./tooling.css?inline";

const tools = [
  ["/map", "City map", "Map Inspector"],
  ["/diagnostic", "Test client", "Connection Test Client"],
  ["/simulation", "Simulation", "Simulation Trace Runner"],
  ["/weapons", "Weapons", "Weapon Preview"],
  ["/architecture", "Architecture", "Architecture Workbench"],
] as const;

/** Loaded only by standalone tool routes; no stylesheet enters the game page. */
export function mountToolingChrome(root: HTMLElement): () => void {
  document.body.classList.add("tooling-mode");
  const style = document.createElement("style");
  // Upstream 0.1.21 ships a malformed hover query; normalize it before injection.
  style.textContent =
    classic
      .replace("(not(hover))", "(hover: none)")
      .replaceAll("ms_sans_serif.woff2", regular2)
      .replaceAll("ms_sans_serif.woff", regular)
      .replaceAll("ms_sans_serif_bold.woff2", bold2)
      .replaceAll("ms_sans_serif_bold.woff", bold) +
    "\n" +
    desktop;
  document.head.append(style);
  const current = tools.find(([path]) => path === location.pathname);
  const previousTitle = document.title;
  document.title = `Port Meridian — ${current?.[2] ?? "Tools"}`;
  const nav = document.createElement("nav");
  nav.className = "tool-menu";
  nav.setAttribute("aria-label", "Development tools");
  nav.innerHTML = `<strong>Port Meridian</strong>${tools.map(([path, label]) => `<a href="${path}" ${location.pathname === path ? 'aria-current="page"' : ""}>${label}</a>`).join("")}<a href="/" class="tool-play">Play game ↗</a>`;
  document.body.prepend(nav);
  let panel = root.querySelector<HTMLElement>(
    ".map-hud, .architecture-panel, .weapon-panel",
  );
  if (!panel) {
    panel = document.createElement("section");
    panel.append(...root.childNodes);
    root.append(panel);
    panel.className = "tool-dialog";
  }
  panel.classList.add("window");
  const title = document.createElement("div");
  title.className = "title-bar";
  const text = document.createElement("div");
  text.className = "title-bar-text";
  text.textContent = current?.[2] ?? "Tools";
  title.append(text);
  panel.prepend(title);
  const footer = document.createElement("footer");
  footer.className = "tool-status status-bar";
  footer.innerHTML = `<p class="status-bar-field">Port Meridian Development Suite</p><p class="status-bar-field">${current?.[2] ?? "Tools"}</p><p class="status-bar-field">Local workspace</p>`;
  document.body.append(footer);
  return () => {
    nav.remove();
    footer.remove();
    style.remove();
    document.body.classList.remove("tooling-mode");
    document.title = previousTitle;
  };
}
