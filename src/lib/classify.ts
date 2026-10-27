/** `USE-UI-5`: the data of a `classify` block, read leniently, and what the table
 * does with it: what a row shows once a person has corrected it, the low-confidence
 * filter, the sort, and the CSV. Pure, so each can be tried without a screen. */

export interface ClassifyChoice {
  /** What a label carries: the option id, `true` or `false`, or a level's index. */
  value: string;
  /** The words the table shows. */
  label: string;
}

export interface ClassifyRow {
  /** Position in the list, which is what a correction is kept under. */
  index: number;
  item: string;
  path?: string;
  image: boolean;
  /** A small picture as a data URL. The app can only load its own generated
   * images by path, so a user's picture travels in the block. */
  thumb?: string;
  answer?: string;
  value?: string;
  /** The winner's own probability, 0 to 1. */
  confidence?: number;
  low: boolean;
  decision?: string;
  /** Why the item was not sorted. */
  note?: string;
}

export interface ClassifyData {
  question: string;
  kind: "yes_no" | "choice" | "scale";
  choices: ClassifyChoice[];
  /** The confidence under which a row is marked low. */
  bar: number;
  calibrated: boolean;
  total: number;
  notReached: number;
  /** Why the list was cut short, in words, when it was. */
  stopped: string | null;
  rows: ClassifyRow[];
}

type Obj = Record<string, unknown>;
const obj = (x: unknown): Obj => (x && typeof x === "object" ? (x as Obj) : {});
const str = (x: unknown): string | undefined => (typeof x === "string" ? x : undefined);
const num = (x: unknown): number | undefined => (typeof x === "number" && Number.isFinite(x) ? x : undefined);

export function readClassify(data: unknown): ClassifyData {
  const d = obj(data);
  const rows: ClassifyRow[] = (Array.isArray(d.rows) ? d.rows : []).map((raw, index) => {
    const r = obj(raw);
    return {
      index,
      item: str(r.item) ?? "",
      path: str(r.path),
      image: r.image === true,
      // Only a picture, never anything else a block could be made to carry.
      thumb: str(r.thumb)?.startsWith("data:image/") ? str(r.thumb) : undefined,
      answer: str(r.answer),
      value: str(r.value),
      confidence: num(r.confidence),
      low: r.low === true,
      decision: str(r.decision),
      note: str(r.note),
    };
  });
  const choices = (Array.isArray(d.choices) ? d.choices : []).map((c) => {
    const o = obj(c);
    return { value: str(o.value) ?? "", label: str(o.label) ?? str(o.value) ?? "" };
  });
  const kind = d.kind === "yes_no" || d.kind === "scale" ? d.kind : "choice";
  return {
    question: str(d.question) ?? "",
    kind,
    choices,
    bar: num(d.bar) ?? 0.7,
    calibrated: d.calibrated === true,
    total: num(d.total) ?? rows.length,
    notReached: num(d.not_reached) ?? 0,
    stopped: str(d.stopped) ?? null,
    rows,
  };
}

/** The corrections kept on the block, by row index. */
export function readCorrections(state: unknown): Record<string, string> {
  const raw = obj(obj(state).corrections);
  return Object.fromEntries(Object.entries(raw).filter((e): e is [string, string] => typeof e[1] === "string"));
}

/** A row as the table draws it, once a person's correction is taken into account.
 * A corrected row has no confidence: the answer is theirs now, and it is not low. */
export interface Shown {
  answer?: string;
  confidence?: number;
  low: boolean;
  corrected: boolean;
}

export function shown(row: ClassifyRow, data: ClassifyData, corrections: Record<string, string>): Shown {
  const picked = corrections[String(row.index)];
  if (picked !== undefined && picked !== row.value) {
    const label = data.choices.find((c) => c.value === picked)?.label ?? picked;
    return { answer: label, low: false, corrected: true };
  }
  // Choosing the model's own answer is a confirmation: no longer a guess.
  const confirmed = picked !== undefined && picked === row.value;
  return { answer: row.answer, confidence: row.confidence, low: row.low && !confirmed, corrected: false };
}

export type SortKey = "item" | "answer" | "confidence";
export type Sort = { key: SortKey; dir: "asc" | "desc" } | null;

/** A header click: first ascending, then descending, then the original order. */
export function nextSort(current: Sort, key: SortKey): Sort {
  if (!current || current.key !== key) return { key, dir: "asc" };
  return current.dir === "asc" ? { key, dir: "desc" } : null;
}

export type Filter = "all" | "low";

/** How many rows are low confidence now, which a correction takes down. */
export function lowCount(data: ClassifyData, corrections: Record<string, string>): number {
  return data.rows.filter((r) => shown(r, data, corrections).low).length;
}

/** The rows to draw: filtered, then sorted. Rows with nothing to sort by (not
 * sorted, or corrected so with no confidence) go last either way; ties keep the
 * original order. */
export function arrange(data: ClassifyData, corrections: Record<string, string>, filter: Filter, sort: Sort): ClassifyRow[] {
  const rows = data.rows.filter((r) => filter === "all" || shown(r, data, corrections).low);
  if (!sort) return rows;
  const sign = sort.dir === "asc" ? 1 : -1;
  const key = (r: ClassifyRow): string | number | undefined => {
    const s = shown(r, data, corrections);
    return sort.key === "item" ? r.item.toLowerCase() : sort.key === "answer" ? s.answer?.toLowerCase() : s.confidence;
  };
  return [...rows].sort((a, b) => {
    const ka = key(a);
    const kb = key(b);
    if (ka === undefined || kb === undefined) {
      return ka === kb ? a.index - b.index : ka === undefined ? 1 : -1;
    }
    const order = typeof ka === "number" && typeof kb === "number" ? ka - kb : String(ka).localeCompare(String(kb));
    return order === 0 ? a.index - b.index : order * sign;
  });
}

/** A cell for a spreadsheet: quoted when it holds a comma, quote or line break, and
 * made harmless when it starts like a formula (the items are outside text). */
export function csvCell(value: string): string {
  const safe = /^[=+\-@\t\r]/.test(value) ? `'${value}` : value;
  return /[",\r\n]/.test(safe) ? `"${safe.replace(/"/g, '""')}"` : safe;
}

/** The whole list as CSV, with a person's corrections in place of the model's
 * answers and the status of every row: `low confidence`, `corrected`, or why it
 * was not sorted. */
export function toCsv(data: ClassifyData, corrections: Record<string, string>): string {
  const withPath = data.rows.some((r) => r.path);
  const header = ["item", ...(withPath ? ["path"] : []), "answer", "confidence", "status"];
  const lines = data.rows.map((r) => {
    const s = shown(r, data, corrections);
    const status = r.note ? `not sorted: ${r.note}` : s.corrected ? "corrected" : s.low ? "low confidence" : "";
    const cells = [
      r.item,
      ...(withPath ? [r.path ?? ""] : []),
      s.answer ?? "",
      s.confidence === undefined ? "" : s.confidence.toFixed(2),
      status,
    ];
    return cells.map(csvCell).join(",");
  });
  return [header.join(","), ...lines].join("\r\n") + "\r\n";
}

/** A file name for the export: `sorted-what-kind-of-ticket-is-this.csv`. */
export function csvName(question: string): string {
  const slug = question
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/(^-|-$)/g, "")
    .slice(0, 40)
    .replace(/-$/, "");
  return `sorted-${slug || "list"}.csv`;
}
