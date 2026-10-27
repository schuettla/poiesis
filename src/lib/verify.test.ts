/**
 * `USE-UI-7`: the words and the underline of Verify. Everything here is a pure
 * function, so the screens only have to draw what these return.
 */
import { describe, expect, it } from "vitest";
import type { VerifyCheck, VerifyClaim, VerifyPassage } from "./api";
import {
  answeredFromKnowledge,
  canVerify,
  counts,
  howWords,
  markClaims,
  marksOf,
  needsRevising,
  progressText,
  sourceLine,
  summaryLine,
  trustSegments,
  underlined,
} from "./verify";

const claim = (over: Partial<VerifyClaim> = {}): VerifyClaim => ({
  id: "c1",
  text: "The tower is 330 metres tall.",
  quote: "330 metres tall",
  source: null,
  importance: 2,
  kind: "numeric",
  finding: "supported",
  p: 0.93,
  how: "fast",
  settled: true,
  fast: "supported",
  slow: null,
  reason: null,
  passages: [],
  decision: "d1",
  mark: null,
  ...over,
});

describe("what can be checked", () => {
  it("leaves out a line too short to hold a claim", () => {
    expect(canVerify("Done.")).toBe(false);
    expect(canVerify("   ")).toBe(false);
    expect(canVerify("The Eiffel Tower is 330 metres tall and opened in 1889.")).toBe(true);
  });
});

describe("the summary line", () => {
  it("names only what there is", () => {
    const claims = [
      claim({ id: "a" }),
      claim({ id: "b" }),
      claim({ id: "c", finding: "refuted" }),
      claim({ id: "d", finding: "insufficient" }),
    ];
    expect(summaryLine(claims)).toBe("2 supported · 1 refuted · 1 not confirmed");
    expect(summaryLine([claim({ finding: "conflicting" }), claim({ finding: "unchecked" })])).toBe(
      "1 in dispute · 1 not checked",
    );
    expect(summaryLine([])).toBe("No claims to check");
  });

  it("counts the claims a review decided", () => {
    const c = counts([claim({ how: "reviewed" }), claim({ how: "fast" }), claim({ how: "none", finding: "unchecked" })]);
    expect(c).toMatchObject({ supported: 2, unchecked: 1, reviewed: 1 });
  });
});

describe("Revise", () => {
  it("is offered for a refuted, disputed or unconfirmed claim", () => {
    expect(needsRevising([claim()])).toBe(false);
    for (const finding of ["refuted", "conflicting", "insufficient"] as const) {
      expect(needsRevising([claim({ finding })])).toBe(true);
    }
  });

  it("is not offered for a finding the person called wrong", () => {
    expect(needsRevising([claim({ finding: "refuted", mark: "wrong" })])).toBe(false);
    expect(needsRevising([claim({ finding: "refuted", mark: "right" })])).toBe(true);
  });
});

describe("how a finding was reached", () => {
  it("says reviewed, quick, quick and unsure, or no evidence", () => {
    expect(howWords(claim({ how: "reviewed" }))).toBe("Reviewed against the evidence");
    expect(howWords(claim())).toBe("Quick verdict (0.93)");
    expect(howWords(claim({ settled: false, p: 0.41 }))).toBe("Quick verdict, not sure (0.41)");
    expect(howWords(claim({ how: "none", finding: "insufficient", p: null }))).toBe("No evidence found");
    expect(howWords(claim({ how: "none", finding: "unchecked", p: null }))).toBe("No verdict");
  });
});

describe("the source line", () => {
  const passage = (over: Partial<VerifyPassage> = {}): VerifyPassage => ({
    text: "t",
    source: "wikipedia.org",
    url: null,
    date: "2024-03-05",
    retrieved_at: new Date(2026, 9, 23, 12).getTime(),
    via: "web",
    ...over,
  });

  it("gives the source, the date it states and the day it was read", () => {
    expect(sourceLine(passage())).toBe("wikipedia.org · dated 2024-03-05 · read 23 Oct");
  });

  it("leaves out a date the source did not give", () => {
    expect(sourceLine(passage({ date: null }))).toBe("wikipedia.org · read 23 Oct");
  });
});

describe("the progress row", () => {
  it("counts claims, then reviews", () => {
    expect(progressText(null)).toBe("Reading the answer");
    expect(progressText({ phase: "reading" })).toBe("Reading the answer");
    expect(progressText({ phase: "checking", done: 4, total: 13 })).toBe("Checking 13 claims · 4 done");
    expect(progressText({ phase: "checking", done: 0, total: 1 })).toBe("Checking 1 claim · 0 done");
    expect(progressText({ phase: "reviewing", done: 1, total: 3 })).toBe("Reviewing 2 of 3 claims");
    expect(progressText({ phase: "reviewing", done: 3, total: 3 })).toBe("Reviewing 3 of 3 claims");
    expect(progressText({ phase: "reviewing", done: 0, total: 0 })).toBe("Reviewing");
  });
});

