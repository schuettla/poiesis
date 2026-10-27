/**
 * @vitest-environment jsdom
 *
 * `USE-UI-4`: the Inbox shows each message with what the check said, and only
 * what it was sure of. A tag click opens a menu whose pick is a label. A row opens
 * nothing; it offers to start a chat about it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { InboxRow, InboxView, MailAccount } from "../../lib/api";

const mailInbox = vi.fn<(accountId?: string | null) => Promise<InboxView>>();
const mailRetag = vi.fn<(a: string, uid: number, kind: string | null) => Promise<InboxRow>>();
const mailSyncNow = vi.fn<() => Promise<unknown>>();
const stop = vi.fn();
let onEvent: () => void = () => {};
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  mailInbox: (id?: string | null) => mailInbox(id),
  mailRetag: (a: string, uid: number, kind: string | null) => mailRetag(a, uid, kind),
  mailSyncNow: () => mailSyncNow(),
  listenAppEvent: (_name: string, handler: () => void) => {
    onEvent = handler;
    return stop;
  },
}));

import MailInbox from "./MailInbox";
import { row } from "./inbox.fixture";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const askAboutMail = vi.fn();
const setView = vi.fn();
const goToAccounts = vi.fn();

const account = (id: string, label: string, enabled = true): MailAccount => ({
  id,
  label,
  email: `${label.toLowerCase()}@example.com`,
  imap_host: "imap.example.com",
  imap_port: 993,
  smtp_host: "smtp.example.com",
  smtp_port: 465,
  username: `${label.toLowerCase()}@example.com`,
  auth: "password",
  security: "tls",
  enabled,
  created_at: 1,
});
const ONE = [account("a1", "Work")];
const TWO = [account("a1", "Work"), account("a2", "Home")];

const view = (rows: InboxRow[], over: Partial<InboxView> = {}): InboxView => ({
  on: true,
  decision_model: true,
  rows,
  last: { at: new Date(2026, 9, 23, 9, 15).getTime(), fetched: 3, triaged: 3, errors: [] },
  every_minutes: 15,
  syncing: false,
  categories: ["personal", "work", "receipt", "newsletter", "notification"],
  ...over,
});

const ROWS = [
  row({ uid: 3, from: "Ana Ruiz", subject: "Contract draft for Thursday", category: "work", category_p: 0.87, urgency: 4, urgency_p: 0.87, reply: 0.81, unread: true }),
  row({ uid: 2, from: "GitHub", subject: "[poiesis] CI failed on main", category: "notification", category_p: 0.9, urgency: 3, urgency_p: 0.7 }),
  row({ uid: 1, from: "Lieferando", subject: "Ihre Bestellung", category: "receipt", category_p: 0.8, urgency: 1, urgency_p: 0.9 }),
  row({ uid: 4, from: "Nobody Sure", subject: "Update" }),
];

const flush = () => act(async () => {});
const button = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.trim() === label) as HTMLButtonElement | undefined;
const click = (el: Element | undefined) => act(() => (el as HTMLElement).click());
const rowsOnScreen = () => Array.from(container.querySelectorAll(".mail-row"));

/** A fresh screen each time, as opening the tab is. */
async function mount(accounts = ONE, v: InboxView = view(ROWS)) {
  act(() => root.unmount());
  root = createRoot(container);
  mailInbox.mockResolvedValue(v);
  await act(async () => root.render(<MailInbox accounts={accounts} onGoToAccounts={goToAccounts} />));
  await flush();
}

beforeEach(() => {
  // The fixtures are dated 23 Oct 2026, and a time of day only shows for today.
  vi.useFakeTimers({ toFake: ["Date"], now: new Date(2026, 9, 23, 10, 0) });
  mailInbox.mockReset();
  mailRetag.mockReset();
  mailSyncNow.mockReset().mockResolvedValue(null);
  stop.mockReset();
  askAboutMail.mockReset();
  setView.mockReset();
  goToAccounts.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ askAboutMail, setView });
});

afterEach(() => {
  vi.useRealTimers();
  act(() => root.unmount());
  container.remove();
});

