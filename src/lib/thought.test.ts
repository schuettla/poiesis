import { describe, expect, it } from "vitest";
import type { DecisionEntry, PlanView, Thought, VerifyCheck, VerifyClaim } from "./api";
import { emptyThought, engineLine, joinWords, receiptOf, stripParts, tokensShorter, type TurnThought } from "./thought";

const thought = (over: Partial<Thought> = {}): Thought => ({ ...emptyThought(), ...over });

const WEB = { id: "web_search", words: "Not searching the web", action: "Search the web too" };
const MAIL = { id: "mcp:abc", words: "Not using Notion", action: "Use Notion too" };

describe("the strip's parts (SEE-T-5)", () => {
  it("says where I will look, what I left out, who does it and how fast, in that order", () => {
    const parts = stripParts(
      thought({ looking: ["in Google Calendar"], left_out: [WEB], handover: { agent: "Claude Code", rule: "U2" }, ms: 38 })
    );
    expect(parts.map((p) => p.text)).toEqual([
      "Looking in Google Calendar",
      "not searching the web",
      "Claude Code's job, by your rule U2",
      "38 ms",
    ]);
    expect(parts.map((p) => p.kind)).toEqual(["where", "left-out", "who", "speed"]);
  });

  it("joins several places, and answers directly when nothing is needed", () => {
    expect(stripParts(thought({ looking: ["in your files", "on the web"], ms: 12 }))[0].text).toBe("Looking in your files and on the web");
    expect(joinWords(["a", "b", "c"])).toBe("a, b and c");
    expect(stripParts(thought({ direct: true, ms: 12 })).map((p) => p.text)).toEqual(["Answering directly", "12 ms"]);
  });

  it("puts the click on the part that was left out, naming the group", () => {
    const left = stripParts(thought({ direct: true, left_out: [WEB, MAIL], ms: 9 })).filter((p) => p.kind === "left-out");
    expect(left.map((p) => p.action)).toEqual([
      { label: "Search the web too", unit: "web_search" },
      { label: "Use Notion too", unit: "mcp:abc" },
    ]);
  });

  it("leads with the capital when nothing came before the left-out part", () => {
    expect(stripParts(thought({ left_out: [WEB], ms: 9 }))[0].text).toBe("Not searching the web");
  });

  it("stops saying a group is left out once it is back", () => {
    const parts = stripParts(thought({ direct: true, left_out: [WEB], ms: 9 }), ["web_search"]);
    expect(parts.map((p) => p.kind)).toEqual(["where", "speed"]);
  });

  it("says nothing at all when nothing was understood, even with a time", () => {
    expect(stripParts(thought({ ms: 40 }))).toEqual([]);
  });

  it("never shows a bare probability", () => {
    const text = stripParts(thought({ looking: ["on the web"], left_out: [WEB], ms: 38 })).map((p) => p.text).join(" ");
    expect(text).not.toMatch(/\d\.\d/);
  });
});

describe("the missing engine line (SEE-T-5)", () => {
  it("is words, with Start only while a model is installed and not running", () => {
    expect(engineLine(thought())).toBeNull();
    expect(engineLine(thought({ engine: "cold" }))).toEqual({
      text: "Quick thinking is starting: the decision model isn't running yet",
      start: true,
      choose: false,
    });
    expect(engineLine(thought({ engine: "unavailable" }))).toEqual({
      text: "Quick thinking is off: no decision model is set up",
      start: false,
      choose: true,
    });
  });

  it("says a model on the processor is running, not missing, and points to a faster one", () => {
    expect(engineLine(thought({ engine: "cpu" }))).toEqual({
      text: "Quick thinking is off for this: the decision model runs on the processor, too slow to wait for",
      start: false,
      choose: true,
    });
  });
});

const decision = (over: Partial<DecisionEntry> = {}): DecisionEntry => ({
  id: "d1",
  conversation_id: "c1",
  run_id: "r1",
  purpose: "tool-select",
  label: "Which tools?",
  background: false,
  experiment: "fast-slow",
  backend: "local:Clef-flash",
  model: "Clef-Flash-Q4_K_M",
  ms: 38,
  calibrated: false,
  answers: { web_search: { p: 0.02 } },
  input_tokens: 100,
  state_preview: "",
  action: "acted",
  at: 1,
  note: null,
  outcomes: [],
  labels: [],
  ...over,
});

const turn = (decisions: DecisionEntry[], over: Partial<Thought> = {}, back: string[] = []): TurnThought => ({
  runId: "r1",
  from: 0,
  thought: thought(over),
  back,
  decisions,
});

