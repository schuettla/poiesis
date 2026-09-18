/* Keeps the icon set in one place.
 *
 * Before Icons.tsx existed there was no home for a glyph, so the cheapest way
 * to get a folder was to paste one — and the folder ended up drawn three
 * times, the gear twice, the refresh twice, and the close cross three times in
 * three different geometries, two of which were the same shape written two
 * ways. Nothing caught any of it, because each copy looked fine on its own.
 *
 * So: an inline <svg> anywhere outside the module is a new icon that skipped
 * the module, and this fails until it moves in. The two exemptions are not
 * icons — one is the brand identity, the other a data visualization — and both
 * say so in their own files. */

import { describe, expect, it } from "vitest";

// Read through Vite rather than node:fs, so the suite needs no @types/node.
// Root-relative so routes/ is covered too, not just this file's neighbours.
const sources = import.meta.glob("/src/**/*.tsx", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const MODULE = "/src/components/Icons/Icons.tsx";

/** Neither of these is an icon, and neither belongs in an icon set. */
const NOT_ICONS = ["/src/components/Mark/PoiesisMark.tsx", "/src/components/Self/GrowthRings.tsx"];

/** A real element opens `<svg` then whitespace before its attributes, which is
 * what separates one from a `<svg>` written in prose in a comment. Deliberately
 * not a `/g` regex: `.test()` on one is stateful across calls. */
const SVG_ELEMENT = /<svg\s/;

describe("icons", () => {
  it("reads the component sources", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(20);
  });

  it("keeps every glyph in the Icons module", () => {
    const strays = Object.entries(sources)
      .filter(([path]) => path !== MODULE && !NOT_ICONS.includes(path))
      .filter(([, src]) => SVG_ELEMENT.test(src))
      .map(([path]) => path);

    // Add the icon to components/Icons/Icons.tsx and import it instead.
    expect(strays).toEqual([]);
  });

  it("draws icons on one grid", () => {
    const icons = sources[MODULE];
    // Every glyph inherits the single <svg> in `Glyph`, so a second one here
    // would mean a shape that opted out of the shared viewBox and stroke.
    expect(icons.match(/<svg\s/g)).toHaveLength(1);
    expect(icons).toContain('viewBox="0 0 20 20"');
  });
});
