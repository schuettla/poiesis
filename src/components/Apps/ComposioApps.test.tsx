/**
 * @vitest-environment jsdom
 *
 * `CA-UI-1`/`CA-UI-3`/`CA-UI-4`/`CA-UI-7`: the disclosure comes before the key
 * field, accounts say their state in words, the sign-in card watches and then
 * resumes the chat, and `/connect` opens a sign-in without a model.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppAccount, ComposioStatus } from "../../lib/api";

const composioConnect = vi.fn();
const composioAccounts = vi.fn();
const composioStatus = vi.fn();
const composioDisconnect = vi.fn();
const composioCancel = vi.fn();
const composioRename = vi.fn();
const composioAppTools = vi.fn();
const setToolKind = vi.fn();
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  composioConnect: (...a: unknown[]) => composioConnect(...a),
  composioAccounts: (...a: unknown[]) => composioAccounts(...a),
  composioStatus: (...a: unknown[]) => composioStatus(...a),
  composioDisconnect: (...a: unknown[]) => composioDisconnect(...a),
  composioCancel: (...a: unknown[]) => composioCancel(...a),
  composioRename: (...a: unknown[]) => composioRename(...a),
  composioAppTools: (...a: unknown[]) => composioAppTools(...a),
  setToolKind: (...a: unknown[]) => setToolKind(...a),
  composioCatalog: () => Promise.resolve({ apps: [], partial: false, source: "bundled", note: null, total: 0 }),
}));

import ComposioSection from "./ComposioSection";
import ConnectBlock from "../Blocks/ConnectBlock";
import { HANDLERS, argChoices } from "../../lib/commands";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const account = (over: Partial<AppAccount> = {}): AppAccount => ({
  id: "ca_1",
  app: "gmail",
  app_name: "Gmail",
  label: "work",
  status: "ACTIVE",
  ...over,
});

const status = (over: Partial<ComposioStatus> = {}): ComposioStatus => ({
  configured: true,
  mode: "project",
  connector_id: "conn",
  enabled: true,
  tool_count: 4,
  checked_at: 1,
  last_error: null,
  accounts: [],
  policies: [],
  ...over,
});

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  composioConnect.mockReset();
  composioAccounts.mockReset();
  composioStatus.mockReset();
  composioDisconnect.mockReset();
  composioCancel.mockReset();
  composioCancel.mockResolvedValue(undefined);
  composioRename.mockReset();
  composioRename.mockResolvedValue([]);
  composioAppTools.mockReset();
  composioAppTools.mockResolvedValue([]);
  setToolKind.mockReset();
  setToolKind.mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

const flush = () => act(async () => void (await Promise.resolve()));

describe("the Composio card", () => {
  /** `CAP-UI-T-2`: the plain disclosure is on the page before the key field. */
  it("shows the disclosure before the key field when no key is saved", () => {
    act(() =>
      root.render(<ComposioSection status={status({ configured: false })} reload={() => {}} onError={() => {}} />),
    );
    const text = container.textContent ?? "";
    expect(text).toContain("pass through Composio's servers");
    expect(text).toContain("your own Composio account and key");
    const disclosure = container.querySelector(".composio-disclosure")!;
    const field = container.querySelector("input[type=password]")!;
    expect(disclosure.compareDocumentPosition(field) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  /** `CAP-C-UI-T1`: the `ck_` way comes first, the disclosure before the key field, and `ak_` is a quiet line. */
  it("tells a person where to find their Connect key before the key field, and keeps ak_ as a quiet option", () => {
    act(() =>
      root.render(
        <ComposioSection status={status({ configured: false, mode: null })} reload={() => {}} onError={() => {}} />,
      ),
    );
    const text = container.textContent ?? "";
    expect(text).toContain("For You");
    expect(text).toContain("Sessions & API Key");
    expect(text).toContain("it starts with ck_");
    expect(text).toContain("A project key (ak_) works too");
    const field = container.querySelector("input[type=password]")!;
    const where = Array.from(container.querySelectorAll(".composio-disclosure")).find((p) =>
      (p.textContent ?? "").includes("Sessions & API Key"),
    )!;
    expect(where.compareDocumentPosition(field) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(container.textContent).toContain("Not connected");
  });

  it("in Connect mode lists a single account without a label, and disconnecting asks Composio to remove the app", async () => {
    composioDisconnect.mockResolvedValue([]);
    const accounts = [account({ id: "connect:gmail", label: null })];
    act(() =>
      root.render(
        <ComposioSection status={status({ mode: "connect", accounts })} reload={() => {}} onError={() => {}} />,
      ),
    );
    const text = container.textContent ?? "";
    expect(text).toContain("Connected through Composio Connect · 1 app");
    expect(text).not.toContain("default");
    expect(text).not.toContain("unnamed");
    // `CAP-C6`: a working app can take one more account, and can be named.
    expect(text).toContain("Add account");
    expect(text).toContain("Name");
    expect(text).toContain("once I've used them");
    const remove = Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "Disconnect")!;
    act(() => remove.click());
    expect(document.body.textContent).toContain("I'll ask Composio to remove Gmail.");
    expect(document.body.textContent).toContain("any other client you connected to Gmail through Composio loses it too");
    await act(async () => Array.from(document.querySelectorAll<HTMLButtonElement>(".confirm-go"))[0].click());
    expect(composioDisconnect).toHaveBeenCalledWith("connect:gmail");
  });

  /** `CAP-C6`: two accounts of one app with a Connect key. */
  it("in Connect mode names each of several accounts and removes only the one chosen", async () => {
    composioDisconnect.mockResolvedValue([]);
    const accounts = [
      account({ id: "connect:gmail:gmail_old0", label: null }),
      account({ id: "connect:gmail:gmail_new1", label: "work" }),
    ];
    act(() =>
      root.render(
        <ComposioSection status={status({ mode: "connect", accounts })} reload={() => {}} onError={() => {}} />,
      ),
    );
    const text = container.textContent ?? "";
    expect(text).toContain("unnamed");
    expect(text).toContain("work");
    const disconnects = Array.from(container.querySelectorAll("button")).filter((b) => b.textContent === "Disconnect");
    act(() => disconnects[1].click());
    expect(document.body.textContent).toContain("Disconnect Gmail (work)?");
    expect(document.body.textContent).toContain("Your other Gmail accounts stay.");
    await act(async () => Array.from(document.querySelectorAll<HTMLButtonElement>(".confirm-go"))[0].click());
    expect(composioDisconnect).toHaveBeenCalledWith("connect:gmail:gmail_new1");
  });

  it("in Connect mode names an account in place, at Composio too", async () => {
    const reload = vi.fn();
    const accounts = [account({ id: "connect:gmail", label: null })];
    act(() =>
      root.render(<ComposioSection status={status({ mode: "connect", accounts })} reload={reload} onError={() => {}} />),
    );
    const name = Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "Name")!;
    act(() => name.click());
    const field = container.querySelector<HTMLInputElement>("input[aria-label='Name for this account']")!;
    act(() => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      set.call(field, "personal");
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const save = Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "Save")!;
    await act(async () => save.click());
    expect(composioRename).toHaveBeenCalledWith("connect:gmail", "personal");
    expect(reload).toHaveBeenCalled();
  });

  it("in Connect mode gives up an open sign-in with Cancel, never Disconnect", async () => {
    const accounts = [
      account({ id: "connect:gmail", label: null }),
      account({ id: "connect:gmail:new", label: "work", status: "INITIATED" }),
    ];
    act(() =>
      root.render(<ComposioSection status={status({ mode: "connect", accounts })} reload={() => {}} onError={() => {}} />),
    );
    const buttons = Array.from(container.querySelectorAll("button")).map((b) => b.textContent);
    expect(buttons.filter((t) => t === "Disconnect")).toHaveLength(1);
    const cancel = Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "Cancel")!;
    await act(async () => cancel.click());
    expect(composioCancel).toHaveBeenCalledWith("connect:gmail:new");
  });

  /** `MCG-7` for Composio: the tools of an app I've used can be corrected here. */
  it("shows the tools of an app I've used and corrects one under that app", async () => {
    composioAppTools.mockResolvedValue([
      { name: "GMAIL_FETCH_EMAILS", description: "Used 3 times.", input_schema: null, kind: "read", auto_kind: "read", overridden: false },
    ]);
    const accounts = [account({ id: "connect:gmail", label: null })];
    act(() =>
      root.render(<ComposioSection status={status({ mode: "connect", accounts })} reload={() => {}} onError={() => {}} />),
    );
    await flush();
    expect(composioAppTools).toHaveBeenCalledWith("gmail");
    expect(container.textContent).toContain("Tools I've used:");
    const chip = Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes("GMAIL_FETCH_EMAILS"))!;
    act(() => chip.click());
    const item = Array.from(container.querySelectorAll("[role=menuitemradio]")).find((b) => b.textContent === "Treat as send")!;
    await act(async () => (item as HTMLButtonElement).click());
    expect(setToolKind).toHaveBeenCalledWith("conn", "GMAIL_FETCH_EMAILS", "send", "gmail");
  });

  it("keeps Replace key, Open Composio and Remove key in one Manage menu", () => {
    act(() => root.render(<ComposioSection status={status()} reload={() => {}} onError={() => {}} />));
    const manage = Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.startsWith("Manage"))!;
    act(() => manage.click());
    const items = Array.from(container.querySelectorAll("[role=menuitem]")).map((b) => b.textContent);
    expect(items).toEqual(["Replace key", "Open Composio", "Remove key"]);
  });

  it("reads the connection in words and groups accounts by app", () => {
    const accounts = [
      account(),
      account({ id: "ca_2", label: "personal", status: "EXPIRED" }),
      account({ id: "ca_3", app: "slack", app_name: "Slack", label: "default", status: "INITIATED" }),
    ];
    act(() => root.render(<ComposioSection status={status({ accounts })} reload={() => {}} onError={() => {}} />));
    const text = container.textContent ?? "";
    expect(text).toContain("Connected through Composio · 1 app · 1 account");
    expect(text).toContain("Connected");
    expect(text).toContain("Sign-in expired");
    expect(text).toContain("Waiting for sign-in");
    expect(container.querySelectorAll(".connector-card").length).toBe(2);
    expect(text).toContain("via Composio");
    // An expired sign-in offers Reconnect; a working one does not.
    expect(container.querySelectorAll(".composio-account")[1].textContent).toContain("Reconnect");
    expect(container.querySelectorAll(".composio-account")[0].textContent).not.toContain("Reconnect");
  });

  it("asks before disconnecting, and names the account", async () => {
    composioDisconnect.mockResolvedValue([]);
    act(() => root.render(<ComposioSection status={status({ accounts: [account()] })} reload={() => {}} onError={() => {}} />));
    const disconnect = Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "Disconnect")!;
    act(() => disconnect.click());
    expect(composioDisconnect).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("Disconnect Gmail (work)?");
    const confirm = Array.from(document.querySelectorAll<HTMLButtonElement>(".confirm-go"))[0];
    await act(async () => confirm.click());
    expect(composioDisconnect).toHaveBeenCalledWith("ca_1");
  });
});

