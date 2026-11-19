/** Quoting a passage of a rendered document into the composer.
 *
 * Three small jobs live here: stamping the rendered markdown with the source
 * lines it came from, turning a text selection back into a line range, and the
 * words a quote is written as inside a message (and read back from one). */

/** What the composer holds for "ask about this part". */
export interface DocQuote {
  text: string;
  /** The document's name: an artifact's title, or a file's name. */
  source: string;
  from: number;
  to: number;
}

interface MdNode {
  type: string;
  position?: { start: { line: number }; end: { line: number } };
  children?: MdNode[];
  data?: { hProperties?: Record<string, unknown> };
}

/** A remark plugin: every rendered element learns which source lines it came from,
 * as `data-from` / `data-to`. Text nodes have no element to carry them. */
export function remarkSourceLines() {
  const mark = (node: MdNode) => {
    if (node.type !== "text" && node.position) {
      node.data = {
        ...node.data,
        hProperties: {
          ...node.data?.hProperties,
          dataFrom: node.position.start.line,
          dataTo: node.position.end.line,
        },
      };
    }
    node.children?.forEach(mark);
  };
  return (tree: MdNode) => mark(tree);
}

function bounds(node: Node, root: HTMLElement): { from: number; to: number } | null {
  const el = node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement;
  const hit = el?.closest<HTMLElement>("[data-from]");
  if (!hit || !root.contains(hit)) return null;
  return { from: Number(hit.dataset.from), to: Number(hit.dataset.to) };
}

/** The quote a selection inside `root` makes, or null when it is empty or not
 * inside the rendered document.
 *
 * The first and last words picked tell which elements it spans, which is a
 * paragraph's worth of lines at worst. Where those words are found verbatim in
 * the source, the range narrows to the exact lines; where inline markup or a
 * table got in the way, the element's own lines stand. */
export function quoteFromSelection(sel: Selection | null, root: HTMLElement, source: string, name: string): DocQuote | null {
  if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return null;
  const range = sel.getRangeAt(0);
  if (!root.contains(range.commonAncestorContainer)) return null;
  const text = sel.toString().trim();
  if (!text) return null;

  // The first and last text nodes the selection really touches. Asking the range's
  // own ends is wrong for a triple click, which ends at the start of the next block.
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let first: Node | null = null;
  let last: Node | null = null;
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (!n.textContent?.trim() || !range.intersectsNode(n)) continue;
    first ??= n;
    last = n;
  }
  const a = first && bounds(first, root);
  const b = last && bounds(last, root);
  if (!a || !b) return null;

  let { from, to } = { from: a.from, to: b.to };
  const lines = source.split("\n");
  const picked = text.split("\n").map((s) => s.trim()).filter(Boolean);
  const find = (needle: string, lo: number, hi: number, fromEnd: boolean) => {
    if (!needle) return null;
    for (let i = fromEnd ? hi : lo; fromEnd ? i >= lo : i <= hi; i += fromEnd ? -1 : 1) {
      if (lines[i - 1]?.includes(needle)) return i;
    }
    return null;
  };
  const exactFrom = find(picked[0].slice(0, 80), from, to, false);
  if (exactFrom !== null) from = exactFrom;
  const exactTo = find(picked[picked.length - 1].slice(0, 80), from, to, true);
  if (exactTo !== null) to = exactTo;
  return { text, source: name, from, to: Math.max(from, to) };
}

const span = (q: Pick<DocQuote, "from" | "to">) => (q.from === q.to ? `line ${q.from}` : `lines ${q.from}-${q.to}`);

/** What a quote reads as in the sent message: a block quote, then where it is from. */
export function quoteBlock(q: DocQuote): string {
  const body = q.text.split("\n").map((l) => (l.trim() ? `> ${l}` : ">"));
  return [...body, `> (${q.source}, ${span(q)})`].join("\n");
}

/** The short form of a quote, for the chip in the composer. */
export function quoteLabel(q: DocQuote): string {
  const words = q.text.replace(/\s+/g, " ");
  return `“${words.length > 40 ? `${words.slice(0, 40).trimEnd()}…` : words}”`;
}

export function quoteWhere(q: DocQuote): string {
  return `${q.source}, ${span(q)}`;
}

/** The inverse of `quoteBlock`, for a message that opens with one. */
export function splitQuote(text: string): { quote: { text: string; where: string } | null; rest: string } {
  const lines = text.split("\n");
  let end = 0;
  while (end < lines.length && lines[end].startsWith(">")) end++;
  if (end < 2) return { quote: null, rest: text };
  const where = /^> \((.+, lines? \d+(?:-\d+)?)\)$/.exec(lines[end - 1]);
  if (!where) return { quote: null, rest: text };
  const body = lines.slice(0, end - 1).map((l) => l.replace(/^> ?/, ""));
  return { quote: { text: body.join("\n"), where: where[1] }, rest: lines.slice(end).join("\n").replace(/^\n+/, "") };
}
