/**
 * @vitest-environment jsdom
 *
 * `USE-UI-4`: the kinds mail is sorted into are the person's own. Add one, take
 * one away, go back to the default list; at most eight, and never none.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MailCategories } from "../../lib/api";

const DEFAULTS = ["personal", "work", "receipt", "newsletter", "notification"];
const mailCategories = vi.fn<() => Promise<MailCategories>>();
const setMailCategories = vi.fn<(list: string[]) => Promise<MailCategories>>();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  mailCategories: () => mailCategories(),
  setMailCategories: (list: string[]) => setMailCategories(list),
}));

import KindEditor from "./KindEditor";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const flush = () => act(async () => {});
const chips = () => Array.from(container.querySelectorAll(".mail-kind-chip span:first-child")).map((c) => c.textContent);
const button = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.trim() === label) as HTMLButtonElement | undefined;
const input = () => container.querySelector<HTMLInputElement>("input")!;
const type = (value: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input(), value);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });

/** A fresh screen each time, as opening the tab is. */
async function mount(categories = DEFAULTS) {
  act(() => root.unmount());
  root = createRoot(container);
  mailCategories.mockResolvedValue({ categories, defaults: DEFAULTS });
  await act(async () => root.render(<KindEditor />));
  await flush();
}

beforeEach(() => {
  mailCategories.mockReset();
  setMailCategories.mockReset().mockImplementation((list) =>
    Promise.resolve({ categories: list.length ? list : DEFAULTS, defaults: DEFAULTS }),
  );
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the kinds editor", () => {
  it("lists the kinds, says unsure is always there, and offers no way back while it is the default list", async () => {
    await mount();
    expect(chips()).toEqual(DEFAULTS);
    expect(container.textContent).toContain('"unsure" when none fits');
    expect(button("Back to the default list")).toBeUndefined();
  });

  it("adds a kind in lower case and clears the field", async () => {
    await mount();
    type("  Family ");
    await act(async () => button("Add")!.click());
    await flush();
    expect(setMailCategories).toHaveBeenCalledWith([...DEFAULTS, "family"]);
    expect(chips()).toContain("family");
    expect(input().value).toBe("");
  });

  it("will not add an empty name, a repeat, or unsure", async () => {
    await mount();
    expect(button("Add")!.disabled).toBe(true);
    type("Work");
    expect(button("Add")!.disabled).toBe(true);
    type("unsure");
    expect(button("Add")!.disabled).toBe(true);
    type("family");
    expect(button("Add")!.disabled).toBe(false);
  });

  it("takes a kind away, and keeps the last one", async () => {
    await mount();
    await act(async () => container.querySelector<HTMLButtonElement>("[aria-label='Remove receipt']")!.click());
    await flush();
    expect(setMailCategories).toHaveBeenCalledWith(["personal", "work", "newsletter", "notification"]);

    await mount(["work"]);
    expect(container.querySelector<HTMLButtonElement>("[aria-label='Remove work']")!.disabled).toBe(true);
  });

  it("stops at eight kinds", async () => {
    await mount(["a", "b", "c", "d", "e", "f", "g", "h"]);
    expect(input().disabled).toBe(true);
    expect(input().placeholder).toBe("Up to 8 kinds");
    expect(button("Add")!.disabled).toBe(true);
  });

  it("goes back to the default list with an empty one", async () => {
    await mount(["family", "work"]);
    await act(async () => button("Back to the default list")!.click());
    await flush();
    expect(setMailCategories).toHaveBeenCalledWith([]);
    expect(chips()).toEqual(DEFAULTS);
    expect(button("Back to the default list")).toBeUndefined();
  });

  it("shows the words of a refused save", async () => {
    await mount();
    setMailCategories.mockRejectedValue(new Error("could not save"));
    await act(async () => container.querySelector<HTMLButtonElement>("[aria-label='Remove work']")!.click());
    await flush();
    expect(container.querySelector(".hw-note.error")?.textContent).toBe("could not save");
  });
});