describe("the receipt (SEE-T-6)", () => {
  it("counts the decisions from the log and the tokens from the held specs, and lists only those that acted", () => {
    const r = receiptOf(
      turn(
        [
          decision({ note: "Left out web search; you did not ask for it" }),
          decision({ id: "d2", purpose: "steer-class" }),
          decision({ id: "d3", purpose: "ret-sufficiency" }),
        ],
        { held: [{ id: "web_search", tokens: 2900 }] }
      )
    )!;
    expect(r.line).toBe(`3 quick decisions · prompt ${(2900).toLocaleString()} tokens shorter`);
    expect(r.rows.map((x) => x.text)).toEqual(["Left out web search; you did not ask for it"]);
    expect(r.quiet).toBe(2);
  });

  it("says nothing it did not measure: no tokens line without held specs, none for groups that came back", () => {
    expect(receiptOf(turn([decision({ note: "x" })]))!.line).toBe("1 quick decision");
    const held = { held: [{ id: "web_search", tokens: 500 }] };
    expect(receiptOf(turn([decision()], held, ["web_search"]))!.line).toBe("1 quick decision");
    expect(tokensShorter(thought(held), [])).toBe(500);
    expect(tokensShorter(thought(held), ["web_search"])).toBe(0);
  });

  it("is nothing when no decision was made, or the log has not been read yet", () => {
    expect(receiptOf(turn([]))).toBeNull();
    expect(receiptOf({ ...turn([]), decisions: undefined })).toBeNull();
  });

  it("puts backend, model, time and how sure on the row's hover, and nowhere else", () => {
    const r = receiptOf(turn([decision({ note: "Left out web search", answers: { web_search: { p: 0.97 } } })]))!;
    expect(r.rows[0].hover).toBe("local:Clef-flash · Clef-Flash-Q4_K_M · 38 ms · sure 0.97");
    expect(r.rows[0].text).not.toMatch(/\d\.\d/);
  });

  it("knows a row already marked wrong from the receipt, and not one marked by another source", () => {
    const mark = (source: string) => decision({ note: "x", labels: [{ label: "wrong", value: null, source }] });
    expect(receiptOf(turn([mark("receipt")]))!.rows[0].wrong).toBe(true);
    expect(receiptOf(turn([mark("need-tools:web_search")]))!.rows[0].wrong).toBe(false);
  });

  it("names the groups still out on the tool-menu row, for the label", () => {
    const r = receiptOf(
      turn([decision({ note: "x" })], { held: [{ id: "web_search", tokens: 1 }, { id: "apps", tokens: 1 }] }, ["apps"])
    )!;
    expect(r.rows[0].units).toEqual(["web_search"]);
  });

  // `SEE-1`: the sentences the answer checked itself on are a part of the line, not decisions.
  describe("with the check of the answer's sentences", () => {
    const checked = (over: Partial<VerifyCheck> = {}, claims: Partial<VerifyClaim>[] = []): VerifyCheck => ({
      message_id: "m1",
      conversation_id: "c1",
      created_at: 1,
      claims: claims.map((c) => ({ finding: "supported", ...c }) as VerifyClaim),
      note: null,
      skipped: 0,
      mode: "auto",
      summary: { sentences: 9, asked: 9, answered: 9, over: 0, pool: 6, sources: 4, factual: 0 },
      ...over,
    });

    it("adds the count of sentences, and keeps the sentence asks out of the decision count", () => {
      const asks = Array.from({ length: 9 }, (_, i) => decision({ id: `q${i}`, purpose: "claim-checkable" }));
      const r = receiptOf(
        turn([decision({ note: "Left out web search" }), ...asks, decision({ id: "v", purpose: "claim-verdict" })], { held: [{ id: "web_search", tokens: 2900 }] }),
        undefined,
        checked({}, [{ finding: "refuted" }, { finding: "supported" }])
      )!;
      expect(r.line).toBe(`1 quick decision · prompt ${(2900).toLocaleString()} tokens shorter · 9 sentences checked`);
      expect(r.rows.map((x) => x.text)).toEqual(["Left out web search", "Checked 9 sentences against 4 sources, 1 contradicted"]);
      expect(r.rows[1].summary).toBe(true);
      expect(r.quiet).toBe(0);
    });

    it("is a receipt of its own when no other decision was made", () => {
      const r = receiptOf(turn([]), undefined, checked({ summary: { sentences: 2, asked: 2, answered: 2, over: 0, pool: 0, sources: 0, factual: 0 } }))!;
      expect(r.line).toBe("2 sentences checked");
      expect(r.rows.map((x) => x.text)).toEqual(["Checked 2 sentences against this run's steps"]);
    });

    it("says nothing it did not count: a deep check, or sentences nobody answered", () => {
      expect(receiptOf(turn([]), undefined, checked({ mode: "deep", summary: null }))).toBeNull();
      expect(receiptOf(turn([]), undefined, checked({ summary: { sentences: 3, asked: 3, answered: 0, over: 0, pool: 2, sources: 1, factual: 0 } }))).toBeNull();
      expect(receiptOf(turn([decision()]), undefined, null)!.line).toBe("1 quick decision");
    });

    it("still lists a sentence mark that changed something, with a way to say it was wrong", () => {
      const r = receiptOf(
        turn([decision({ id: "u", purpose: "claim-checkable", note: "Marked a sentence: No step in this run did this" })]),
        undefined,
        checked({}, [{ finding: "undone" }])
      )!;
      expect(r.rows[0]).toMatchObject({ id: "u", text: "Marked a sentence: No step in this run did this" });
      expect(r.rows[0].summary).toBeUndefined();
    });
  });

  it("offers a plan step to reopen only while the step is still ticked", () => {
    const plan = (status: "done" | "todo"): PlanView => ({
      revisions: 0,
      items: [
        { text: "a", status: "done" },
        { text: "b", status: "done" },
        { text: "c", status },
      ],
    });
    const tick = decision({ purpose: "plan-done", note: "Looks finished from the answer: plan step 3" });
    expect(receiptOf(turn([tick]), plan("done"))!.rows[0].step).toBe(2);
    expect(receiptOf(turn([tick]), plan("todo"))!.rows[0].step).toBeNull();
    expect(receiptOf(turn([tick]))!.rows[0].step).toBeNull();
    expect(receiptOf(turn([decision({ note: "Looks finished from the answer: plan step 3" })]), plan("done"))!.rows[0].step).toBeNull();
  });
});
