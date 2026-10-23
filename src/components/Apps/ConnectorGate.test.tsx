/**
 * @vitest-environment jsdom
 *
 * `MCG-UI-2`/`MCG-7`: the connector card says what its tools may do, marks each
 * tool R / C / S, and lets a wrong guess be corrected, stricter at once and
 * looser only after saying what it gives up.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ConnectorView, ToolView } from "../../lib/api";

const setToolKind = vi.fn(() => Promise.resolve());
const setConnectorPolicy = vi.fn(() => Promise.resolve());
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  setToolKind: (...a: unknown[]) => (setToolKind as (...x: unknown[]) => unknown)(...a),
  setConnectorPolicy: (...a: unknown[]) => (setConnectorPolicy as (...x: unknown[]) => unknown)(...a),
}));

import { ToolChips, WhatIMayDo, isLoosened, policyLine } from "./ConnectorGate";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

function tool(name: string, kind: ToolView["kind"], auto: ToolView["auto_kind"] = kind): ToolView {
  return { name, description: `${name} does a thing`, input_schema: {}, kind, auto_kind: auto, overridden: kind !== auto };
}

function connector(tools: ToolView[], policy: Partial<ConnectorView["policy"]> = {}): ConnectorView {
  return {
    id: "c1",
    name: "Slack",
    url: "http://x",
    transport: "http",
    enabled: true,
    has_auth: false,
    created_at: 0,
    tools,
    policy: { read_policy: "run", change_policy: "ask", send_policy: "ask", send_cap: 25, sent_today: 3, ...policy },
  };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  setToolKind.mockClear();
  setConnectorPolicy.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const noop = () => {};

describe("what I may do", () => {
  it("reads the defaults in the person's words", () => {
    expect(policyLine(connector([]).policy)).toBe("Read: without asking · Change: ask · Send: ask");
    expect(policyLine(connector([], { send_policy: "allow", send_cap: 10, change_policy: "allow", read_policy: "ask" }).policy)).toBe(
      "Read: ask · Change: always allowed · Send: up to 10 a day",
    );
  });

  it("opens to controls and saves a choice with the rest of the policy intact", async () => {
    act(() => root.render(<WhatIMayDo connector={connector([])} onChanged={noop} onError={noop} />));
    expect(container.querySelector(".gate-body")).toBeNull();
    act(() => container.querySelector<HTMLButtonElement>(".gate-toggle")!.click());
    expect(container.textContent).toContain("3 of 25 used today");
    const allow = Array.from(container.querySelectorAll<HTMLButtonElement>(".seg")).find((b) => b.textContent === "Always allowed")!;
    await act(async () => allow.click());
    expect(setConnectorPolicy).toHaveBeenCalledWith(
      "c1",
      {
        read_policy: "run",
        change_policy: "allow",
        send_policy: "ask",
        send_cap: 25,
      },
      undefined,
    );
  });
});

describe("tool chips", () => {
  const tools = [tool("list_messages", "read"), tool("update_topic", "change"), tool("post_message", "send")];

  function render(c = connector(tools)) {
    act(() => root.render(<ToolChips connector={c} onChanged={noop} onError={noop} />));
  }
  const chip = (name: string) =>
    Array.from(container.querySelectorAll<HTMLButtonElement>(".tool-chip-button")).find((b) => b.textContent?.includes(name))!;
  const menuItem = (text: string) =>
    Array.from(container.querySelectorAll<HTMLButtonElement>(".tool-menu-item")).find((b) => b.textContent?.includes(text))!;

  it("marks each tool R, C or S with the word on hover", () => {
    render();
    expect(chip("list_messages").querySelector(".tool-kind")?.textContent).toBe("R");
    expect(chip("update_topic").querySelector(".tool-kind")?.textContent).toBe("C");
    expect(chip("post_message").querySelector(".tool-kind")?.getAttribute("aria-label")).toBe("sends");
    expect(chip("post_message").title).toContain("sends");
  });

  it("makes a tool stricter in one click", async () => {
    render();
    act(() => chip("list_messages").click());
    await act(async () => menuItem("Treat as send").click());
    expect(setToolKind).toHaveBeenCalledWith("c1", "list_messages", "send", undefined);
    expect(container.querySelector(".confirm-card")).toBeNull();
  });

  it("asks before making a tool looser, and does nothing if declined", async () => {
    render();
    act(() => chip("post_message").click());
    act(() => menuItem("Treat as read").click());
    expect(setToolKind).not.toHaveBeenCalled();
    expect(container.querySelector(".confirm-card")?.textContent).toContain("I'll stop asking before post_message runs.");
    act(() => container.querySelector<HTMLButtonElement>(".confirm-cancel")!.click());
    expect(setToolKind).not.toHaveBeenCalled();
  });

  it("loosens after the person confirms", async () => {
    render();
    act(() => chip("post_message").click());
    act(() => menuItem("Treat as change").click());
    await act(async () => container.querySelector<HTMLButtonElement>(".confirm-go")!.click());
    expect(setToolKind).toHaveBeenCalledWith("c1", "post_message", "change", undefined);
  });

  it("keeps a visible mark on a loosened tool, and going back is stricter so needs no ask", async () => {
    const loosened = tool("post_message", "change", "send");
    expect(isLoosened(loosened)).toBe(true);
    expect(isLoosened(tool("x", "send", "read"))).toBe(false);
    render(connector([loosened]));
    expect(chip("post_message").textContent).toContain("loosened");
    act(() => chip("post_message").click());
    await act(async () => menuItem("Back to what its name says").click());
    expect(setToolKind).toHaveBeenCalledWith("c1", "post_message", null, undefined);
    expect(container.querySelector(".confirm-card")).toBeNull();
  });

  it("shows only eight until asked for the rest", () => {
    const many = Array.from({ length: 11 }, (_, i) => tool(`get_${i}`, "read"));
    render(connector(many));
    expect(container.querySelectorAll(".tool-chip-wrap")).toHaveLength(8);
    const chips = Array.from(container.querySelectorAll<HTMLButtonElement>(".tool-chip-button"));
    act(() => chips[chips.length - 1].click());
    expect(container.querySelectorAll(".tool-chip-wrap")).toHaveLength(11);
  });
});
