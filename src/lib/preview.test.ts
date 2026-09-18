import { describe, expect, it } from "vitest";
import { contentVersion } from "./api";
import { loadIsForeign } from "../components/Workbench/PreviewConsole";

/**
 * `ART-6`: no CSP directive stops a document navigating *itself*, and from
 * outside the frame its URL is unreadable across origins. So the panel counts
 * the hellos every served document sends against the loads it sees: a load with
 * no hello is a page that left.
 */
describe("loadIsForeign", () => {
  it("accepts our own document, which says hello before its load", () => {
    expect(loadIsForeign(1, 1)).toBe(false);
  });

  it("catches a page that navigated itself away", () => {
    expect(loadIsForeign(1, 2)).toBe(true);
  });

  it("catches an instant redirect, where the foreign load lands milliseconds later", () => {
    // hello(1) → load(1) → location.href = … → load(2), no hello.
    expect(loadIsForeign(1, 2)).toBe(true);
  });

  it("does not fire again once the missing hello has been absorbed", () => {
    // The guard sets hellos = loads when it reverts; the restored page then
    // says hello and loads, keeping the totals level.
    expect(loadIsForeign(3, 3)).toBe(false);
  });

  it("tolerates a hello that arrives before its load event is counted", () => {
    expect(loadIsForeign(2, 1)).toBe(false);
  });
});

/**
 * `ART-6`: a served artifact keeps its id when it is updated in place, so the
 * only thing that can tell the webview to reload is the version in the query.
 * If this ever stops changing with the content, "fix the bug" appears to do
 * nothing — the panel goes on showing the broken page.
 */
describe("contentVersion", () => {
  it("changes when the content does", () => {
    expect(contentVersion("<p>a</p>")).not.toBe(contentVersion("<p>b</p>"));
  });

  it("is stable for the same content, so a re-render doesn't reload the page", () => {
    expect(contentVersion("<p>a</p>")).toBe(contentVersion("<p>a</p>"));
  });

  it("notices a one-character edit deep in a long page", () => {
    const filler = "x".repeat(5000);
    const before = `<!doctype html><body>${filler}<script>let a=1</script></body>`;
    const after = `<!doctype html><body>${filler}<script>let a=2</script></body>`;
    expect(contentVersion(before)).not.toBe(contentVersion(after));
  });

  it("is URL-safe, since it goes straight into the query string", () => {
    expect(contentVersion("<p>ünïcodé 🎮</p>")).toMatch(/^[a-z0-9]+$/);
  });
});
