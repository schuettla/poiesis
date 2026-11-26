/**
 * @vitest-environment jsdom
 *
 * `UI-7`: links in an answer as small chips. The reading of an address (`linkKinds`) has its own
 * tests; these cover what the answer does with it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import RunText from "./RunText";
import { LINK_LOGOS } from "./LinkChip";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});
const render = (text: string, props: Partial<React.ComponentProps<typeof RunText>> = {}) =>
  act(() => root.render(<RunText text={text} {...props} />));
const links = () => Array.from(container.querySelectorAll("a"));

describe("link chips (UI-7)", () => {
  it("shows a GitHub pull request as a chip with its mark and what it points at", () => {
    render("See https://github.com/acme/app/pull/12 for it.");
    const [a] = links();
    expect(a.className).toBe("link-chip");
    expect(a.textContent).toContain("acme/app#12");
    expect(a.getAttribute("href")).toBe("https://github.com/acme/app/pull/12");
    // The full address is the tooltip, and the mark is a bundled file under the base path.
    expect(a.getAttribute("title")).toBe("https://github.com/acme/app/pull/12");
    expect(a.querySelector("img")?.getAttribute("src")).toBe(`${import.meta.env.BASE_URL}app-logos/github.svg`);
  });

  it("keeps the words of a link someone wrote, with the mark before them", () => {
    render("[the Q3 plan](https://docs.google.com/document/d/abc123/edit)");
    const [a] = links();
    expect(a.className).toBe("link-chip");
    expect(a.querySelector(".link-chip-text")?.textContent).toBe("the Q3 plan");
    expect(a.querySelector("img")?.getAttribute("src")).toContain("googledocs.svg");
  });

  it("leaves a link to an unknown host as it was", () => {
    render("Read https://example.org/some/page now.");
    const [a] = links();
    expect(a.className).toBe("");
    expect(a.textContent).toBe("https://example.org/some/page");
    expect(a.querySelector("img")).toBeNull();
  });

  it("does not make a chip of a look-alike host", () => {
    render("Go to https://github.com.evil.io/acme/app/pull/12 or https://xgithub.com/acme/app");
    expect(links().map((a) => a.className)).toEqual(["", ""]);
  });

  it("shows the first letter for a service with no mark", () => {
    render("https://www.google.com/maps/place/Vienna");
    const [a] = links();
    expect(a.className).toBe("link-chip");
    expect(a.querySelector("img")).toBeNull();
    // The letter comes from the chip's name ("Maps · Vienna").
    expect(a.querySelector(".app-logo")?.textContent).toBe("M");
  });

  it("still marks claims in a checked answer, and a chip stays a link there", () => {
    render("See https://github.com/acme/app/issues/5.", { claims: { marks: [], picked: null, onPick: vi.fn() } });
    expect(links()[0].className).toBe("link-chip");
  });
});

describe("the marks", () => {
  it("are all files the app ships", () => {
    const shipped = Object.keys(import.meta.glob("../../../public/app-logos/*.svg")).map((p) => p.split("/").pop());
    expect(shipped.length).toBeGreaterThan(100);
    for (const [service, file] of Object.entries(LINK_LOGOS)) expect(shipped, `${service}: ${file}`).toContain(file);
  });
});