describe("which claims are underlined", () => {
  it("skips supported unless asked, never an unchecked claim, never one with no words to mark", () => {
    expect(underlined(claim(), false)).toBe(false);
    expect(underlined(claim(), true)).toBe(true);
    expect(underlined(claim({ finding: "refuted" }), false)).toBe(true);
    expect(underlined(claim({ finding: "insufficient" }), false)).toBe(true);
    expect(underlined(claim({ finding: "unchecked" }), true)).toBe(false);
    expect(underlined(claim({ finding: "refuted", quote: "" }), true)).toBe(false);
  });

  it("carries the finding and a short hover title", () => {
    const [mark] = marksOf([claim({ finding: "refuted", how: "reviewed" })], false);
    expect(mark).toMatchObject({ id: "c1", finding: "refuted", how: "reviewed" });
    expect(mark.title).toBe("Refuted, reviewed. Click for the evidence.");
  });
});

describe("marking the words in the drawn answer", () => {
  const tree = () => ({
    type: "root",
    children: [
      {
        type: "paragraph",
        children: [
          { type: "text", value: "It stands about 330 metres\ntall and opened in 1889." },
          { type: "strong", children: [{ type: "text", value: "Bold words." }] },
        ],
      },
    ],
  });
  const mark = (id: string, quote: string) => ({
    id,
    quote,
    finding: "refuted" as const,
    how: "fast" as const,
    title: "t",
  });

  it("wraps the first place the quote appears and keeps the text around it", () => {
    const t = tree();
    markClaims(t, [mark("c1", "330 metres tall")]);
    const kids = t.children[0].children as { type: string; value?: string; children?: { value: string }[]; data?: any }[];
    expect(kids.map((k) => k.type)).toEqual(["text", "claimMark", "text", "strong"]);
    expect(kids[0].value).toBe("It stands about ");
    expect(kids[1].children![0].value).toBe("330 metres\ntall");
    expect(kids[1].data.hName).toBe("mark");
    expect(kids[1].data.hProperties).toMatchObject({ className: ["vf-mark", "vf-refuted"], dataClaim: "c1" });
    expect(kids[2].value).toBe(" and opened in 1889.");
  });

  it("finds a quote inside bold text and ignores case", () => {
    const t = tree();
    markClaims(t, [mark("c1", "BOLD words")]);
    const strong = (t.children[0].children as any[])[1];
    expect(strong.children.map((k: { type: string }) => k.type)).toEqual(["claimMark", "text"]);
  });

  it("marks nothing when the words are not there, and uses a place once", () => {
    const t = tree();
    markClaims(t, [mark("c1", "the statue of liberty"), mark("c2", "opened in 1889"), mark("c3", "opened in 1889")]);
    const marks = (t.children[0].children as { type: string; data?: any }[]).filter((k) => k.type === "claimMark");
    expect(marks.map((m) => m.data.hProperties.dataClaim)).toEqual(["c2"]);
  });

  it("does not trip over characters that mean something in a pattern", () => {
    const t = {
      type: "root",
      children: [{ type: "paragraph", children: [{ type: "text", value: "It costs $5.00 (about) now." }] }],
    };
    markClaims(t, [mark("c1", "$5.00 (about)")]);
    expect((t.children[0].children as { type: string }[]).some((k) => k.type === "claimMark")).toBe(true);
  });
});

// ---- `SEE-1`, `SEE-UI-1`: the check every answer gets ----

const sentence = (over: Partial<VerifyClaim> = {}) =>
  claim({ id: "s1", kind: "fact", how: "fast", ...over });

const check = (claims: VerifyClaim[], over: Partial<VerifyCheck> = {}): VerifyCheck => ({
  message_id: "m1",
  conversation_id: "c1",
  created_at: 1,
  claims,
  note: null,
  skipped: 0,
  mode: "auto",
  summary: { sentences: 9, asked: 9, answered: 9, over: 0, pool: 4, sources: 2, factual: 0 },
  ...over,
});

