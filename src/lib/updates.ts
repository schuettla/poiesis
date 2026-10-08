// Self-update (AUTOUPDATE_PLAN, UPD-4/5): the pure parts — the phase machine's
// vocabulary, version comparison, and the words. Kept out of the store and the
// API wrapper so each can be tested without either.

/** What the UI knows about a found update. The live handle stays in Rust. */
export type AppUpdateInfo = { version: string; notes: string };

export type UpdateState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "current"; checkedAt: number }
  | { phase: "available"; info: AppUpdateInfo }
  | { phase: "downloading"; info: AppUpdateInfo; downloaded: number; total: number | null }
  /** Windows only: the bytes are in and the installer is taking over. The app
   * exits during this, so the next thing the user sees is the new version. */
  | { phase: "installing"; info: AppUpdateInfo }
  /** Elsewhere the install returns and the app keeps running until restarted. */
  | { phase: "ready"; info: AppUpdateInfo }
  | { phase: "error"; message: string };

/** The wire shape of `update_install_cmd`'s channel (`DownloadEvent` in Rust). */
export type DownloadEvent =
  | { event: "progress"; chunkLength: number; contentLength: number | null }
  | { event: "finished" };

/**
 * Turns the channel's per-chunk events into one running `(downloaded, total)`,
 * so nothing above this has to know the protocol. `onFinished` fires once,
 * when every byte is in.
 */
export function trackDownload(
  onProgress: (downloaded: number, total: number | null) => void,
  onFinished: () => void = () => {}
): (e: DownloadEvent) => void {
  let downloaded = 0;
  return (e) => {
    if (e.event === "progress") {
      downloaded += e.chunkLength;
      onProgress(downloaded, e.contentLength);
    } else {
      onFinished();
    }
  };
}

/**
 * The reason, in words. Rust sends a code, never prose, and anything that isn't
 * one of the two we can say something useful about is "something went wrong" —
 * a raw error string in this UI is a bug.
 */
export function describeUpdateError(e: unknown): string {
  const code = typeof e === "string" ? e : e instanceof Error ? e.message : "";
  if (code === "network") return "I couldn't reach GitHub";
  if (code === "signature") return "that download didn't verify";
  return "something went wrong";
}

/** Numeric semver-ish comparison: `0.1.10` is newer than `0.1.9`. Anything
 * after a `-` or `+` is ignored; a missing part counts as 0. */
export function compareVersions(a: string, b: string): number {
  const parts = (v: string) =>
    v
      .replace(/^v/i, "")
      .split(/[-+]/)[0]
      .split(".")
      .map((n) => parseInt(n, 10) || 0);
  const pa = parts(a);
  const pb = parts(b);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d < 0 ? -1 : 1;
  }
  return 0;
}

/** Decimal units, as GitHub lists the asset, so the two can be compared. */
export function formatBytes(n: number): string {
  if (n < 1_000_000) return `${Math.max(1, Math.round(n / 1000))} KB`;
  return `${(n / 1_000_000).toFixed(1)} MB`;
}

/** `Checked just now` / `Checked 5 minutes ago` — the part after "Checked". */
export function checkedAgo(ts: number, now = Date.now()): string {
  const mins = Math.floor((now - ts) / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins} minute${mins === 1 ? "" : "s"} ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.floor(hours / 24);
  return `${days} day${days === 1 ? "" : "s"} ago`;
}

/** Settings keys (`settings` table). */
export const UPDATE_AUTOCHECK_KEY = "updates.autocheck";
/** The last version we told the user about with a toast — one mention each. */
export const UPDATE_NOTIFIED_KEY = "updates.notified_version";
/** The version that was running last time, for the arrival receipt. */
export const UPDATE_LAST_SEEN_KEY = "updates.last_seen_version";

/** Whether the About tab and the Settings cog should carry the badge. */
export const updateIsWaiting = (s: UpdateState): boolean =>
  s.phase === "available" || s.phase === "ready";

/** Where "Report a problem" goes: a new issue with the three version facts
 * already in the body (PUB-3). Nothing from the user's data. */
export const ISSUES_NEW_URL = "https://github.com/schuettla/poiesis/issues/new";

export function problemReportUrl(f: {
  app_version: string;
  windows: string;
  schema_version: number;
}): string {
  const body = [
    "**What happened?**",
    "",
    "",
    "**What did you expect?**",
    "",
    "",
    "---",
    `Poiesis ${f.app_version} · ${f.windows} · data v${f.schema_version}`,
  ].join("\n");
  return `${ISSUES_NEW_URL}?${new URLSearchParams({ body }).toString()}`;
}
