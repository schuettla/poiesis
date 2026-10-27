// The words and the underline of Verify (`USE-UI-7`). Pure, so they can be tested
// without a screen.

import type { Finding, VerifyCheck, VerifyClaim, VerifyPassage, VerifyProgress } from "./api";

/** What the store holds for one answer that was, or is being, checked. */
export interface VerifyEntry {
  running: boolean;
  progress: VerifyProgress | null;
  check: VerifyCheck | null;
  error: string | null;
}

/** The shortest answer that is worth checking: a greeting or a one-liner holds no claim. */
const MIN_CHECKABLE = 40;

export const canVerify = (text: string) => text.trim().length >= MIN_CHECKABLE;

export const FINDING_WORDS: Record<Finding, string> = {
  supported: "Supported",
  refuted: "Refuted",
  insufficient: "Not confirmed",
  conflicting: "Sources disagree",
  unchecked: "Not checked",
  undone: "No step did this",
};

export interface Counts {
  supported: number;
  refuted: number;
  conflicting: number;
  insufficient: number;
  unchecked: number;
  undone: number;
  /** Claims a slow review decided. */
  reviewed: number;
}

export function counts(claims: VerifyClaim[]): Counts {
  const c: Counts = { supported: 0, refuted: 0, conflicting: 0, insufficient: 0, unchecked: 0, undone: 0, reviewed: 0 };
  for (const claim of claims) {
    c[claim.finding] += 1;
    if (claim.how === "reviewed") c.reviewed += 1;
  }
  return c;
}

/** `11 supported · 1 refuted · 1 not confirmed`. Only what there is. */
export function summaryLine(claims: VerifyClaim[]): string {
  if (claims.length === 0) return "No claims to check";
  const c = counts(claims);
  const parts: string[] = [];
  if (c.supported) parts.push(`${c.supported} supported`);
  if (c.refuted) parts.push(`${c.refuted} refuted`);
  if (c.conflicting) parts.push(`${c.conflicting} in dispute`);
  if (c.insufficient) parts.push(`${c.insufficient} not confirmed`);
  if (c.unchecked) parts.push(`${c.unchecked} not checked`);
  if (c.undone) parts.push(`${c.undone} not done`);
  return parts.join(" · ");
}

/** Whether a check found something worth acting on, which is what Revise is for. */
export const needsRevising = (claims: VerifyClaim[]) =>
  claims.some(
    (c) =>
      c.mark !== "wrong" &&
      (c.finding === "refuted" || c.finding === "conflicting" || c.finding === "insufficient" || c.finding === "undone"),
  );

/** How a finding was reached, in the words the card uses. */
export function howWords(c: VerifyClaim): string {
  if (c.how === "ledger") return "Matched against what this run did";
  if (c.how === "reviewed") return "Reviewed against the evidence";
  if (c.how === "fast") {
    const p = c.p === null ? "" : ` (${c.p.toFixed(2)})`;
    return c.settled ? `Quick verdict${p}` : `Quick verdict, not sure${p}`;
  }
  return c.finding === "insufficient" ? "No evidence found" : "No verdict";
}

/** The tooltip on an underlined claim. Short: the card has the rest. */
export function markTitle(c: VerifyClaim): string {
  if (c.how === "ledger") return `${FINDING_WORDS[c.finding]}. Click for the details.`;
  return `${FINDING_WORDS[c.finding]}, ${c.how === "reviewed" ? "reviewed" : "quick verdict"}. Click for the evidence.`;
}

/** A claim is underlined when its finding is worth a look. A supported one only
 * when asked for, and one nothing was said about never. */
