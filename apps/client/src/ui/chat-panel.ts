import type { ChatLine } from "@port-meridian/protocol";
export type ChatPart = { text: string; style?: "strong" | "em" | "s" | "code" };
/** Inline markup only; text is always inserted as textContent, never HTML. */
export function chatParts(text: string): ChatPart[] {
  const parts: ChatPart[] = [];
  const pattern =
    /\*\*([^*\n]+)\*\*|\*([^*\n]+)\*|_([^_\n]+)_|~~([^~\n]+)~~|~([^~\n]+)~|`([^`\n]+)`/g;
  let start = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > start)
      parts.push({ text: text.slice(start, match.index) });
    const content =
      match[1] ??
      match[2] ??
      match[3] ??
      match[4] ??
      match[5] ??
      match[6] ??
      "";
    parts.push({
      text: content,
      style:
        match[1] || match[2]
          ? "strong"
          : match[3]
            ? "em"
            : match[4] || match[5]
              ? "s"
              : "code",
    });
    start = match.index + match[0].length;
  }
  if (start < text.length) parts.push({ text: text.slice(start) });
  return parts;
}
export class ChatPanel {
  readonly element = document.createElement("section");
  private readonly log = document.createElement("div");
  private readonly form = document.createElement("form");
  private readonly input = document.createElement("textarea");
  private readonly error = document.createElement("p");
  private readonly entries: { id: number; at: number; node: HTMLElement }[] =
    [];
  focused = false;
  constructor(
    private readonly send: (text: string) => boolean,
    private readonly releaseGame: () => void,
  ) {
    this.element.className = "chat-panel";
    this.element.hidden = true;
    this.element.setAttribute("aria-label", "Player chat");
    this.log.className = "chat-messages";
    this.log.setAttribute("role", "log");
    this.log.setAttribute("aria-live", "polite");
    this.log.setAttribute("aria-label", "Recent chat messages");
    this.input.setAttribute("aria-label", "Chat message");
    this.input.placeholder = "Message… Enter to send";
    this.input.maxLength = 256;
    this.input.rows = 2;
    this.form.append(this.input);
    this.form.hidden = true;
    this.error.className = "chat-error";
    this.error.setAttribute("role", "status");
    this.error.hidden = true;
    this.form.onsubmit = (e) => {
      e.preventDefault();
      const text = this.input.value.trim();
      if (!text) return;
      if (new TextEncoder().encode(text).length > 512) {
        this.notice("Message is too long (512 bytes maximum).");
        return;
      }
      if (this.send(text)) {
        this.input.value = "";
        this.error.hidden = true;
      } else this.notice("Chat is not connected yet.");
    };
    this.input.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.code === "Enter" && !e.shiftKey && !e.isComposing) {
        e.preventDefault();
        this.form.requestSubmit();
      }
      if (e.code === "Escape") {
        e.preventDefault();
        this.blur();
      }
    });
    this.input.addEventListener("blur", () => this.blur());
    this.element.append(this.log, this.form, this.error);
  }
  focus(): void {
    this.focused = true;
    this.element.dataset.editing = "true";
    this.form.hidden = false;
    this.releaseGame();
    this.input.focus();
    this.update(performance.now());
    this.log.scrollTop = this.log.scrollHeight;
  }
  blur(): void {
    this.focused = false;
    delete this.element.dataset.editing;
    this.form.hidden = true;
    this.input.blur();
  }
  notice(text: string): void {
    this.error.textContent = text;
    this.error.hidden = false;
  }
  receive(line: ChatLine): void {
    if (this.entries.some((entry) => entry.id === line.id)) return;
    const node = document.createElement("div");
    node.className = "chat-line";
    const name = document.createElement("span");
    name.className = "chat-name";
    name.textContent = `${line.username}: `;
    node.append(name);
    for (const part of chatParts(line.text)) {
      const span = document.createElement(part.style ?? "span");
      span.textContent = part.text;
      node.append(span);
    }
    this.entries.push({ id: line.id, at: performance.now(), node });
    this.log.append(node);
    if (this.entries.length > 32) this.entries.shift()?.node.remove();
    this.update(performance.now());
    this.log.scrollTop = this.log.scrollHeight;
  }
  update(now: number): void {
    this.entries.forEach((entry, i) => {
      const age = now - entry.at;
      entry.node.hidden =
        !this.focused && (age >= 12000 || i < this.entries.length - 6);
      entry.node.style.opacity = this.focused
        ? "1"
        : String(Math.min(1, Math.max(0, (12000 - age) / 2000)));
    });
  }
  clear(): void {
    this.blur();
    this.entries.length = 0;
    this.log.replaceChildren();
    this.input.value = "";
    this.error.hidden = true;
  }
}
