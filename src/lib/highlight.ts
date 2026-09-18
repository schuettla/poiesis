import { useEffect, useState, type CSSProperties } from "react";
import type { HighlighterCore, ThemedToken } from "shiki/core";
import type { FileChange } from "./api";

export type { ThemedToken };

/**
 * Syntax colouring for code files, code artifacts and diffs, via Shiki (the
 * VS Code TextMate grammars).
 *
 * Two choices keep it inside the app's rules:
 * - The JavaScript regex engine, not the Oniguruma WASM one: the CSP has no
 *   `wasm-unsafe-eval`, so the WASM engine would not load.
 * - A CSS-variables theme: a token's colour is `var(--hl-token-…)`, so the
 *   palette lives in `tokens.css` and follows light and dark mode with no
 *   re-highlight.
 *
 * Shiki and each grammar are loaded on first use, so none of it is in the
 * startup bundle. Anything that fails or is too big stays plain text.
 */

const THEME = "poiesis";
/** Past these a file stays plain: tokenizing is main-thread work. */
const MAX_CHARS = 400_000;
const MAX_LINES = 8_000;

// Names Shiki does not know as extensions, mapped to one it does.
const EXT_ALIAS: Record<string, string> = {
  h: "c",
  hh: "cpp",
  hpp: "cpp",
  hxx: "cpp",
  cc: "cpp",
  cxx: "cpp",
  htm: "html",
  bash: "shellscript",
  conf: "ini",
  cfg: "ini",
  gradle: "groovy",
  svg: "xml",
  csproj: "xml",
  props: "xml",
  plist: "xml",
};
const FILE_NAMES: Record<string, string> = {
  dockerfile: "dockerfile",
  makefile: "makefile",
  justfile: "just",
  "cargo.lock": "toml",
  ".env": "dotenv",
  "cmakelists.txt": "cmake",
};

let highlighter: Promise<HighlighterCore> | null = null;
let langs: Promise<typeof import("shiki/langs")> | null = null;

function loadLangs() {
  return (langs ??= import("shiki/langs"));
}

function getHighlighter(): Promise<HighlighterCore> {
  return (highlighter ??= (async () => {
    const [{ createHighlighterCore, createCssVariablesTheme }, { createJavaScriptRegexEngine }] = await Promise.all([
      import("shiki/core"),
      import("shiki/engine/javascript"),
    ]);
    return createHighlighterCore({
      themes: [createCssVariablesTheme({ name: THEME, variablePrefix: "--hl-", fontStyle: true })],
      langs: [],
      // Skip the few grammar patterns JS regexes cannot express rather than
      // failing the whole language.
      engine: createJavaScriptRegexEngine({ forgiving: true }),
    });
  })());
}

/** The Shiki language id for a file path, or null when there is none. */
export async function languageForPath(path: string): Promise<string | null> {
  const name = path.split(/[\\/]/).pop()?.toLowerCase() ?? "";
  if (FILE_NAMES[name]) return FILE_NAMES[name];
  const dot = name.lastIndexOf(".");
  if (dot < 0) return null;
  const ext = name.slice(dot + 1);
  return languageFor(EXT_ALIAS[ext] ?? ext);
}

/**
 * The language of a code artifact, from its title: a file name in it
 * (`fizzbuzz.py`) or a language named in it (`Parser in Rust`). Short aliases
 * like `c` or `r` are not read out of prose, where they would be words.
 */
export async function languageForTitle(title: string): Promise<string | null> {
  for (const word of title.split(/[\s()[\]{},:;"'`]+/)) {
    if (word.includes(".")) {
      const byPath = await languageForPath(word);
      if (byPath) return byPath;
    }
  }
  for (const word of title.toLowerCase().split(/[^a-z0-9#+-]+/)) {
    if (word.length < 4) continue;
    const id = await languageFor(word);
    if (id) return id;
  }
  return null;
}

/** Resolve a language name or alias (`ts`, `py`, `rust`) to a canonical id. */
export async function languageFor(name: string): Promise<string | null> {
  const key = name.trim().toLowerCase();
  if (!key) return null;
  const { bundledLanguagesInfo } = await loadLangs();
  const info = bundledLanguagesInfo.find((l) => l.id === key || l.aliases?.includes(key));
  return info?.id ?? null;
}

/** Tokens per line, or null when the text should stay plain. */
export async function tokenize(code: string, lang: string): Promise<ThemedToken[][] | null> {
  if (code.length > MAX_CHARS || countLines(code) > MAX_LINES) return null;
  const id = await languageFor(lang);
  if (!id) return null;
  const [h, { bundledLanguages }] = await Promise.all([getHighlighter(), loadLangs()]);
  if (!h.getLoadedLanguages().includes(id)) {
    await h.loadLanguage(bundledLanguages[id as keyof typeof bundledLanguages]);
  }
  return h.codeToTokensBase(code, {
    lang: id,
    theme: THEME,
    // A minified line would stall the view; past this it is left uncoloured.
    tokenizeMaxLineLength: 4_000,
    tokenizeTimeLimit: 500,
  });
}

function countLines(code: string): number {
  let n = 1;
  for (let i = 0; i < code.length; i++) if (code.charCodeAt(i) === 10) n++;
  return n;
}

/**
 * Tokens for one diff's lines, in the same order as the hunk lines. Each hunk
 * is coloured as two texts, the old side (context and removed lines) and the
 * new side (context and added lines), so a line is read in the grammar state
 * of its own version of the file. A hunk starts fresh, so a comment opened
 * above the hunk is not known to it.
 */
export async function tokenizeDiff(file: FileChange, lang: string): Promise<ThemedToken[][][] | null> {
  const out: ThemedToken[][][] = [];
  for (const hunk of file.hunks) {
    const oldSide = hunk.lines.filter((l) => l.kind !== "added");
    const newSide = hunk.lines.filter((l) => l.kind !== "removed");
    const [oldTokens, newTokens] = await Promise.all([
      tokenize(oldSide.map((l) => l.text).join("\n"), lang),
      tokenize(newSide.map((l) => l.text).join("\n"), lang),
    ]);
    if (!oldTokens || !newTokens) return null;
    let o = 0;
    let n = 0;
    out.push(
      hunk.lines.map((l) => {
        if (l.kind === "removed") return oldTokens[o++] ?? [];
        if (l.kind === "context") o++;
        return newTokens[n++] ?? [];
      }),
    );
  }
  return out;
}

/**
 * Run an async highlight and hold its result; null until it lands, and again
 * whenever the inputs change, so the plain text shows in the meantime.
 */
export function useTokens<T>(run: () => Promise<T | null>, deps: unknown[]): T | null {
  const [value, setValue] = useState<T | null>(null);
  useEffect(() => {
    let cancelled = false;
    setValue(null);
    run()
      .then((v) => !cancelled && setValue(v))
      .catch(() => {
        // A grammar that fails to load leaves the text plain; nothing to tell.
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return value;
}

/** A token's inline style: the theme colour plus italic/bold/underline bits. */
export function tokenStyle(t: ThemedToken): CSSProperties | undefined {
  const fs = t.fontStyle ?? 0;
  if (!t.color && fs <= 0) return undefined;
  return {
    color: t.color,
    fontStyle: fs & 1 ? "italic" : undefined,
    fontWeight: fs & 2 ? 600 : undefined,
    textDecoration: fs & 4 ? "underline" : undefined,
  };
}