export function underlined(c: VerifyClaim, showSupported: boolean): boolean {
  if (!c.quote) return false;
  if (c.finding === "supported") return showSupported;
  return c.finding !== "unchecked";
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** `wikipedia.org · dated 2024-03-05 · read 23 Oct`. */
export function sourceLine(p: VerifyPassage): string {
  const read = new Date(p.retrieved_at);
  const parts = [p.source];
  if (p.date) parts.push(`dated ${p.date}`);
  if (Number.isFinite(read.getTime())) parts.push(`read ${read.getDate()} ${MONTHS[read.getMonth()]}`);
  return parts.join(" · ");
}

export const VIA_WORDS: Record<VerifyPassage["via"], string> = {
  cited: "cited by the answer",
  folder: "your folder",
  web: "the web",
  read: "read for this answer",
  run: "this run's steps",
};

/** The progress row's words. */
export function progressText(p: VerifyProgress | null): string {
  if (p?.phase === "auto") {
    return p.total === 0 ? "Checking against what I read" : `Checking against what I read · ${Math.min(p.done, p.total)} of ${p.total}`;
  }
  if (!p || p.phase === "reading") return "Reading the answer";
  if (p.phase === "checking") {
    return p.total === 0 ? "Looking for claims" : `Checking ${p.total} claim${p.total === 1 ? "" : "s"} · ${p.done} done`;
  }
  return p.total === 0 ? "Reviewing" : `Reviewing ${Math.min(p.done + 1, p.total)} of ${p.total} claims`;
}

// ---- the underline ----

/** What the text plugin needs of a claim. */
export interface Mark {
  id: string;
  quote: string;
  finding: Finding;
  how: VerifyClaim["how"];
  title: string;
  /** Drawn for a moment as the check passes over the text. */
  sweep?: boolean;
}

/** `sweeping` are the backed sentences whose check just arrived (`SEE-UI-1`): they are
 * drawn for a moment as the check passes over the text, and then settle back to plain text.
 * With the toggle on a backed sentence stays marked, and does not sweep. */
export function marksOf(claims: VerifyClaim[], showSupported: boolean, sweeping: ReadonlySet<string> = new Set()): Mark[] {
  return claims
    .filter((c) => underlined(c, showSupported) || (c.finding === "supported" && c.quote && sweeping.has(c.id)))
    .map((c) => ({
      id: c.id,
      quote: c.quote,
      finding: c.finding,
      how: c.how,
      title: markTitle(c),
      ...(c.finding === "supported" && !showSupported && sweeping.has(c.id) ? { sweep: true } : {}),
    }));
}

// ---- the trust line (`SEE-UI-1`) ----

export type TrustKey = "backed" | "nothing" | "disagree" | "contradicted" | "undone";

export interface TrustSegment {
  key: TrustKey;
  text: string;
}

const TRUST_KEY: Record<Finding, TrustKey | null> = {
  supported: "backed",
  insufficient: "nothing",
  conflicting: "disagree",
  refuted: "contradicted",
  undone: "undone",
  unchecked: null,
};

export const trustKeyOf = (c: VerifyClaim): TrustKey | null => TRUST_KEY[c.finding];

/** The line after a check: only what there is, each part a filter on the claim list. */
export function trustSegments(claims: VerifyClaim[]): TrustSegment[] {
  const of = (key: TrustKey) => claims.filter((c) => trustKeyOf(c) === key);
  const out: TrustSegment[] = [];
  const backed = of("backed").length;
  if (backed) out.push({ key: "backed", text: `✓ ${backed} backed by what I read` });
  const nothing = of("nothing").length;
  if (nothing) out.push({ key: "nothing", text: nothing === 1 ? "1 nothing backs it" : `${nothing} have nothing to back them` });
  const disagree = of("disagree").length;
  if (disagree) out.push({ key: "disagree", text: `${disagree} where sources disagree` });
  const wrong = of("contradicted");
  if (wrong.length) {
    const source = wrong.length === 1 ? wrong[0].passages[0]?.source : undefined;
    out.push({
      key: "contradicted",
      text: wrong.length === 1 ? `✕ 1 contradicts ${source ?? "what I read"}` : `✕ ${wrong.length} contradict what I read`,
    });
  }
  const undone = of("undone").length;
  if (undone) out.push({ key: "undone", text: `✕ ${undone} said done, no step did it` });
  return out;
}

/** An automatic check that read nothing and marked nothing: the answer came from what I know. */
export function answeredFromKnowledge(check: VerifyCheck): boolean {
  const s = check.summary;
  return check.mode === "auto" && !!s && s.pool === 0 && s.factual > 0 && check.claims.length === 0;
}

interface Node {
  type: string;
  value?: string;
  children?: Node[];
  data?: Record<string, unknown>;
}

const escapeRegExp = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** The words of a quote as a pattern: any run of white space matches any other, so
 * a line break the markdown kept inside a paragraph still finds its claim. */
function pattern(quote: string): RegExp | null {
  const words = quote.trim().split(/\s+/).filter(Boolean);
  return words.length ? new RegExp(words.map(escapeRegExp).join("\\s+"), "i") : null;
}

/** Wrap the first place each quote appears, whole inside one piece of text, in a
 * `mark` element. A quote that runs across bold or a link is not found, and its
 * claim is simply not underlined (it is still in the list under the answer). A
 * place is used once: a quote inside another's mark stays plain. */
export function markClaims(tree: Node, marks: Mark[]): void {
  for (const mark of marks) {
    const re = pattern(mark.quote);
    if (!re || wrapFirst(tree, re, mark)) continue;
    // A sentence that runs across bold, a link or code is not one piece of text. Its start is
    // marked instead, as far as it goes in one piece.
    const words = mark.quote.trim().split(/\s+/).filter(Boolean);
    for (const n of [8, 5, 4]) {
      if (words.length <= n) continue;
      const head = pattern(words.slice(0, n).join(" "));
      if (head && wrapFirst(tree, head, mark)) break;
    }
  }
}

function wrapFirst(node: Node, re: RegExp, mark: Mark): boolean {
  if (!node.children) return false;
  for (let i = 0; i < node.children.length; i++) {
    const child = node.children[i];
    if (child.type === "claimMark") continue;
    if (child.type === "text" && typeof child.value === "string") {
      const hit = re.exec(child.value);
      if (!hit) continue;
      const before = child.value.slice(0, hit.index);
      const after = child.value.slice(hit.index + hit[0].length);
      const pieces: Node[] = [];
      if (before) pieces.push({ type: "text", value: before });
      pieces.push({
        type: "claimMark",
        data: {
          hName: "mark",
          hProperties: {
            className: ["vf-mark", `vf-${mark.finding}`, ...(mark.sweep ? ["vf-sweep"] : [])],
            dataClaim: mark.id,
            tabIndex: 0,
            title: mark.title,
          },
        },
        children: [{ type: "text", value: hit[0] }],
      });
      if (after) pieces.push({ type: "text", value: after });
      node.children.splice(i, 1, ...pieces);
      return true;
    }
    if (wrapFirst(child, re, mark)) return true;
  }
  return false;
}

/** The remark plugin: `[remarkClaimMarks, marks]`. */
export function remarkClaimMarks(marks: Mark[]) {
  return (tree: unknown) => {
    if (marks.length) markClaims(tree as Node, marks);
  };
}
