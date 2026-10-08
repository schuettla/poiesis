/**
 * `UPD-5`/`UPD-6`: the update phase machine as the real store runs it, with
 * only the Rust boundary scripted.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppUpdateInfo } from "./updates";

let found: AppUpdateInfo | null = null;
let checkError: unknown = null;
let installScript: (progress: (d: number, t: number | null) => void, finished: () => void) => Promise<void> =
  async () => {};
let running = "0.1.1";
const settings = new Map<string, string>();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  getAppVersion: () => Promise.resolve(running),
  getSetting: (k: string) => Promise.resolve(settings.get(k) ?? null),
  setSetting: (k: string, v: string) => {
    settings.set(k, v);
    return Promise.resolve();
  },
  checkForUpdate: () => (checkError ? Promise.reject(checkError) : Promise.resolve(found)),
  installUpdate: (p: (d: number, t: number | null) => void, f: () => void) => installScript(p, f),
}));

import { useAppStore } from "./store";

const state = () => useAppStore.getState();
const v2: AppUpdateInfo = { version: "0.1.2", notes: "A few fixes." };

beforeEach(() => {
  found = null;
  checkError = null;
  installScript = async () => {};
  running = "0.1.1";
  settings.clear();
  useAppStore.setState({
    updateState: { phase: "idle" },
    updateAutoCheck: true,
    updateToast: null,
    updateReceiptToast: null,
  });
});

describe("checking", () => {
  it("lands on current, with the time, when nothing is newer", async () => {
    await state().checkForUpdates(true);
    expect(state().updateState.phase).toBe("current");
  });

  it("lands on available, carrying the notes, when something is", async () => {
    found = v2;
    await state().checkForUpdates(true);
    expect(state().updateState).toEqual({ phase: "available", info: v2 });
  });

  it("says so out loud when the user asked and it failed", async () => {
    checkError = "network";
    await state().checkForUpdates(true);
    expect(state().updateState).toEqual({ phase: "error", message: "I couldn't reach GitHub" });
  });

  it("stays quiet when the startup check fails — being offline is not news", async () => {
    checkError = "network";
    await state().checkForUpdates(false);
    expect(state().updateState).toEqual({ phase: "idle" });
  });

  it("does not start a second check on top of one running", async () => {
    useAppStore.setState({ updateState: { phase: "checking" } });
    found = v2;
    await state().checkForUpdates(true);
    expect(state().updateState.phase).toBe("checking");
  });
});

describe("installing", () => {
  it("goes available → downloading → ready, following the real byte counts", async () => {
    useAppStore.setState({ updateState: { phase: "available", info: v2 } });
    const seen: string[] = [];
    installScript = async (progress) => {
      progress(100, 1000);
      seen.push(JSON.stringify(state().updateState));
      progress(600, 1000);
      seen.push(JSON.stringify(state().updateState));
    };
    await state().startUpdateInstall();
    expect(JSON.parse(seen[0])).toEqual({ phase: "downloading", info: v2, downloaded: 100, total: 1000 });
    expect(JSON.parse(seen[1])).toMatchObject({ downloaded: 600, total: 1000 });
    expect(state().updateState).toEqual({ phase: "ready", info: v2 });
  });

  it("shows the installer taking over once the bytes are in", async () => {
    useAppStore.setState({ updateState: { phase: "available", info: v2 } });
    let during: unknown;
    installScript = async (_p, finished) => {
      finished();
      during = state().updateState;
      // On Windows the process ends here; nothing after this ever runs.
      throw new Error("process exited");
    };
    await state().startUpdateInstall();
    expect(during).toEqual({ phase: "installing", info: v2 });
  });

  it("always reports an install failure, and says a bad signature plainly", async () => {
    useAppStore.setState({ updateState: { phase: "available", info: v2 } });
    installScript = async () => {
      throw "signature";
    };
    await state().startUpdateInstall();
    expect(state().updateState).toEqual({ phase: "error", message: "that download didn't verify" });
  });

  it("will not install anything that was not offered", async () => {
    const run = vi.fn();
    installScript = run as never;
    await state().startUpdateInstall();
    expect(run).not.toHaveBeenCalled();
  });

  it("forgets the offer on Not now", () => {
    useAppStore.setState({ updateState: { phase: "available", info: v2 } });
    state().declineUpdate();
    expect(state().updateState.phase).toBe("idle");
  });
});

describe("the startup check (UPD-6)", () => {
  it("mentions a new version once, and keeps it available both times", async () => {
    found = v2;
    settings.set("updates.last_seen_version", "0.1.1");
    await state().initUpdates(0);
    expect(state().updateToast).toEqual(v2);
    expect(state().updateState.phase).toBe("available");

    // Restart: the toast is gone with the session, the latch is not.
    useAppStore.setState({ updateToast: null, updateState: { phase: "idle" } });
    await state().initUpdates(0);
    expect(state().updateToast).toBeNull();
    expect(state().updateState.phase).toBe("available");
  });

  it("announces the next version even though an earlier one was mentioned", async () => {
    settings.set("updates.notified_version", "0.1.2");
    found = { version: "0.1.3", notes: "" };
    await state().initUpdates(0);
    expect(state().updateToast?.version).toBe("0.1.3");
  });

  it("does not check at all when the user turned that off", async () => {
    settings.set("updates.autocheck", "false");
    found = v2;
    await state().initUpdates(0);
    expect(state().updateAutoCheck).toBe(false);
    expect(state().updateState.phase).toBe("idle");
    expect(state().updateToast).toBeNull();
  });

  it("stays silent when the startup check can't reach GitHub", async () => {
    checkError = "network";
    await state().initUpdates(0);
    expect(state().updateState.phase).toBe("idle");
    expect(state().updateToast).toBeNull();
  });
});

describe("the arrival receipt (UPD-UI-4)", () => {
  it("says nothing on a first-ever run, but remembers the version", async () => {
    await state().initUpdates(0);
    expect(state().updateReceiptToast).toBeNull();
    expect(settings.get("updates.last_seen_version")).toBe("0.1.1");
  });

  it("shows once after the version went up, and not on the next start", async () => {
    settings.set("updates.last_seen_version", "0.1.1");
    running = "0.1.2";
    await state().initUpdates(0);
    expect(state().updateReceiptToast).toBe("0.1.2");
    expect(settings.get("updates.last_seen_version")).toBe("0.1.2");

    useAppStore.setState({ updateReceiptToast: null });
    await state().initUpdates(0);
    expect(state().updateReceiptToast).toBeNull();
  });

  it("says nothing when the version went down", async () => {
    settings.set("updates.last_seen_version", "0.1.5");
    running = "0.1.2";
    await state().initUpdates(0);
    expect(state().updateReceiptToast).toBeNull();
    expect(settings.get("updates.last_seen_version")).toBe("0.1.2");
  });
});
