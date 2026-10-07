/** Every word must appear. Lower is better: a word at the start of the text
 * beats one at a word boundary, which beats one mid-word.
 *
 * Shared by the Ctrl K palette and the composer's `/` menu (`CMP-2`), so the two
 * rank the same way and nobody has to learn two kinds of "close enough". */
export function score(text: string, words: string[]): number | null {
  const t = text.toLowerCase();
  let total = 0;
  for (const w of words) {
    const i = t.indexOf(w);
    if (i < 0) return null;
    total += i === 0 ? 0 : /[\s\-_/\.:]/.test(t[i - 1]) ? 1 : 2;
  }
  return total;
}

export function ranked<T>(items: T[], text: (item: T) => string, words: string[], limit: number): T[] {
  return items
    .map((item, order) => ({ item, order, s: score(text(item), words) }))
    .filter((r): r is { item: T; order: number; s: number } => r.s !== null)
    .sort((a, b) => a.s - b.s || a.order - b.order)
    .slice(0, limit)
    .map((r) => r.item);
}
