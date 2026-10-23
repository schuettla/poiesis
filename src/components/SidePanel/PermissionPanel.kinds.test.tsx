/**
 * @vitest-environment jsdom
 *
 * `MCG-7` for Composio: an approval names each app tool it runs with what it is
 * taken to do, and a wrong guess is corrected from there, under that app.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppTool, PermissionRequest } from "../../lib/api";

const setToolKind = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  setToolKind: (...a: unknown[]) => setToolKind(...a),
}));

import PermissionPanel from "./PermissionPanel";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  setToolKind.mockReset();
  setToolKind.mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState({ pendingPermissions: [] });
});

const tool = (over: Partial<AppTool> = {}): AppTool => ({
  connector_id: "comp",
  app: "gmail",
  app_name: "Gmail",
  name: "GMAIL_CREATE_EMAIL_DRAFT",
  kind: "change",
  auto_kind: "change",
  overridden: false,
  ...over,
});

function show(tools?: AppTool[]) {
  const r: PermissionRequest = {
    id: "perm_1",
    summary: "I'd like to change something in Gmail and Slack: create email draft; list channels",
    path: "Gmail and Slack",
    mode: "read",
    in_folder: false,
    capability: "app",
    call_kind: "change",
    diff: JSON.stringify({ to: "ana@example.com" }),
    tools,
  };
  useAppStore.setState({ pendingPermissions: [r] });
  act(() => root.render(<PermissionPanel />));
}

const chip = (name: string) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>(".tool-chip")).find((b) => b.textContent?.includes(name))!;
const item = (label: string) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>("[role=menuitemradio]")).find((b) => b.textContent === label)!;

describe("correcting a tool's kind from an approval", () => {
  it("names each tool under its app, marked with what it does", () => {
    show([tool(), tool({ app: "slack", app_name: "Slack", name: "SLACK_LIST_CHANNELS", kind: "read", auto_kind: "read" })]);
    const text = container.querySelector(".permission-kinds")?.textContent ?? "";
    expect(text).toContain("Gmail:");
    expect(text).toContain("Slack:");
    expect(chip("GMAIL_CREATE_EMAIL_DRAFT").querySelector(".tool-kind")?.textContent).toBe("C");
    expect(chip("SLACK_LIST_CHANNELS").querySelector(".tool-kind")?.textContent).toBe("R");
    expect(text).toContain("Click it to correct me");
  });

  it("makes a tool stricter at once, under its app, and says it counts from next time", async () => {
    show([tool()]);
    act(() => chip("GMAIL_CREATE_EMAIL_DRAFT").click());
    await act(async () => item("Treat as send").click());
    expect(setToolKind).toHaveBeenCalledWith("comp", "GMAIL_CREATE_EMAIL_DRAFT", "send", "gmail");
    expect(chip("GMAIL_CREATE_EMAIL_DRAFT").querySelector(".tool-kind")?.textContent).toBe("S");
    expect(container.textContent).toContain("It applies from the next time.");
    // The question itself is unchanged: still a change, still its buttons.
    expect(container.querySelector(".side-panel.outbound")).toBeNull();
  });

  it("asks before loosening one", async () => {
    show([tool({ name: "GMAIL_SEND_EMAIL", kind: "send", auto_kind: "send" })]);
    act(() => chip("GMAIL_SEND_EMAIL").click());
    act(() => item("Treat as read").click());
    expect(setToolKind).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("I'll stop asking before GMAIL_SEND_EMAIL runs.");
  });

  it("shows nothing extra for a request without tools", () => {
    show(undefined);
    expect(container.querySelector(".permission-kinds")).toBeNull();
  });
});
