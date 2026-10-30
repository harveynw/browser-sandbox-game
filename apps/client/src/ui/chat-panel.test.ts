import { expect, it } from "vitest";
import { chatParts } from "./chat-panel";
it("formats common chat markup while leaving HTML and malformed markup as text", () => {
  expect(chatParts("Hi *bold* _italic_ ~gone~ `code`!")).toEqual([
    { text: "Hi " },
    { text: "bold", style: "strong" },
    { text: " " },
    { text: "italic", style: "em" },
    { text: " " },
    { text: "gone", style: "s" },
    { text: " " },
    { text: "code", style: "code" },
    { text: "!" },
  ]);
  expect(chatParts("<img src=x onerror=alert(1)> *unfinished")).toEqual([
    { text: "<img src=x onerror=alert(1)> *unfinished" },
  ]);
  expect(chatParts("**bold** ~~gone~~\nnext")).toEqual([
    { text: "bold", style: "strong" },
    { text: " " },
    { text: "gone", style: "s" },
    { text: "\nnext" },
  ]);
});
