/**
 * `USE-10`, `USE-UI-7`: the store side of Verify. Progress lands under the message,
 * a cancel keeps nothing, an error stays where it can be read, a mark replaces the
 * claim, and Revise sends a new message and never edits the old one.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { VerifyCheck, VerifyClaim, VerifyEvent } from "./api";

const verifyMessage = vi.fn();
const verifyCancel = vi.fn();
const verifyList = vi.fn();
const verifyMark = vi.fn();
const verifyRevise = vi.fn();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  verifyMessage: (...a: unknown[]) => verifyMessage(...a),
  verifyCancel: (...a: unknown[]) => verifyCancel(...a),
  verifyList: (...a: unknown[]) => verifyList(...a),
  verifyMark: (...a: unknown[]) => verifyMark(...a),
  verifyRevise: (...a: unknown[]) => verifyRevise(...a),
}));

import { useAppStore } from "./store";

const claim = (over: Partial<VerifyClaim> = {}): VerifyClaim => ({
  id: "c1",
  text: "A is B.",
  quote: "A is B",
  source: null,
  importance: 2,
  kind: "stable",
  finding: "refuted",
  p: 0.9,
  how: "fast",
  settled: true,
  fast: "refuted",
  slow: null,
  reason: null,
  passages: [],
  decision: "d1",
  mark: null,
  ...over,
});
const check = (over: Partial<VerifyCheck> = {}): VerifyCheck => ({
  message_id: "m1",
  conversation_id: "c1",
  created_at: 1,
  claims: [claim()],
  note: null,
  skipped: 0,
  mode: "deep",
  summary: null,
  ...over,
});
const entry = (id = "m1") => useAppStore.getState().verifications[id];

const sendMessage = vi.fn(async () => {});

beforeEach(() => {
  for (const f of [verifyMessage, verifyCancel, verifyList, verifyMark, verifyRevise]) f.mockReset();
  verifyCancel.mockResolvedValue(undefined);
  verifyList.mockResolvedValue([]);
  sendMessage.mockClear();
  useAppStore.setState({ activeConversationId: "c1", verifications: {}, verifyLoaded: {}, sendMessage });
});

describe("verifyMessage", () => {
  it("shows progress while it runs and keeps the check when it is done", async () => {
    const seen: (string | undefined)[] = [];
    verifyMessage.mockImplementation(async (_c: string, _m: string, on: (e: VerifyEvent) => void) => {
      on({ type: "progress", phase: "reading" });
      seen.push(entry().progress?.phase);
      on({ type: "progress", phase: "checking", done: 1, total: 4 });
      seen.push(entry().progress?.phase);
      expect(entry().running).toBe(true);
      on({ type: "done", check: check() });
    });
    await useAppStore.getState().verifyMessage("m1");
    expect(seen).toEqual(["reading", "checking"]);
    expect(entry()).toMatchObject({ running: false, progress: null, error: null });
    expect(entry().check?.claims).toHaveLength(1);
    expect(verifyMessage.mock.calls[0].slice(0, 2)).toEqual(["c1", "m1"]);
  });

  it("keeps nothing of a check that was cancelled, and the old check stays", async () => {
    useAppStore.setState({ verifications: { m1: { running: false, progress: null, check: check(), error: null } } });
    verifyMessage.mockImplementation(async (_c: string, _m: string, on: (e: VerifyEvent) => void) => {
      on({ type: "progress", phase: "reading" });
      on({ type: "cancelled" });
    });
    await useAppStore.getState().verifyMessage("m1");
    expect(entry()).toMatchObject({ running: false, progress: null });
    expect(entry().check).not.toBeNull();
  });

  it("keeps an error where the panel can show it", async () => {
    verifyMessage.mockImplementation(async (_c: string, _m: string, on: (e: VerifyEvent) => void) => {
      on({ type: "error", message: "No model is loaded." });
    });
    await useAppStore.getState().verifyMessage("m1");
    expect(entry()).toMatchObject({ running: false, error: "No model is loaded." });
  });

  it("turns a failed command into an error and never stays running", async () => {
    verifyMessage.mockRejectedValue(new Error("Verify answers is an experiment."));
    await useAppStore.getState().verifyMessage("m1");
    expect(entry()).toMatchObject({ running: false, error: "Verify answers is an experiment." });
    // A command that ended without a closing event is not left running either.
    verifyMessage.mockImplementation(async (_c: string, _m: string, on: (e: VerifyEvent) => void) => {
      on({ type: "progress", phase: "reading" });
    });
    await useAppStore.getState().verifyMessage("m2");
    expect(entry("m2").running).toBe(false);
  });

  it("does not start a second check of an answer that is being checked", async () => {
    useAppStore.setState({ verifications: { m1: { running: true, progress: null, check: null, error: null } } });
    await useAppStore.getState().verifyMessage("m1");
    expect(verifyMessage).not.toHaveBeenCalled();
  });

  it("clears an earlier error when it starts again", async () => {
    useAppStore.setState({ verifications: { m1: { running: false, progress: null, check: null, error: "old" } } });
    verifyMessage.mockImplementation(async (_c: string, _m: string, on: (e: VerifyEvent) => void) => {
      expect(entry().error).toBeNull();
      on({ type: "done", check: check() });
    });
    await useAppStore.getState().verifyMessage("m1");
    expect(entry().error).toBeNull();
  });
});

describe("cancelVerify", () => {
  it("asks the backend to stop the open chat's check", () => {
    useAppStore.getState().cancelVerify();
    expect(verifyCancel).toHaveBeenCalledWith("c1");
  });
});

describe("loadVerifications", () => {
  it("reads a chat's kept checks once", async () => {
    verifyList.mockResolvedValue([check()]);
    await useAppStore.getState().loadVerifications("c1");
    await useAppStore.getState().loadVerifications("c1");
    expect(verifyList).toHaveBeenCalledTimes(1);
    expect(entry().check?.message_id).toBe("m1");
  });

  it("does not overwrite a check that is running now", async () => {
    useAppStore.setState({ verifications: { m1: { running: true, progress: null, check: null, error: null } } });
    verifyList.mockResolvedValue([check()]);
    await useAppStore.getState().loadVerifications("c1");
    expect(entry()).toMatchObject({ running: true, check: null });
  });

  it("reads again next time when the read failed", async () => {
    verifyList.mockRejectedValueOnce(new Error("db"));
    await useAppStore.getState().loadVerifications("c1");
    expect(useAppStore.getState().verifyLoaded.c1).toBeUndefined();
    verifyList.mockResolvedValue([check()]);
    await useAppStore.getState().loadVerifications("c1");
    expect(entry().check).not.toBeNull();
  });
});

describe("markClaim", () => {
  it("replaces the claim with the one the backend returns", async () => {
    useAppStore.setState({
      verifications: {
        m1: { running: false, progress: null, error: null, check: check({ claims: [claim(), claim({ id: "c2", text: "C." })] }) },
      },
    });
    verifyMark.mockResolvedValue(claim({ mark: "wrong" }));
    await useAppStore.getState().markClaim("m1", "c1", "wrong");
    expect(verifyMark).toHaveBeenCalledWith("m1", "c1", "wrong");
    const claims = entry().check!.claims;
    expect(claims.map((c) => c.mark)).toEqual(["wrong", null]);
  });
});

describe("reviseFromVerification", () => {
  it("sends the request as a new message", async () => {
    verifyRevise.mockResolvedValue("Write a corrected version of it as a new answer.");
    await useAppStore.getState().reviseFromVerification("m1");
    expect(verifyRevise).toHaveBeenCalledWith("m1");
    expect(sendMessage).toHaveBeenCalledWith("Write a corrected version of it as a new answer.");
  });

  it("sends nothing when there is nothing to revise", async () => {
    verifyRevise.mockRejectedValue(new Error("There is nothing to revise."));
    await expect(useAppStore.getState().reviseFromVerification("m1")).rejects.toThrow("nothing to revise");
    expect(sendMessage).not.toHaveBeenCalled();
  });
});
