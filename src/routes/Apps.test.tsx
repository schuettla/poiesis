/**
 * @vitest-environment jsdom
 *
 * The Apps page: Composio and MCP servers each in a tab of their own, and the
 * MCP tab's toolbar, form and server cards.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ComposioStatus, ConnectorView } from "../lib/api";

const listConnectors = vi.fn();
const deleteConnector = vi.fn();
const addConnector = vi.fn();
const composioStatus = vi.fn();
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  listConnectors: (...a: unknown[]) => listConnectors(...a),
  deleteConnector: (...a: unknown[]) => deleteConnector(...a),
  addConnector: (...a: unknown[]) => addConnector(...a),
  composioStatus: (...a: unknown[]) => composioStatus(...a),
  composioAccounts: () => Promise.resolve([]),
  composioAppTools: () => Promise.resolve([]),
  listMcpPrompts: () => Promise.resolve([]),
}));

import Apps from "./Apps";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const status = (over: Partial<ComposioStatus> = {}): ComposioStatus => ({
  configured: true,
  mode: "connect",
  connector_id: "comp",
  enabled: true,
  tool_count: 4,
  checked_at: 1,
  last_error: null,
  accounts: [{ id: "connect:gmail", app: "gmail", app_name: "Gmail", label: null, status: "ACTIVE" }],
  policies: [],
  ...over,
});

const server = (over: Partial<ConnectorView> = {}): ConnectorView => ({
  id: "c1",
  name: "My notes",
  url: "https://notes.example.com/mcp",
  transport: "http",
  enabled: true,
  has_auth: true,
  tools: [],
  created_at: 1,
  policy: { read_policy: "run", change_policy: "ask", send_policy: "ask", send_cap: 25, sent_today: 0 },
  ...over,
});

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  listConnectors.mockReset();
  deleteConnector.mockReset();
  deleteConnector.mockResolvedValue(undefined);
  addConnector.mockReset();
  composioStatus.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const settle = () => act(async () => void (await new Promise((r) => setTimeout(r, 0))));
const tab = (label: string) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>("[role=tab]")).find((b) => b.textContent?.startsWith(label))!;
const button = (label: string) =>
  Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent?.trim() === label);

async function open(connectors: ConnectorView[], s: ComposioStatus = status()) {
  listConnectors.mockResolvedValue([...connectors, server({ id: "comp", name: "Composio", transport: "composio" })]);
  composioStatus.mockResolvedValue(s);
  act(() => root.render(<Apps />));
  await settle();
  await settle();
}

describe("the Apps page", () => {
  it("keeps Composio and MCP servers in two tabs, with a count on each", async () => {
    await open([server()]);
    expect(tab("Composio").textContent).toBe("Composio1");
    expect(tab("MCP servers").textContent).toBe("MCP servers1");
    expect(tab("Composio").getAttribute("aria-selected")).toBe("true");
    expect(container.textContent).toContain("Connected through Composio Connect");
    expect(container.textContent).not.toContain("My notes");

    act(() => tab("MCP servers").click());
    expect(container.textContent).toContain("My notes");
    expect(container.textContent).not.toContain("Connected through Composio Connect");
    // The Composio connector itself is not one of the person's servers.
    const names = Array.from(container.querySelectorAll(".connector-name")).map((n) => n.textContent);
    expect(names).toEqual(["My notes"]);
  });

  it("opens on MCP servers for someone with servers and no Composio key", async () => {
    await open([server()], status({ configured: false, mode: null, accounts: [] }));
    expect(tab("MCP servers").getAttribute("aria-selected")).toBe("true");
  });

  it("shows the add form at once when there are no servers, and behind Add server once there are", async () => {
    await open([]);
    act(() => tab("MCP servers").click());
    expect(container.textContent).toContain("No servers yet");
    expect(container.textContent).toContain("Add a server");
    expect(button("Cancel")).toBeUndefined();

    act(() => root.unmount());
    root = createRoot(container);
    await open([server()]);
    act(() => tab("MCP servers").click());
    expect(container.textContent).not.toContain("Add a server");
    act(() => button("Add server")!.click());
    expect(container.textContent).toContain("Add a server");
    act(() => button("Cancel")!.click());
    expect(container.textContent).not.toContain("Add a server");
  });

  it("says what kind of server each is in words, and asks before removing one", async () => {
    await open([server(), server({ id: "c2", name: "Files", transport: "stdio", has_auth: false, url: "npx files" })]);
    act(() => tab("MCP servers").click());
    const badges = Array.from(container.querySelectorAll(".mcp-badge")).map((b) => b.textContent);
    expect(badges).toEqual(["Remote", "Token saved", "Local"]);
    expect(container.textContent).toContain("2 servers · 2 on");

    act(() => button("Remove")!.click());
    expect(deleteConnector).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("Remove My notes?");
    expect(document.body.textContent).toContain("its saved token is deleted");
    await act(async () => document.querySelector<HTMLButtonElement>(".confirm-go")!.click());
    expect(deleteConnector).toHaveBeenCalledWith("c1");
  });
});
