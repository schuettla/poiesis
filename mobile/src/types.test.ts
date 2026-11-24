/**
 * The events the phone reads, against the files the desktop's own test keeps in step with Rust
 * (`src-tauri/src/remote/phone_api.rs`, `the_events_the_phone_reads_match_the_files...`). When the desktop
 * changes an event, that test fails first; rewriting the files then makes this one show what to adjust.
 */
import { describe, expect, it } from "vitest";
import { readEvent, statusLine } from "./types";

const files = import.meta.glob("./fixtures/*.json", { eager: true, import: "default" });
const fixture = (name: string): unknown => {
  const found = files[`./fixtures/${name}.json`];
  if (!found) throw new Error(`fixtures/${name}.json is missing`);
  return found;
};

describe("the events the desktop sends", () => {
  it("reads every type the phone uses", () => {
    expect(readEvent(fixture("token"))).toEqual({ type: "token", text: "Hello" });
    expect(readEvent(fixture("error"))).toEqual({ type: "error", message: "No model is running." });
    expect(readEvent(fixture("turn_saved"))).toEqual({
      type: "turn_saved",
      user_message_id: "u1",
      assistant_message_id: "a1",
    });
    expect(readEvent(fixture("step_start"))).toEqual({ type: "step_start", verb: "searched" });
    expect(readEvent(fixture("turn_finalized"))).toEqual({ type: "turn_finalized", message_id: "a1" });
    expect(readEvent(fixture("permission_gone"))).toEqual({ type: "permission_gone", id: "req1" });
    expect(readEvent(fixture("artifact"))).toMatchObject({ type: "artifact", id: "art1", title: "Notes", kind: "markdown" });
    expect(readEvent(fixture("block"))).toEqual({ type: "block", id: "b1", kind: "plan", title: "Plan" });
    expect(readEvent(fixture("sub"))).toEqual({ type: "sub", run_id: "r1" });
  });

  it("reads a permission as the card the phone shows", () => {
    expect(readEvent(fixture("permission"))).toEqual({
      type: "permission",
      request: {
        id: "req1",
        summary: "Write to report.md",
        path: "C:/Users/x/report.md",
        diff: "+ a new line",
        argv: undefined,
        project: undefined,
      },
    });
  });

  it("ignores what the phone does not use and what is broken", () => {
    expect(readEvent({ type: "step_done", id: "s" })).toBeNull();
    expect(readEvent({ type: "token" })).toBeNull();
    expect(readEvent({ type: "permission", request: { id: "x" } })).toBeNull();
    expect(readEvent("token")).toBeNull();
    expect(readEvent(null)).toBeNull();
  });

  it("reads the three lines the phone adds itself", () => {
    expect(readEvent({ type: "chat", conversation_id: "c1" })).toEqual({ type: "chat", conversation_id: "c1" });
    expect(readEvent({ type: "steered" })).toEqual({ type: "steered" });
    expect(readEvent({ type: "remote_notice", text: "Nobody answered." })).toEqual({
      type: "remote_notice",
      text: "Nobody answered.",
    });
  });
});

describe("the status line", () => {
  it("says it in words and never shows an id", () => {
    expect(statusLine("connected", "Erich's desktop")).toBe("Connected to Erich's desktop");
    expect(statusLine("connecting", null)).toBe("Looking for your desktop ...");
    expect(statusLine("notfound", "x")).toBe("Your desktop is asleep or Poiesis is not running.");
  });
});
