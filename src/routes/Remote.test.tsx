/**
 * @vitest-environment jsdom
 *
 * The Remote tab (`REM-UI-1`): the switch and its plain line, the pairing dialog, and the paired
 * phones with a confirmation before one is removed.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RemoteSender, RemoteStatus, RemoteTelegramStatus } from "../lib/api";

const remoteTelegramStatus = vi.fn();
const remoteTelegramConnect = vi.fn();
const remoteTelegramDisconnect = vi.fn();
const remoteTelegramCode = vi.fn();
const remoteStatus = vi.fn();
const remoteSetEnabled = vi.fn();
const remoteNewCode = vi.fn();
const remoteRemoveSender = vi.fn();
const remoteCancelCode = vi.fn();
const appEvents: Array<() => void> = [];
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  listenAppEvent: (_name: string, cb: () => void) => {
    appEvents.push(cb);
    return () => {
      appEvents.splice(appEvents.indexOf(cb), 1);
    };
  },
  remoteTelegramStatus: (...a: unknown[]) => remoteTelegramStatus(...a),
  remoteTelegramConnect: (...a: unknown[]) => remoteTelegramConnect(...a),
  remoteTelegramDisconnect: (...a: unknown[]) => remoteTelegramDisconnect(...a),
  remoteTelegramCode: (...a: unknown[]) => remoteTelegramCode(...a),
  remoteStatus: (...a: unknown[]) => remoteStatus(...a),
  remoteSetEnabled: (...a: unknown[]) => remoteSetEnabled(...a),
  remoteNewCode: (...a: unknown[]) => remoteNewCode(...a),
  remoteRemoveSender: (...a: unknown[]) => remoteRemoveSender(...a),
  remoteCancelCode: (...a: unknown[]) => remoteCancelCode(...a),
}));

import Remote from "./Remote";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const phone = (over: Partial<RemoteSender> = {}): RemoteSender => ({
  transport: "phone",
  id: "k1",
  label: "Pixel",
  paired_at: 1,
  last_seen: Date.now() - 5 * 60_000,
  level: "chat",
  ...over,
});
// Built in two pieces so the Icons test, which looks for stray inline drawings, does not take this string for one.
const QR = "<" + 'svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"></svg>';
const status = (over: Partial<RemoteStatus> = {}): RemoteStatus => ({ enabled: false, connected: false, senders: [], ...over });
const bot = (over: Partial<RemoteTelegramStatus> = {}): RemoteTelegramStatus => ({
  token_hint: null,
  bot: null,
  error: null,
  polling: false,
  ...over,
});
const account = (over: Partial<RemoteSender> = {}): RemoteSender => phone({ transport: "telegram", id: "77", label: "Erich", ...over });

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  for (const m of [
    remoteStatus,
    remoteSetEnabled,
    remoteNewCode,
    remoteRemoveSender,
    remoteCancelCode,
    remoteTelegramStatus,
    remoteTelegramConnect,
    remoteTelegramDisconnect,
    remoteTelegramCode,
  ])
    m.mockReset();
  remoteTelegramStatus.mockResolvedValue(bot());
  remoteRemoveSender.mockResolvedValue(true);
  remoteCancelCode.mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const settle = () => act(async () => void (await new Promise((r) => setTimeout(r, 0))));
const button = (label: string) =>
  Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent?.trim() === label);

async function open(s: RemoteStatus) {
  remoteStatus.mockResolvedValue(s);
  act(() => root.render(<Remote />));
  await settle();
}

describe("the Remote tab", () => {
  it("says nothing outside can reach me while the switch is off, and keeps pairing shut", async () => {
    await open(status());
    expect(container.textContent).toContain("Off. Nothing outside can reach me.");
    expect(container.querySelector<HTMLInputElement>("input[role=switch]")!.checked).toBe(false);
    expect(button("Pair a phone")!.disabled).toBe(true);
    expect(container.textContent).toContain("No phone is paired.");
  });

  it("turns remote on from the switch and says who can reach me", async () => {
    await open(status());
    remoteSetEnabled.mockResolvedValue(status({ enabled: true, senders: [phone(), phone({ id: "k2", label: "Tablet" })] }));
    act(() => container.querySelector<HTMLInputElement>("input[role=switch]")!.click());
    await settle();
    expect(remoteSetEnabled).toHaveBeenCalledWith(true);
    expect(container.textContent).toContain("On. I am reachable by 2 paired senders.");
    expect(container.textContent).toContain("Pixel");
    expect(container.textContent).toContain("Tablet");
    expect(container.textContent).toContain("Seen 5 minutes ago");
    expect(button("Pair a phone")!.disabled).toBe(false);
  });

  it("shows the one-time code and its countdown in a dialog", async () => {
    await open(status({ enabled: true }));
    remoteNewCode.mockResolvedValue({ code: "ABCD2345", expires_at: Date.now() + 300_000, svg: QR, text: "{}" });
    act(() => button("Pair a phone")!.click());
    await settle();
    const dialog = document.querySelector("[role=dialog]")!;
    expect(dialog.textContent).toContain("ABCD 2345");
    expect(dialog.textContent).toMatch(/Works for \d:\d\d more, once\./);
    // A new code replaces it.
    remoteNewCode.mockResolvedValue({ code: "WXYZ6789", expires_at: Date.now() + 300_000, svg: QR, text: "{}" });
    act(() => button("New code")!.click());
    await settle();
    expect(document.querySelector("[role=dialog]")!.textContent).toContain("WXYZ 6789");
    expect(remoteCancelCode).not.toHaveBeenCalled();
    act(() => button("Done")!.click());
    expect(document.querySelector("[role=dialog]")).toBeNull();
    // Off the screen, the code stops working.
    expect(remoteCancelCode).toHaveBeenCalledTimes(1);
  });

  it("shows the QR code as a picture the phone can scan, and offers the text to copy", async () => {
    await open(status({ enabled: true }));
    remoteNewCode.mockResolvedValue({ code: "ABCD2345", expires_at: Date.now() + 300_000, svg: QR, text: "{\"v\":1}" });
    act(() => button("Pair a phone")!.click());
    await settle();
    const img = document.querySelector<HTMLImageElement>(".remote-qr img")!;
    expect(img.src).toContain("data:image/svg+xml");
    expect(button("Can't scan? Copy the pairing text")).toBeTruthy();
  });

  it("tells the person when a phone pairs while the dialog is open", async () => {
    await open(status({ enabled: true }));
    remoteNewCode.mockResolvedValue({ code: "ABCD2345", expires_at: Date.now() + 300_000, svg: QR, text: "{}" });
    act(() => button("Pair a phone")!.click());
    await settle();
    expect(document.querySelector("[role=dialog]")!.textContent).not.toContain("is paired");
    // The desktop tells the window a link changed, and the list now has the phone.
    remoteStatus.mockResolvedValue(status({ enabled: true, senders: [phone()] }));
    act(() => appEvents.forEach((cb) => cb()));
    await settle();
    const text = document.querySelector("[role=dialog]")!.textContent!;
    expect(text).toContain("Your phone is paired.");
    expect(text).not.toContain("ABCD 2345");
    expect(button("New code")!.disabled).toBe(true);
  });

  it("says why there is no code when the gateway refuses one", async () => {
    await open(status({ enabled: true }));
    remoteNewCode.mockRejectedValue("Turn remote on first.");
    act(() => button("Pair a phone")!.click());
    await settle();
    expect(document.querySelector("[role=dialog]")!.textContent).toContain("Turn remote on first.");
  });

  it("asks before removing a phone, and removes it only on yes", async () => {
    await open(status({ enabled: true, senders: [phone()] }));
    act(() => button("Remove")!.click());
    expect(document.querySelector("[role=alertdialog]")!.textContent).toContain("Remove Pixel?");
    expect(remoteRemoveSender).not.toHaveBeenCalled();
    // Cancel: nothing happens.
    act(() => button("Cancel")!.click());
    expect(remoteRemoveSender).not.toHaveBeenCalled();
    // Yes: it goes.
    act(() => button("Remove")!.click());
    act(() => document.querySelector<HTMLButtonElement>(".confirm-go")!.click());
    await settle();
    expect(remoteRemoveSender).toHaveBeenCalledWith("phone", "k1");
  });
});

describe("the Telegram block", () => {
  const typeToken = (value: string) => {
    const input = container.querySelector<HTMLInputElement>("input[aria-label='Telegram bot token']")!;
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    act(() => {
      set.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };

  it("starts with a token field, says it is not connected, and keeps pairing shut", async () => {
    await open(status({ enabled: true }));
    expect(container.textContent).toContain("Not connected.");
    expect(container.querySelector<HTMLInputElement>("input[aria-label='Telegram bot token']")!.type).toBe("password");
    expect(button("Pair an account")!.disabled).toBe(true);
    expect(container.textContent).toContain("Messages to the Telegram bot pass through Telegram.");
    expect(container.textContent).toContain("No account is paired.");
  });

  it("shows Telegram's own words when the token is refused", async () => {
    await open(status({ enabled: true }));
    remoteTelegramConnect.mockRejectedValue("Unauthorized");
    typeToken("123:bad");
    act(() => button("Connect")!.click());
    await settle();
    expect(remoteTelegramConnect).toHaveBeenCalledWith("123:bad");
    expect(container.textContent).toContain("Unauthorized");
    expect(container.querySelector("input[aria-label='Telegram bot token']")).not.toBeNull();
  });

  it("connects, shows the bot and only the end of the token, and offers Disconnect", async () => {
    await open(status({ enabled: true }));
    remoteTelegramConnect.mockResolvedValue(bot({ token_hint: "...x9Z4", bot: "@poiesis_bot", polling: true }));
    typeToken("987:secretsecretx9Z4");
    act(() => button("Connect")!.click());
    await settle();
    expect(container.textContent).toContain("Connected as @poiesis_bot.");
    expect(container.textContent).toContain("Token ...x9Z4.");
    expect(container.textContent).not.toContain("secretsecret");
    expect(container.querySelector("input[aria-label='Telegram bot token']")).toBeNull();
    expect(button("Pair an account")!.disabled).toBe(false);
    remoteTelegramDisconnect.mockResolvedValue(bot());
    act(() => button("Disconnect")!.click());
    await settle();
    expect(container.textContent).toContain("Not connected.");
  });

  it("says what Telegram said when the bot cannot listen", async () => {
    remoteTelegramStatus.mockResolvedValue(bot({ token_hint: "...x9Z4", bot: "@poiesis_bot", error: "Conflict: another poller" }));
    await open(status({ enabled: true }));
    expect(container.textContent).toContain("Conflict: another poller");
  });

  it("lists paired accounts apart from phones, and asks before removing one", async () => {
    remoteTelegramStatus.mockResolvedValue(bot({ token_hint: "...x9Z4", bot: "@poiesis_bot", polling: true }));
    await open(status({ enabled: true, senders: [phone(), account()] }));
    const rows = Array.from(container.querySelectorAll("section[aria-label=Telegram] .remote-phone"));
    expect(rows.map((r) => r.textContent)).toEqual([expect.stringContaining("Erich")]);
    act(() => (rows[0].querySelector("button") as HTMLButtonElement).click());
    expect(document.querySelector("[role=alertdialog]")!.textContent).toContain("Remove Erich?");
    expect(remoteRemoveSender).not.toHaveBeenCalled();
    act(() => document.querySelector<HTMLButtonElement>(".confirm-go")!.click());
    await settle();
    expect(remoteRemoveSender).toHaveBeenCalledWith("telegram", "77");
  });

  it("shows the code to send to the bot, and says so when an account pairs", async () => {
    remoteTelegramStatus.mockResolvedValue(bot({ token_hint: "...x9Z4", bot: "@poiesis_bot", polling: true }));
    await open(status({ enabled: true }));
    remoteTelegramCode.mockResolvedValue({ code: "ABCD2345", expires_at: Date.now() + 300_000 });
    act(() => button("Pair an account")!.click());
    await settle();
    const dialog = document.querySelector("[role=dialog]")!;
    expect(dialog.textContent).toContain("Send this code to @poiesis_bot in Telegram.");
    expect(dialog.textContent).toContain("ABCD 2345");
    expect(dialog.textContent).toMatch(/Works for \d:\d\d more, once\./);
    remoteStatus.mockResolvedValue(status({ enabled: true, senders: [account()] }));
    act(() => appEvents.forEach((cb) => cb()));
    await settle();
    expect(document.querySelector("[role=dialog]")!.textContent).toContain("Your account is paired.");
    act(() => button("Done")!.click());
    expect(remoteCancelCode).toHaveBeenCalledTimes(1);
  });
});