describe("the sign-in card", () => {
  const block = (state?: unknown, data: Record<string, unknown> = {}) => ({
    id: "b1",
    kind: "connect" as const,
    title: "Sign in to Gmail",
    data: { app: "gmail", name: "Gmail", label: null, state: "waiting", ...data },
    state,
  });

  beforeEach(() => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    composioStatus.mockResolvedValue(status({ accounts: [] }));
  });

  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("signs in on a click, keeps the account and never the address", async () => {
    const setBlockState = vi.fn();
    useAppStore.setState({ setBlockState, sendBlockAction: vi.fn() });
    composioConnect.mockResolvedValue(account({ status: "INITIATED" }));
    act(() => root.render(<ConnectBlock block={block()} />));
    await flush();
    expect(container.textContent).toContain("I need your Gmail for this");
    await act(async () => container.querySelector<HTMLButtonElement>(".btn-primary")!.click());
    expect(composioConnect).toHaveBeenCalledWith("gmail", undefined);
    const saved = setBlockState.mock.calls[0][1];
    expect(saved).toMatchObject({ phase: "signing", accountId: "ca_1", label: "work" });
    expect(JSON.stringify(saved)).not.toContain("http");
  });

  it("asks for a name when the app already has an account", async () => {
    composioStatus.mockResolvedValue(status({ accounts: [account({ label: "personal", status: "EXPIRED" })] }));
    useAppStore.setState({ setBlockState: vi.fn(), sendBlockAction: vi.fn() });
    composioConnect.mockResolvedValue(account({ status: "INITIATED", label: "work" }));
    act(() => root.render(<ConnectBlock block={block()} />));
    await flush();
    const input = container.querySelector<HTMLInputElement>("input.field-input")!;
    expect(input.value).toBe("work");
    await act(async () => container.querySelector<HTMLButtonElement>(".btn-primary")!.click());
    expect(composioConnect).toHaveBeenCalledWith("gmail", "work");
  });

  /** The card turns to "Connected" and the chat carries on, once. */
  it("watches the sign-in and resumes the conversation when it finishes", async () => {
    const setBlockState = vi.fn();
    const sendBlockAction = vi.fn();
    useAppStore.setState({ setBlockState, sendBlockAction });
    composioAccounts.mockResolvedValue([account()]);
    act(() =>
      root.render(
        <ConnectBlock block={block({ phase: "signing", accountId: "ca_1", label: "work", startedAt: Date.now() })} />,
      ),
    );
    await flush();
    await flush();
    expect(setBlockState).toHaveBeenCalledWith("b1", expect.objectContaining({ phase: "connected", label: "work" }));
    expect(sendBlockAction).toHaveBeenCalledTimes(1);
    expect(sendBlockAction.mock.calls[0][1]).toBe("Gmail is connected (work).");
  });

  it("gives up after ten minutes and offers to try again", async () => {
    const setBlockState = vi.fn();
    useAppStore.setState({ setBlockState, sendBlockAction: vi.fn() });
    composioAccounts.mockResolvedValue([account({ status: "INITIATED" })]);
    act(() =>
      root.render(
        <ConnectBlock block={block({ phase: "signing", accountId: "ca_1", startedAt: Date.now() - 11 * 60 * 1000 })} />,
      ),
    );
    await flush();
    expect(setBlockState).toHaveBeenCalledWith("b1", expect.objectContaining({ phase: "failed" }));
  });

  /** `CAP-C-UI-T2` */
  it("in Connect mode asks for no name, and an app that is already in flips the card at once", async () => {
    const setBlockState = vi.fn();
    const sendBlockAction = vi.fn();
    useAppStore.setState({ setBlockState, sendBlockAction });
    composioStatus.mockResolvedValue(
      status({ mode: "connect", accounts: [account({ id: "connect:gmail", label: null, status: "EXPIRED" })] }),
    );
    composioConnect.mockResolvedValue(account({ id: "connect:gmail", label: null, status: "ACTIVE" }));
    act(() => root.render(<ConnectBlock block={block()} />));
    await flush();
    expect(container.querySelector("input.field-input")).toBeNull();
    await act(async () => container.querySelector<HTMLButtonElement>(".btn-primary")!.click());
    expect(composioConnect).toHaveBeenCalledWith("gmail", undefined);
    expect(setBlockState).toHaveBeenCalledWith("b1", expect.objectContaining({ phase: "connected", label: null }));
    expect(sendBlockAction.mock.calls[0][1]).toBe("Gmail is connected.");
  });

  it("shows Connected without a name when there is none", () => {
    act(() => root.render(<ConnectBlock block={block({ phase: "connected", label: null })} />));
    expect(container.textContent).toContain("Connected.");
    expect(container.textContent).not.toContain("Connected as");
  });

  it("opens the same sign-in again with one call, and cancelling drops it", async () => {
    const setBlockState = vi.fn();
    useAppStore.setState({ setBlockState, sendBlockAction: vi.fn() });
    composioStatus.mockResolvedValue(status({ mode: "connect" }));
    composioAccounts.mockResolvedValue([account({ id: "connect:gmail", status: "INITIATED" })]);
    composioConnect.mockResolvedValue(account({ id: "connect:gmail", label: null, status: "INITIATED" }));
    act(() =>
      root.render(
        <ConnectBlock block={block({ phase: "signing", accountId: "connect:gmail", startedAt: Date.now() })} />,
      ),
    );
    await flush();
    const again = Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "Open again")!;
    await act(async () => again.click());
    expect(composioConnect).toHaveBeenCalledTimes(1);
    const cancel = Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "Cancel")!;
    await act(async () => cancel.click());
    expect(composioCancel).toHaveBeenCalledWith("connect:gmail");
    // The watch asked about this app only.
    expect(composioAccounts).toHaveBeenCalledWith(true, ["gmail"]);
  });

  it("says what to do when there is no key, and offers no sign-in", async () => {
    composioStatus.mockResolvedValue(status({ configured: false }));
    act(() => root.render(<ConnectBlock block={block()} />));
    await flush();
    expect(container.textContent).toContain("once you've added a Composio key");
    expect(container.querySelector(".btn-primary")).toBeNull();
  });
});

