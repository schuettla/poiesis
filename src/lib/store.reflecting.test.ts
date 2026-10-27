/**
 * `FIX-1`: every reflection, wherever it was started, moves the mark and the
 * Rail row through one app event. The launch catch-up and the daily job have no
 * frontend call to hang the state on, so the event is the only source.
 */
import { beforeEach, describe, expect, it } from "vitest";
import type { ReflectingEvent } from "./api";
import { applyReflecting, useAppStore } from "./store";

const conv = (id: string, reflectedAt?: number) =>
  ({ id, title: id, messages: [], reflectedAt }) as unknown as ReturnType<typeof useAppStore.getState>["conversations"][number];

const apply = (e: ReflectingEvent) => useAppStore.setState(applyReflecting(useAppStore.getState(), e));

describe("applyReflecting", () => {
  beforeEach(() => {
    useAppStore.setState({
      presence: "idle",
      busy: false,
      reflectingIds: [],
      digestedIds: [],
      conversations: [conv("c1"), conv("c2")],
    });
  });

  it("weaves the mark while a background reflection runs, then idles", () => {
    apply({ conversation_id: "c1", on: true, learned: 0 });
    expect(useAppStore.getState().presence).toBe("reflecting");
    expect(useAppStore.getState().reflectingIds).toEqual(["c1"]);

    apply({ conversation_id: "c1", on: false, learned: 0 });
    expect(useAppStore.getState().presence).toBe("idle");
    expect(useAppStore.getState().reflectingIds).toEqual([]);
  });

  it("stays reflecting until the last of several is done", () => {
    apply({ conversation_id: "c1", on: true, learned: 0 });
    apply({ conversation_id: "c2", on: true, learned: 0 });
    apply({ conversation_id: "c1", on: false, learned: 0 });
    expect(useAppStore.getState().presence).toBe("reflecting");
    apply({ conversation_id: "c2", on: false, learned: 0 });
    expect(useAppStore.getState().presence).toBe("idle");
  });

  it("marks only a chat where a lesson was written as digested", () => {
    apply({ conversation_id: "c1", on: true, learned: 0 });
    apply({ conversation_id: "c1", on: false, learned: 0 });
    expect(useAppStore.getState().digestedIds).toEqual([]);

    apply({ conversation_id: "c2", on: true, learned: 0 });
    apply({ conversation_id: "c2", on: false, learned: 2 });
    expect(useAppStore.getState().digestedIds).toEqual(["c2"]);
  });

  it("stamps the chat as reflected when the pass ends", () => {
    apply({ conversation_id: "c1", on: false, learned: 0 });
    const c1 = useAppStore.getState().conversations.find((c) => c.id === "c1");
    expect(c1?.reflectedAt).toBeGreaterThan(0);
  });

  it("does not take the mark off a running generation", () => {
    useAppStore.setState({ busy: true, presence: "active" });
    apply({ conversation_id: "c1", on: true, learned: 0 });
    expect(useAppStore.getState().presence).toBe("active");
    apply({ conversation_id: "c1", on: false, learned: 0 });
    expect(useAppStore.getState().presence).toBe("active");
  });
});