describe("a row", () => {
  it("shows the kind, the urgency bar, the reply mark and the sender, subject and time", async () => {
    await mount();
    const first = rowsOnScreen()[0];
    expect(first.querySelector(".mail-from")?.textContent).toBe("Ana Ruiz");
    expect(first.querySelector(".mail-subject-text")?.textContent).toBe("Contract draft for Thursday");
    expect(first.querySelector(".mail-kind")?.textContent).toBe("work");
    expect(first.querySelector(".mail-urgency")?.getAttribute("aria-label")).toBe("Urgency 4 of 4");
    expect(first.querySelector(".mail-urgency")?.getAttribute("title")).toBe("Probably urgent");
    expect(first.querySelectorAll(".mail-urgency-bar.on")).toHaveLength(4);
    expect(first.querySelector(".mail-reply")?.getAttribute("title")).toBe("Probably wants a reply");
    expect(first.classList.contains("unread")).toBe(true);
    expect(first.querySelector(".mail-when")?.textContent).toBe("08:52");
    // Fewer bars for a lower urgency.
    expect(rowsOnScreen()[2].querySelectorAll(".mail-urgency-bar.on")).toHaveLength(1);
  });

  it("shows nothing for a part the model was not sure of: no bar, no tag, no mark", async () => {
    await mount();
    const bare = rowsOnScreen()[3];
    expect(bare.querySelector(".mail-urgency-bar")).toBeNull();
    expect(bare.querySelector(".mail-kind:not(.none)")).toBeNull();
    expect(bare.querySelector(".mail-reply")).toBeNull();
    expect(bare.querySelector(".mail-kind.none")?.textContent).toBe("Set kind");
  });

  it("names a message with no sender or subject instead of leaving a gap", async () => {
    await mount(ONE, view([row({ from: "", subject: "" })]));
    expect(container.querySelector(".mail-from")?.textContent).toBe("(unknown sender)");
    expect(container.querySelector(".mail-subject-text")?.textContent).toBe("(no subject)");
  });
});

describe("the filters", () => {
  it("count what needs a reply and what is urgent, and narrow the list with a count", async () => {
    await mount();
    expect(button("Needs reply1")).toBeDefined();
    expect(button("Urgent2")).toBeDefined();
    expect(container.querySelector(".mail-total")?.textContent).toBe("4 messages");

    click(button("Urgent2"));
    expect(rowsOnScreen().map((r) => r.querySelector(".mail-from")?.textContent)).toEqual(["Ana Ruiz", "GitHub"]);
    expect(container.querySelector(".mail-total")?.textContent).toBe("2 of 4 messages");
    expect(button("Urgent2")?.getAttribute("aria-pressed")).toBe("true");

    click(button("All"));
    expect(rowsOnScreen()).toHaveLength(4);
  });
});

describe("the kind menu", () => {
  it("offers the person's kinds, unsure and no kind, and a pick is sent as a retag", async () => {
    await mount();
    click(rowsOnScreen()[0].querySelector(".mail-kind")!);
    const menu = container.querySelector("[role=menu]")!;
    const items = Array.from(menu.querySelectorAll("[role=menuitemradio],[role=menuitem]")).map((i) => i.textContent?.replace("✓", ""));
    expect(items).toEqual(["personal", "work", "receipt", "newsletter", "notification", "unsure", "No kind"]);
    expect(menu.querySelector("[aria-checked=true]")?.textContent).toContain("work");

    mailRetag.mockResolvedValue({ ...ROWS[0], category: "personal", category_p: null, retagged: true });
    await act(async () => click(Array.from(menu.querySelectorAll("button")).find((b) => b.textContent === "personal")));
    await flush();
    expect(mailRetag).toHaveBeenCalledWith("a1", 3, "personal");
    expect(container.querySelector("[role=menu]")).toBeNull();
    const tag = rowsOnScreen()[0].querySelector(".mail-kind");
    expect(tag?.textContent).toBe("personal");
    expect(tag?.classList.contains("mine")).toBe(true);
    expect(tag?.getAttribute("title")).toBe("Set by you. Click to change it.");
  });

  it("clears a kind with No kind, and a row with none can be given one", async () => {
    await mount();
    click(rowsOnScreen()[0].querySelector(".mail-kind")!);
    mailRetag.mockResolvedValue({ ...ROWS[0], category: null, retagged: true });
    await act(async () => click(button("No kind")));
    expect(mailRetag).toHaveBeenCalledWith("a1", 3, null);

    click(rowsOnScreen()[3].querySelector(".mail-kind.none")!);
    const menu = container.querySelector("[role=menu]")!;
    expect(menu.textContent).not.toContain("No kind");
    expect(menu.querySelector("[aria-checked=true]")).toBeNull();
  });

  it("shows the words of a refused retag and leaves the row as it was", async () => {
    await mount();
    click(rowsOnScreen()[0].querySelector(".mail-kind")!);
    mailRetag.mockRejectedValue(new Error("“invoices” is not one of your kinds."));
    await act(async () => click(button("newsletter")));
    await flush();
    expect(container.textContent).toContain("is not one of your kinds");
    expect(rowsOnScreen()[0].querySelector(".mail-kind")?.textContent).toBe("work");
  });
});