describe("/connect", () => {
  beforeEach(() => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  /** `CAP-UI-T-1`: the apps already in come first. */
  it("offers a connected app that is not on the common list", () => {
    useAppStore.setState({ appAccounts: [account({ app: "hackernews", app_name: "Hacker News" })] });
    const choices = argChoices("apps", "", useAppStore.getState());
    expect(choices[0]).toMatchObject({ value: "hackernews", label: "Hacker News" });
  });

  it("lists connected apps first", () => {
    useAppStore.setState({ appAccounts: [account({ app: "slack", app_name: "Slack" })] });
    const choices = argChoices("apps", "", useAppStore.getState());
    expect(choices[0].value).toBe("slack");
    expect(choices[0].hint).toContain("connected");
    expect(choices.some((c) => c.value === "gmail")).toBe(true);
  });

  it("starts a sign-in without a model call", async () => {
    composioStatus.mockResolvedValue(status());
    composioConnect.mockResolvedValue(account({ status: "INITIATED" }));
    const result = await HANDLERS.connect("gmail work", useAppStore.getState());
    expect(composioConnect).toHaveBeenCalledWith("gmail", "work");
    expect(result.kind).toBe("done");
  });

  it("explains itself instead of connecting when no key is saved", async () => {
    composioStatus.mockResolvedValue(status({ configured: false }));
    const result = await HANDLERS.connect("gmail", useAppStore.getState());
    expect(composioConnect).not.toHaveBeenCalled();
    expect(result).toMatchObject({ kind: "error" });
  });

  it("refuses a name it cannot use as an app", async () => {
    const result = await HANDLERS.connect("../etc", useAppStore.getState());
    expect(composioConnect).not.toHaveBeenCalled();
    expect(result.kind).toBe("error");
  });
});
