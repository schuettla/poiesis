// The walking rules are ported from oh-my-hermes (MIT), apps/omher/src/chat/parts/promptHistory.test.ts.
import { describe, expect, it } from "vitest";
import { newer, older, promptsOf, type Recall } from "./promptHistory";
import type { Message } from "../../lib/types";

const user = (text: string, over: Partial<Message> = {}) => ({ id: text, role: "user", text, ...over }) as Message;

describe("promptsOf", () => {
  it("keeps the person's messages only, oldest first, as typed", () => {
    const messages = [
      user("fix this now"),
      { id: "a", role: "assistant", text: "done" } as Message,
      user("and test it"),
    ];
    expect(promptsOf(messages)).toEqual(["fix this now", "and test it"]);
  });

  it("collapses a prompt sent twice in a row, not one sent again later", () => {
    const messages = ["a", "a", "b", "a"].map((t, i) => user(t, { id: String(i) }));
    expect(promptsOf(messages)).toEqual(["a", "b", "a"]);
  });

  it("gives a skill as the command that was typed, not the words it was sent as", () => {
    const m = user("Run my review skill on the diff", { command: { name: "review", args: "the diff" } });
    expect(promptsOf([m])).toEqual(["/review the diff"]);
    expect(promptsOf([user("x", { command: { name: "plan", args: "" } })])).toEqual(["/plan"]);
  });

  it("leaves out a quoted passage, a block's action, a held message and a send with no words", () => {
    const quoted = user("> a passage\n> from a page\n> (notes.md, lines 3-4)\n\nwhat does it mean?");
    const action = user("I picked the blue one\n```poiesis-action\n{}\n```");
    const held = user("stop it", { midRun: "held" });
    expect(promptsOf([quoted, action, held, user("  "), user("kept", { midRun: "delivered" })])).toEqual([
      "what does it mean?",
      "kept",
    ]);
  });

  it("is empty for a chat that is not there", () => {
    expect(promptsOf(undefined)).toEqual([]);
  });
});

describe("older / newer", () => {
  const history = ["first", "second", "third"];

  it("walks newest first, then stops at the oldest", () => {
    let r: Recall = null;
    const seen: string[] = [];
    for (;;) {
      const s = older(history, r, "my draft");
      if (!s) break;
      seen.push(s.text);
      r = s.recall;
    }
    expect(seen).toEqual(["third", "second", "first"]);
    expect(r).toEqual({ at: 0, draft: "my draft" });
  });

  it("keeps the draft typed before the first ArrowUp, not the recalled text", () => {
    const one = older(history, null, "my draft")!;
    const two = older(history, one.recall, one.text)!;
    expect(two.recall).toEqual({ at: 1, draft: "my draft" });
  });

  it("goes newer, and past the newest gives the draft back and ends the walk", () => {
    expect(newer(history, { at: 1, draft: "d" })).toEqual({ recall: { at: 2, draft: "d" }, text: "third" });
    expect(newer(history, { at: 2, draft: "d" })).toEqual({ recall: null, text: "d" });
  });

  it("does nothing outside a walk or with no history", () => {
    expect(newer(history, null)).toBeNull();
    expect(older([], null, "d")).toBeNull();
  });

  it("stays inside a history that shrank under the walk", () => {
    expect(older(["a"], { at: 4, draft: "d" }, "x")).toEqual({ recall: { at: 0, draft: "d" }, text: "a" });
  });

  it("never changes the history", () => {
    const h = Object.freeze(["a", "b"]);
    older(h, null, "x");
    newer(h, { at: 0, draft: "x" });
    expect(h).toEqual(["a", "b"]);
  });
});