describe("the trust line (SEE-UI-1)", () => {
  it("counts what there is, in words, in the plan's order", () => {
    const claims = [
      ...Array.from({ length: 7 }, (_, i) => sentence({ id: `b${i}`, finding: "supported" })),
      sentence({ id: "n", finding: "insufficient" }),
      sentence({
        id: "x",
        finding: "refuted",
        passages: [{ text: "t", source: "nytimes.com", url: null, date: null, retrieved_at: 1, via: "web" }],
      }),
    ];
    expect(trustSegments(claims).map((s) => s.text)).toEqual([
      "✓ 7 backed by what I read",
      "1 nothing backs it",
      "✕ 1 contradicts nytimes.com",
    ]);
    expect(trustSegments(claims).map((s) => s.key)).toEqual(["backed", "nothing", "contradicted"]);
  });

  it("has a word for several, for disagreement and for something said done that no step did", () => {
    const claims = [
      sentence({ id: "a", finding: "insufficient" }),
      sentence({ id: "b", finding: "insufficient" }),
      sentence({ id: "c", finding: "conflicting" }),
      sentence({ id: "d", finding: "refuted" }),
      sentence({ id: "e", finding: "refuted" }),
      sentence({ id: "f", finding: "undone", kind: "action", how: "ledger" }),
    ];
    expect(trustSegments(claims).map((s) => s.text)).toEqual([
      "2 have nothing to back them",
      "1 where sources disagree",
      "✕ 2 contradict what I read",
      "✕ 1 said done, no step did it",
    ]);
    expect(trustSegments([])).toEqual([]);
  });

  it("says the answer came from what I know only when nothing was read and nothing was marked", () => {
    const none = check([], { summary: { sentences: 4, asked: 0, answered: 0, over: 0, pool: 0, sources: 0, factual: 3 } });
    expect(answeredFromKnowledge(none)).toBe(true);
    expect(answeredFromKnowledge({ ...none, summary: { ...none.summary!, pool: 2 } })).toBe(false);
    expect(answeredFromKnowledge({ ...none, summary: { ...none.summary!, factual: 0 } })).toBe(false);
    expect(answeredFromKnowledge({ ...none, claims: [sentence({ finding: "undone" })] })).toBe(false);
    expect(answeredFromKnowledge({ ...none, mode: "deep" })).toBe(false);
  });

  it("counts the sentences down while the check runs", () => {
    expect(progressText({ phase: "auto", done: 4, total: 9 })).toBe("Checking against what I read · 4 of 9");
    expect(progressText({ phase: "auto", done: 0, total: 0 })).toBe("Checking against what I read");
  });

  it("asks to revise when a sentence is red or amber, and a sentence no step did counts", () => {
    expect(needsRevising([sentence({ finding: "undone" })])).toBe(true);
    expect(needsRevising([sentence({ finding: "supported" })])).toBe(false);
    expect(summaryLine([sentence({ finding: "undone" })])).toBe("1 not done");
    expect(counts([sentence({ finding: "undone" })]).undone).toBe(1);
  });

  it("words a sentence matched against the run's steps", () => {
    const c = sentence({ finding: "undone", how: "ledger" });
    expect(howWords(c)).toBe("Matched against what this run did");
  });
});

describe("the sweep (SEE-UI-1)", () => {
  it("draws a backed sentence for a moment while it is sweeping, and not after", () => {
    const backed = sentence({ finding: "supported" });
    expect(marksOf([backed], false)).toEqual([]);
    const swept = marksOf([backed], false, new Set(["s1"]));
    expect(swept).toHaveLength(1);
    expect(swept[0]).toMatchObject({ id: "s1", finding: "supported", sweep: true });
  });

  it("keeps a backed sentence marked, without the sweep, when asked to underline what is supported", () => {
    const [m] = marksOf([sentence({ finding: "supported" })], true, new Set(["s1"]));
    expect(m.sweep).toBeUndefined();
  });

  it("marks the sweep with its own class on the words", () => {
    const tree = { type: "root", children: [{ type: "paragraph", children: [{ type: "text", value: "Paris is the capital." }] }] };
    markClaims(tree, [{ id: "s1", quote: "Paris is the capital.", finding: "supported", how: "fast", title: "t", sweep: true }]);
    const kid = (tree.children[0].children as { data?: { hProperties: { className: string[] } } }[])[0];
    expect(kid.data!.hProperties.className).toEqual(["vf-mark", "vf-supported", "vf-sweep"]);
  });

  it("marks a sentence that has bold in it by its start, as far as it goes in one piece", () => {
    const tree = {
      type: "root",
      children: [
        {
          type: "paragraph",
          children: [
            { type: "text", value: "The Eiffel Tower is " },
            { type: "strong", children: [{ type: "text", value: "330 metres" }] },
            { type: "text", value: " tall." },
          ],
        },
      ],
    };
    markClaims(tree, [
      { id: "s1", quote: "The Eiffel Tower is 330 metres tall.", finding: "refuted", how: "fast", title: "t" },
    ]);
    const kids = tree.children[0].children as { type: string; children?: { value: string }[] }[];
    expect(kids.map((k) => k.type)).toEqual(["claimMark", "text", "strong", "text"]);
    expect(kids[0].children![0].value).toBe("The Eiffel Tower is");
  });
});
