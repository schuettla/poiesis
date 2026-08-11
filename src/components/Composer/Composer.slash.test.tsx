/**
 * @vitest-environment jsdom
 *
 * `SKL-UI-2`: typing `/` in the composer opens the skill list.
 *
 * This half of the standard shipped as dead UI once already — the picker
 * existed but only behind the ⌁ menu, so `/` did nothing. These tests drive the
 * real input rather than the handler, because the bug was in the wiring.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Composer from "./Composer";
import { useAppStore } from "../../lib/store";
import type { SkillView } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

function skill(name: string, enabled = true): SkillView {
  return {
    name,
    description: `does ${name}`,
    when_to_use: null,
    source: "app",
    dir: `/skills/${name}`,
    enabled,
    unsupported: [],
    used: 0,
    rough: 0,
    risk: 0,
    risk_flags: [],
  };
}

let container: HTMLDivElement;
let root: Root;
const sent: string[] = [];

beforeEach(() => {
  sent.length = 0;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    skills: [skill("weekly-report"), skill("pdf-forms"), skill("off-one", false)],
  });
  act(() => {
    root.render(<Composer onSend={(t) => sent.push(t)} />);
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const input = () => container.querySelector("input[type=text]") as HTMLInputElement;
const menu = () => container.querySelector(".composer-slash-menu");
const options = () => Array.from(container.querySelectorAll(".composer-slash-menu .mi-body"));

/** Drive the controlled input the way a keystroke would. */
function type(text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    setter.call(input(), text);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function press(key: string) {
  act(() => {
    input().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });
}

describe("composer `/` skill drop-up", () => {
  it("is closed until a slash is typed", () => {
    expect(menu()).toBeNull();
  });

  it("opens on `/` and lists only enabled skills", () => {
    type("/");
    expect(menu()).not.toBeNull();
    const names = options().map((o) => o.textContent ?? "");
    expect(names.some((n) => n.startsWith("weekly-report"))).toBe(true);
    expect(names.some((n) => n.startsWith("pdf-forms"))).toBe(true);
    expect(names.some((n) => n.startsWith("off-one"))).toBe(false);
  });

  it("filters as you keep typing", () => {
    type("/week");
    expect(options()).toHaveLength(1);
    expect(options()[0].textContent).toContain("weekly-report");
  });

  it("closes once the name is complete and a space is typed", () => {
    type("/weekly-report ");
    expect(menu()).toBeNull();
  });

  it("ignores a slash that isn't at the start — dates and paths aren't commands", () => {
    type("what about 2026/08/10");
    expect(menu()).toBeNull();
  });

  it("Enter picks the highlighted skill instead of sending the message", () => {
    type("/week");
    press("Enter");
    expect(sent, "the half-typed command must not go out as a message").toHaveLength(0);
    expect(input().value).toBe("/weekly-report ");
    expect(menu()).toBeNull();
  });

  it("arrows move the highlight before Enter takes it", () => {
    type("/");
    press("ArrowDown");
    press("Enter");
    // Order follows the store's list; the second entry is the one taken.
    expect(input().value).toBe("/pdf-forms ");
  });

  it("Escape hides the list without eating what was typed", () => {
    type("/week");
    press("Escape");
    expect(menu()).toBeNull();
    expect(input().value).toBe("/week");
  });

  it("Enter sends normally when the list isn't open", () => {
    type("hello there");
    press("Enter");
    expect(sent).toEqual(["hello there"]);
  });
});
