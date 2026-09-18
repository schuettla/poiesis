/* The frontend has no CSS framework and no CSS Modules: every component
   stylesheet is plain global CSS, scoped only by the convention that class
   names are prefixed with their component (`memory-*` in Memory.css,
   `rail-*` in Rail.css). Vite concatenates all 38 of them into one stylesheet,
   so that convention is the only thing standing between two components and a
   silent override — and nothing enforces it at build time.

   This test does. It guards the one invariant that actually broke: a class
   whose *base* rule is declared in two different files, where the winner is
   decided by bundle order rather than by anyone's intent.

   State and modifier selectors (`.app.dock-max .workbench`, `.rail .nav-icon`,
   `.btn-primary.big`) are deliberately not base rules — they qualify a class
   another file owns, which is normal and stays legal. */

import { describe, expect, it } from "vitest";

// Read through Vite rather than node:fs so the suite needs no @types/node
// (the build's `tsc` pass covers src/ including this file).
const sheets = import.meta.glob("../**/*.css", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Selectors that declare a class outright: the whole selector is one bare
    class, with no second compound part, no descendant and no pseudo. */
function baseClasses(css: string): Set<string> {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const found = new Set<string>();
  for (const match of withoutComments.matchAll(/(^|[}{;])([^{}]+)\{/g)) {
    for (const selector of match[2].split(",")) {
      const bare = selector.trim().match(/^\.([a-zA-Z0-9_-]+)$/);
      if (bare) found.add(bare[1]);
    }
  }
  return found;
}

const files = Object.entries(sheets).map(
  ([path, css]) => [path.replace(/^\.\.\//, ""), css] as const,
);

describe("css conventions", () => {
  it("finds the component stylesheets", () => {
    expect(files.length).toBeGreaterThan(20);
  });

  it("declares each class in exactly one file", () => {
    const owners = new Map<string, string[]>();
    for (const [path, css] of files) {
      for (const cls of baseClasses(css)) {
        owners.set(cls, [...(owners.get(cls) ?? []), path]);
      }
    }

    const clashes = [...owners.entries()]
      .filter(([, where]) => where.length > 1)
      .map(([cls, where]) => `.${cls} — ${where.join(", ")}`);

    // A shared primitive belongs in styles/global.css, not in whichever route
    // stylesheet happened to need it first.
    expect(clashes).toEqual([]);
  });

  it("uses kebab-case class names", () => {
    const bad = new Set<string>();
    for (const [, css] of files) {
      for (const cls of baseClasses(css)) {
        if (!/^[a-z0-9]+(-[a-z0-9]+)*$/.test(cls)) bad.add(cls);
      }
    }
    expect([...bad]).toEqual([]);
  });
});
