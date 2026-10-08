/**
 * Self-update, the pure parts: version order, the download-event accumulator,
 * and the words an error becomes. Nothing here may ever show a user a raw error.
 */
import { describe, expect, it, vi } from "vitest";
import {
  checkedAgo,
  compareVersions,
  describeUpdateError,
  formatBytes,
  problemReportUrl,
  trackDownload,
} from "./updates";

describe("compareVersions", () => {
  it("orders numerically, so 0.1.10 is newer than 0.1.9", () => {
    expect(compareVersions("0.1.10", "0.1.9")).toBe(1);
    expect(compareVersions("0.1.9", "0.1.10")).toBe(-1);
  });

  it("treats equal versions as equal, and a missing part as zero", () => {
    expect(compareVersions("0.1.1", "0.1.1")).toBe(0);
    expect(compareVersions("0.2", "0.2.0")).toBe(0);
    expect(compareVersions("v0.1.2", "0.1.1")).toBe(1);
  });

  it("ranks a minor bump above any patch", () => {
    expect(compareVersions("0.2.0", "0.1.99")).toBe(1);
  });
});

describe("trackDownload", () => {
  it("adds up each piece that arrives into a running total", () => {
    const seen: Array<[number, number | null]> = [];
    const on = trackDownload((d, t) => seen.push([d, t]));
    on({ event: "progress", chunkLength: 100, contentLength: 1000 });
    on({ event: "progress", chunkLength: 250, contentLength: 1000 });
    on({ event: "progress", chunkLength: 50, contentLength: null });
    expect(seen).toEqual([
      [100, 1000],
      [350, 1000],
      [400, null],
    ]);
  });

  it("reports finished once, and not as progress", () => {
    const progress = vi.fn();
    const finished = vi.fn();
    const on = trackDownload(progress, finished);
    on({ event: "progress", chunkLength: 10, contentLength: 10 });
    on({ event: "finished" });
    expect(progress).toHaveBeenCalledTimes(1);
    expect(finished).toHaveBeenCalledTimes(1);
  });

  it("keeps separate downloads separate", () => {
    const a = vi.fn();
    const b = vi.fn();
    const onA = trackDownload(a);
    const onB = trackDownload(b);
    onA({ event: "progress", chunkLength: 5, contentLength: null });
    onB({ event: "progress", chunkLength: 7, contentLength: null });
    expect(a).toHaveBeenLastCalledWith(5, null);
    expect(b).toHaveBeenLastCalledWith(7, null);
  });
});

describe("describeUpdateError", () => {
  it("maps the codes Rust sends to plain words", () => {
    expect(describeUpdateError("network")).toBe("I couldn't reach GitHub");
    expect(describeUpdateError("signature")).toBe("that download didn't verify");
  });

  it("never lets a raw error through", () => {
    expect(describeUpdateError("error sending request for url (https://x)")).toBe(
      "something went wrong"
    );
    expect(describeUpdateError(new Error("os error 10061"))).toBe("something went wrong");
    expect(describeUpdateError(undefined)).toBe("something went wrong");
  });
});

describe("formatting", () => {
  it("formats sizes the way the copy table shows them", () => {
    expect(formatBytes(12_400_000)).toBe("12.4 MB");
    expect(formatBytes(300)).toBe("1 KB");
    expect(formatBytes(512_000)).toBe("512 KB");
  });

  it("says how long ago a check was", () => {
    const now = 1_000_000_000;
    expect(checkedAgo(now - 5_000, now)).toBe("just now");
    expect(checkedAgo(now - 60_000, now)).toBe("1 minute ago");
    expect(checkedAgo(now - 5 * 60_000, now)).toBe("5 minutes ago");
    expect(checkedAgo(now - 3 * 3_600_000, now)).toBe("3 hours ago");
    expect(checkedAgo(now - 2 * 86_400_000, now)).toBe("2 days ago");
  });
});

describe("problemReportUrl", () => {
  it("opens a new issue carrying the three version facts and nothing else", () => {
    const url = new URL(
      problemReportUrl({ app_version: "0.1.2", windows: "Windows 11 (build 22621)", schema_version: 30 })
    );
    expect(url.origin + url.pathname).toBe("https://github.com/schuettla/poiesis/issues/new");
    const body = url.searchParams.get("body")!;
    expect(body).toContain("Poiesis 0.1.2");
    expect(body).toContain("Windows 11 (build 22621)");
    expect(body).toContain("data v30");
  });
});
