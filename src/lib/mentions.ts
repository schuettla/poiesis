import type { Artifact } from "./api";
import type { Attachment, ContextRef } from "./types";

/**
 * `@` in the composer: point at a file from the working folder or something from
 * the library. Everything here is plain text and lists, so it can be tested
 * without the window.
 */

export interface MentionToken {
  /** Index of the `@`. */
  start: number;
  /** End of the word the caret is in. */
  end: number;
  /** What was typed after the `@`, up to the caret. */
  query: string;
}

/** The `@word` the caret is in, or null. An `@` counts only at the start or
 * after whitespace, so an email address is not a mention. */
export function mentionAt(text: string, caret: number): MentionToken | null {
  const m = /(^|\s)@([^\s@]*)$/.exec(text.slice(0, caret));
  if (!m) return null;
  const tail = /^[^\s@]*/.exec(text.slice(caret))?.[0] ?? "";
  return { start: caret - m[2].length - 1, end: caret + tail.length, query: m[2] };
}

function isSubsequence(q: string, text: string): boolean {
  let at = 0;
  for (const ch of text) if (ch === q[at] && ++at === q.length) return true;
  return q.length === 0;
}

/** The files that match `query`, best first: a name that starts with it, then a
 * name that holds it, then a path that holds it, then its letters in order. */
export function rankFiles(files: string[], query: string, limit: number): string[] {
  const q = query.toLowerCase().replace(/\\/g, "/");
  if (!q) return files.slice(0, limit);
  const hits: Array<[number, string]> = [];
  for (const path of files) {
    const lower = path.toLowerCase();
    const name = lower.slice(lower.lastIndexOf("/") + 1);
    const score = name.startsWith(q)
      ? 0
      : name.includes(q)
        ? 1
        : lower.includes(q)
          ? 2
          : isSubsequence(q, lower)
            ? 3
            : -1;
    if (score >= 0) hits.push([score, path]);
  }
  hits.sort((a, b) => a[0] - b[0] || a[1].length - b[1].length || a[1].localeCompare(b[1]));
  return hits.slice(0, limit).map((h) => h[1]);
}

/** The library items that match `query`; with nothing typed, the newest. */
export function rankLibrary(items: Artifact[], query: string, limit: number): Artifact[] {
  const q = query.trim().toLowerCase();
  const newest = [...items].sort((a, b) => b.created_at - a.created_at);
  if (!q) return newest.slice(0, limit);
  const starts = newest.filter((a) => a.title.toLowerCase().startsWith(q));
  const holds = newest.filter((a) => !a.title.toLowerCase().startsWith(q) && a.title.toLowerCase().includes(q));
  return [...starts, ...holds].slice(0, limit);
}

/** `text` with the mention replaced by `insert` and a space after it, and where
 * the caret goes. */
export function insertMention(text: string, token: MentionToken, insert: string): { text: string; caret: number } {
  const tail = text.slice(token.end);
  const glue = tail.startsWith(" ") ? "" : " ";
  const head = text.slice(0, token.start) + insert;
  return { text: head + glue + tail, caret: head.length + 1 };
}

/** `text` with the mention taken out, and no double space left behind. */
export function dropMention(text: string, token: MentionToken): { text: string; caret: number } {
  const before = text.slice(0, token.start);
  const tail = text.slice(token.end);
  const after = before === "" || before.endsWith(" ") ? tail.replace(/^ /, "") : tail;
  return { text: before + after, caret: before.length };
}

/** The file mentions still in the text. One the user has since deleted is not sent. */
export function mentionsStillIn(text: string, mentions: ContextRef[]): ContextRef[] {
  return mentions.filter((m) => text.includes(`@${m.id}`));
}

/** A library item added to a message: a picture goes along as an attachment, as
 * the `+` menu does; anything else is a pointer the model reads itself. */
export function libraryAdd(a: Artifact): { attachment: Attachment } | { ref: ContextRef } {
  if (a.kind === "image") {
    return { attachment: { id: `lib-${a.id}`, kind: "image", name: a.title, path: a.content } };
  }
  return { ref: { kind: "artifact", id: a.id, label: a.title } };
}
