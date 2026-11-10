/**
 * @vitest-environment jsdom
 *
 * `WRK-17`: a chat nobody chose a preset for starts where its place says. The
 * this-chat button says what the next run will hold, asked of the backend, and
 * its menu says where it comes from.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const defaultStyle = vi.hoisted(() => vi.fn());
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  conversationDefaultStyle: defaultStyle,
}));

import ChatSetup from "./ChatSetup";
import { useAppStore } from "../../lib/store";
import type { PresetView } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const PRESETS: PresetView[] = [
  { slug: "ad-hoc", name: "Ad hoc", line: "Just answer", glyph: "ad-hoc", built_in: true, locked: false, version: 1, rules: [] },
  { slug: "lean", name: "Lean", line: "One thing at a time", glyph: "lean", built_in: true, locked: false, version: 1, rules: [] },
];

async function put(style: string | null) {
  await act(async () => {
    useAppStore.setState({
      activeConversationId: "c1",
      conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style }],
      presets: PRESETS,
      turnModifiers: {},
      workspaceMode: false,
    } as never);
    root.render(<ChatSetup />);
  });
  await act(async () => {});
}

beforeEach(() => {
  defaultStyle.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const trigger = () => container.querySelector<HTMLButtonElement>(".chat-setup-btn")!;
const how = () => {
  if (!container.querySelector(".chat-setup-menu")) act(() => trigger().click());
  return container.querySelector<HTMLButtonElement>(".chat-setup-menu [role=menuitem]")!;
};

describe("where a chat starts", () => {
  it("shows the preset its place starts with, and says so", async () => {
    defaultStyle.mockResolvedValue("lean");
    await put(null);
    expect(trigger().title).toContain("How I work: Lean");
    expect(trigger().className).toContain("lit");
    expect(how().title).toContain("Where chats like this start");
    expect(defaultStyle).toHaveBeenCalledWith("c1");
  });

  it("stays Ad hoc, quietly, where nothing is set", async () => {
    defaultStyle.mockResolvedValue(null);
    await put(null);
    expect(trigger().title).toContain("How I work: Ad hoc");
    expect(trigger().className).not.toContain("lit");
  });

  it("the chat's own choice wins and is not asked about", async () => {
    defaultStyle.mockResolvedValue("lean");
    await put("ad-hoc");
    expect(how().title).toContain("Your choice for this chat");
    expect(defaultStyle).not.toHaveBeenCalled();
  });

  it("falls back to Ad hoc when the backend cannot say", async () => {
    defaultStyle.mockRejectedValue("no");
    await put(null);
    expect(trigger().title).toContain("How I work: Ad hoc");
  });
});