describe("Ask about this", () => {
  it("starts a chat about the message, naming the account only when there are several", async () => {
    await mount();
    click(Array.from(container.querySelectorAll(".mail-ask"))[0]);
    expect(askAboutMail).toHaveBeenLastCalledWith("Ana Ruiz", "Contract draft for Thursday", undefined);

    await mount(TWO);
    click(Array.from(container.querySelectorAll(".mail-ask"))[0]);
    expect(askAboutMail).toHaveBeenLastCalledWith("Ana Ruiz", "Contract draft for Thursday", "Work");
  });
});

describe("the status line", () => {
  it("says when I last looked and that nothing leaves this computer", async () => {
    await mount();
    expect(container.querySelector(".mail-status")?.textContent).toBe(
      "Fetched 09:15 · every 15 minutes · nothing leaves this computer",
    );
  });

  it("looks now on the button, and the button waits while a look is running", async () => {
    await mount();
    const refresh = container.querySelector<HTMLButtonElement>(".mail-refresh")!;
    mailInbox.mockClear();
    await act(async () => click(refresh));
    await flush();
    expect(mailSyncNow).toHaveBeenCalledTimes(1);
    expect(mailInbox).toHaveBeenCalled();

    await mount(ONE, view(ROWS, { syncing: true }));
    expect(container.querySelector<HTMLButtonElement>(".mail-refresh")?.disabled).toBe(true);
  });

  it("shows an account that could not be read", async () => {
    await mount(ONE, view(ROWS, { last: { at: 1, fetched: 0, triaged: 0, errors: ["Work: couldn't reach imap.example.com"] } }));
    expect(container.querySelector(".hw-note.error")?.textContent).toContain("Work: couldn't reach");
  });

  it("reloads when the backend says a look or a check finished, and stops listening on close", async () => {
    await mount();
    mailInbox.mockClear();
    await act(async () => onEvent());
    expect(mailInbox).toHaveBeenCalledTimes(1);
    act(() => root.unmount());
    expect(stop).toHaveBeenCalled();
    root = createRoot(container);
  });
});

describe("the account list", () => {
  it("has a picker only when there are several accounts", async () => {
    await mount(ONE);
    expect(container.querySelector("[aria-label=Account]")).toBeNull();
    await mount(TWO);
    expect(container.querySelector("[aria-label=Account]")).not.toBeNull();
  });

  it("asks for one account when it is picked", async () => {
    await mount(TWO);
    click(container.querySelector("[aria-label=Account]")!);
    mailInbox.mockClear();
    await act(async () => click(Array.from(container.querySelectorAll("[role=option]")).find((o) => o.textContent === "Home")));
    await flush();
    expect(mailInbox).toHaveBeenCalledWith("a2");
  });
});

describe("the empty states", () => {
  it("asks for an account first, with a way to the Accounts tab", async () => {
    await mount([account("a1", "Work", false)]);
    expect(container.textContent).toContain("Add a mail account first.");
    expect(container.querySelector(".mail-row")).toBeNull();
    click(button("Go to Accounts"));
    expect(goToAccounts).toHaveBeenCalled();
    expect(mailInbox).not.toHaveBeenCalled();
  });

  it("says the decision model is missing, with a way to Models, and still lists the mail", async () => {
    await mount(ONE, view(ROWS, { decision_model: false }));
    expect(container.textContent).toContain("Mail triage needs the decision model on this computer");
    click(button("Set it up in Models"));
    expect(setView).toHaveBeenCalledWith("models");
    expect(rowsOnScreen()).toHaveLength(4);
  });

  it("says nothing was fetched yet, instead of an empty list", async () => {
    await mount(ONE, view([], { last: null }));
    expect(container.textContent).toContain("Nothing fetched yet");
    expect(container.querySelector(".mail-list")).toBeNull();
    expect(container.querySelector(".mail-status")?.textContent).toContain("Not fetched yet");
  });
});
