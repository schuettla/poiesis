/**
 * @vitest-environment jsdom
 *
 * `USE-UI-4`: Mail has an Inbox tab only while Mail triage is on. Off, the page
 * is the account list alone, as it was, with no editor of kinds.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { InboxView, MailAccount } from "../lib/api";

const account: MailAccount = {
  id: "a1",
  label: "Work",
  email: "work@example.com",
  imap_host: "imap.example.com",
  imap_port: 993,
  smtp_host: "smtp.example.com",
  smtp_port: 465,
  username: "work@example.com",
  auth: "password",
  security: "tls",
  enabled: true,
  created_at: 1,
};
const emptyInbox: InboxView = {
  on: true,
  decision_model: true,
  rows: [],
  last: null,
  every_minutes: 15,
  syncing: false,
  categories: ["personal", "work"],
};
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  listMailAccounts: () => Promise.resolve([account]),
  mailInbox: () => Promise.resolve(emptyInbox),
  mailCategories: () => Promise.resolve({ categories: ["personal", "work"], defaults: ["personal", "work"] }),
  listenAppEvent: () => () => {},
}));

import Mail from "./Mail";
import { useAppStore } from "../lib/store";
import { experiment } from "../lib/experiments.fixture";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const flush = () => act(async () => {});
const tab = (label: string) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>("[role=tab]")).find((t) => t.textContent === label);

async function mount(triage: boolean | null) {
  act(() =>
    useAppStore.setState({
      experiments: triage === null ? null : [experiment({ id: "mail-triage", title: "Mail triage", on: triage })],
    }),
  );
  await act(async () => root.render(<Mail />));
  await flush();
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the Mail tabs", () => {
  it("are not there while Mail triage is off, and the page is the account list", async () => {
    await mount(false);
    expect(container.querySelector("[role=tablist]")).toBeNull();
    expect(container.textContent).toContain("work@example.com");
    expect(container.querySelector(".mail-kinds")).toBeNull();
  });

  it("are not there before the experiments have been read", async () => {
    await mount(null);
    expect(container.querySelector("[role=tablist]")).toBeNull();
  });

  it("are Accounts and Inbox while it is on, opening on Accounts with the kinds editor under the list", async () => {
    await mount(true);
    expect(tab("Accounts")?.getAttribute("aria-selected")).toBe("true");
    expect(tab("Inbox")?.getAttribute("aria-selected")).toBe("false");
    expect(container.textContent).toContain("work@example.com");
    expect(container.querySelector(".mail-kinds")).not.toBeNull();
  });

  it("show the Inbox on its tab, and not the account list or the editor", async () => {
    await mount(true);
    await act(async () => tab("Inbox")!.click());
    await flush();
    expect(tab("Inbox")?.getAttribute("aria-selected")).toBe("true");
    expect(container.querySelector("[aria-label=Inbox]")).not.toBeNull();
    expect(container.textContent).not.toContain("work@example.com");
    expect(container.querySelector(".mail-kinds")).toBeNull();
  });

  it("fall back to the account list when Mail triage is turned off with the Inbox open", async () => {
    await mount(true);
    await act(async () => tab("Inbox")!.click());
    await flush();
    await mount(false);
    expect(container.querySelector("[role=tablist]")).toBeNull();
    expect(container.textContent).toContain("work@example.com");
  });
});
