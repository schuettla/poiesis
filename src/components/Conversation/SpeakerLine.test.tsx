/**
 * @vitest-environment jsdom
 *
 * `TEAM-3`: the name line appears where the speaker changes and nowhere else, and
 * a handover is one plain sentence in the chat.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import SpeakerLine, { speakerChanges } from "./SpeakerLine";
import CommandNote from "./CommandNote";
import { useAppStore } from "../../lib/store";
import type { Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => useAppStore.setState({ personas: [{ id: "p1", name: "Mira" }] } as never));
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const reply = (id: string, author?: string): Message => ({ id, role: "assistant", text: "ok", author }) as Message;
const ask = (id: string): Message => ({ id, role: "user", text: "hi" }) as Message;

describe("where the speaker changes", () => {
  it("is nowhere in a chat with one leader", () => {
    const chat = [ask("u1"), reply("a1", "persona:p1"), ask("u2"), reply("a2", "persona:p1")];
    expect([...speakerChanges(chat)]).toEqual([]);
    expect([...speakerChanges([ask("u1"), reply("a1", "poiesis"), ask("u2"), reply("a2", "poiesis")])]).toEqual([]);
  });

  it("is the first reply after a handover, and again when it is handed back", () => {
    const chat = [ask("u1"), reply("a1", "poiesis"), ask("u2"), reply("a2", "persona:p1"), ask("u3"), reply("a3", "persona:p1"), ask("u4"), reply("a4", "poiesis")];
    expect([...speakerChanges(chat)]).toEqual(["a2", "a4"]);
  });

  it("is a message that someone else led, and the one after it", () => {
    const chat = [ask("u1"), reply("a1", "persona:p1"), ask("u2"), reply("a2", "persona:p2"), ask("u3"), reply("a3", "persona:p1")];
    expect([...speakerChanges(chat)]).toEqual(["a2", "a3"]);
  });

  it("ignores replies from before authors were kept, and the first reply of a chat", () => {
    expect([...speakerChanges([ask("u1"), reply("a1", "persona:p1")])]).toEqual([]);
    expect([...speakerChanges([ask("u1"), reply("a1"), ask("u2"), reply("a2", "persona:p1")])]).toEqual([]);
  });
});

describe("the name line", () => {
  it("shows the persona's face and name", () => {
    act(() => root.render(<SpeakerLine author="persona:p1" />));
    expect(container.querySelector(".speaker-line")?.textContent).toContain("Mira");
    expect(container.querySelector(".persona-glyph")?.textContent).toBe("M");
  });

  it("shows Poiesis, and says nothing for a persona that is gone", () => {
    act(() => root.render(<SpeakerLine author="poiesis" />));
    expect(container.querySelector(".speaker-line")?.textContent).toContain("Poiesis");
    act(() => root.render(<SpeakerLine author="persona:gone" />));
    expect(container.querySelector(".speaker-line")).toBeNull();
  });
});

describe("the handover line", () => {
  it("reads as the sentence it is, not as a typed command", () => {
    const note = { id: "n1", name: "lead", args: "mira", by: "user", outcome: "done", note: "Mira leads this chat from here.", at: 1 } as never;
    act(() => root.render(<CommandNote note={note} />));
    const text = container.querySelector(".command-note")?.textContent ?? "";
    expect(text).toContain("Mira leads this chat from here.");
    expect(text).not.toContain("/lead");
  });
});
