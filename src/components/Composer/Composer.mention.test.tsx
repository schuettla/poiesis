/**
 * @vitest-environment jsdom
 *
 * `@` in the composer points the agent at a file from the working folder, and the
 * `+` menu's list of earlier conversations. These drive the real input, because
 * what broke before was the wiring: the `+` list filtered on a field that is only
 * filled for the open chat, so it was always empty.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  listFolderFiles: () => Promise.resolve(["README.md", "docs/readme-old.md", "src/main.rs"]),
}));

import Composer from "./Composer";
import { useAppStore } from "../../lib/store";
import type { Attachment, SendOptions } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

let container: HTMLDivElement;
let root: Root;
const sent: { text: string; opts?: SendOptions }[] = [];

const input = () => container.querySelector("input[type=text]") as HTMLInputElement;
const rows = () => Array.from(container.querySelectorAll(".mention-menu [role=option]"));
const flush = () => act(async () => void (await Promise.resolve()));

function type(text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    setter.call(input(), text);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const press = (key: string) =>
  act(() => {
    input().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });

beforeEach(() => {
  sent.length = 0;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    activeConversationId: "c1",
    conversations: [
      { id: "c1", title: "Now", updatedAt: 3, messages: [], folderPath: "C:/proj" },
      // Only the open chat has its messages loaded; the rest arrive empty.
      { id: "c2", title: "Trip to Lisbon", updatedAt: 2, messages: [] },
      { id: "c3", title: "Tax estimate", updatedAt: 1, messages: [] },
    ] as never,
    allArtifacts: [{ id: "a1", title: "Readme draft", kind: "page", content: "x", created_at: 5 }] as never,
    refreshAllArtifacts: async () => {},
  });
  act(() => {
    root.render(
      <Composer onSend={(text: string, _a?: Attachment[], opts?: SendOptions) => sent.push({ text, opts })} />
    );
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("composer `@` mentions", () => {
  it("lists the folder's files and the library, best match first", async () => {
    type("look at @read");
    await flush();
    const titles = rows().map((r) => r.querySelector(".command-menu-title")?.textContent);
    expect(titles).toEqual(["README.md", "readme-old.md", "Readme draft"]);
  });

  it("puts the chosen path in the text and sends it as a file ref", async () => {
    type("look at @read");
    await flush();
    press("Enter");
    expect(input().value).toBe("look at @README.md ");
    expect(sent).toEqual([]);
    press("Enter");
    expect(sent).toHaveLength(1);
    expect(sent[0].text).toBe("look at @README.md");
    expect(sent[0].opts?.refs).toEqual([{ kind: "file", id: "README.md", label: "README.md" }]);
  });

  it("does not send a file whose mention was deleted again", async () => {
    type("look at @read");
    await flush();
    press("Enter");
    type("look at it");
    press("Enter");
    expect(sent[0].text).toBe("look at it");
    expect(sent[0].opts).toBeUndefined();
  });

  it("makes a library item a chip and takes the @word out of the text", async () => {
    type("use @draft");
    await flush();
    press("Enter");
    expect(input().value).toBe("use ");
    expect(container.querySelector(".attachment-chip")?.textContent).toContain("Readme draft");
    press("Enter");
    expect(sent[0].opts?.refs).toEqual([{ kind: "artifact", id: "a1", label: "Readme draft" }]);
  });

  it("leaves an email address and plain Enter alone", async () => {
    type("write to me@home.com");
    await flush();
    expect(rows()).toHaveLength(0);
    press("Enter");
    expect(sent[0].text).toBe("write to me@home.com");
  });

  it("closes on Escape, and Enter then sends what was typed", async () => {
    type("see @read");
    await flush();
    press("Escape");
    expect(rows()).toHaveLength(0);
    press("Enter");
    expect(sent[0].text).toBe("see @read");
  });
});

describe("the + menu's earlier conversations", () => {
  it("lists other chats even though their messages are not loaded", () => {
    act(() => (container.querySelector(".plus-btn") as HTMLElement).click());
    const more = Array.from(container.querySelectorAll(".composer-menu-item")).find((b) =>
      b.textContent?.includes("An earlier conversation")
    ) as HTMLElement;
    act(() => more.click());
    const titles = Array.from(container.querySelectorAll(".composer-submenu .mi-body")).map((b) => b.textContent);
    expect(titles).toEqual(["Trip to Lisbon", "Tax estimate"]);
  });
});
