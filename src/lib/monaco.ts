import * as monaco from "monaco-editor/esm/vs/editor/editor.api";
// Every Monarch grammar Monaco ships (~80 languages), colouring only. The
// *language services* (`vs/language/{typescript,json,css,html}`) are
// deliberately left out: they would drag in a ~10 MB TypeScript compiler and
// then decorate a file opened out of its project with red squiggles for every
// import it cannot resolve — diagnostics that are noise here, since this tab
// edits a file, it does not open a project. Errors in this app come from the
// agent's own build, in the diagnostics rows.
import "monaco-editor/esm/vs/basic-languages/monaco.contribution";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";

export { monaco };

/**
 * Monaco, wired for this app.
 *
 * Two constraints shape every choice below.
 *
 * **The CSP.** `tauri.conf.json` sets `script-src 'self' 'unsafe-inline'` with
 * no `blob:`, and `worker-src` falls back to `script-src`. Monaco's stock
 * loader builds its worker by wrapping a bootstrap in a `Blob` URL, which that
 * policy blocks outright. Vite's `?worker` import instead emits the worker as
 * a real same-origin chunk, so `new Worker(url)` needs no blob and no
 * exception in the policy. This is the same reason `lib/highlight.ts` runs
 * Shiki's JavaScript regex engine rather than the Oniguruma WASM one.
 *
 * **The palette.** Monaco cannot read CSS variables — `defineTheme` wants
 * literal hex. So the theme is *built* from the computed values of the
 * `--hl-*` tokens in `tokens.css` and rebuilt when the mode flips, which keeps
 * one palette for the whole app instead of a second one living in here.
 */

// Monaco asks for a worker by label; with no language services registered, the
// only one it ever asks for is the base editor worker (word-based suggestions,
// link detection, diff).
(self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
};

export const THEME = "poiesis";

/** A `--custom-property`'s computed value. Custom properties resolve nested
 * `var()` before they are reported, so `--hl-token-comment: var(--ink-muted)`
 * arrives here as a literal colour. */
function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** Monaco token rules want bare hex — `1d2b24`, never `#1d2b24` — while the
 * `colors` map wants the `#`. Anything that isn't a plain hex triple (a
 * `color-mix`, an `oklch`, an empty string from a missing token) is dropped so
 * one unparseable value cannot take the whole theme down with it. */
function bare(name: string, fallback: string): string {
  const v = cssVar(name);
  return /^#[0-9a-f]{6}$/i.test(v) ? v.slice(1) : fallback;
}

function hash(name: string, fallback: string): string {
  const v = cssVar(name);
  return /^#[0-9a-f]{3,8}$/i.test(v) ? v : fallback;
}

/**
 * Define (or redefine) the app's Monaco theme from the current mode's tokens.
 *
 * Monarch's token names are coarser than the TextMate scopes Shiki produced,
 * so this is a mapping rather than a translation: `number`, `regexp` and the
 * boolean-ish keywords all land on the one `--hl-token-constant` the palette
 * has, and `delimiter`/`operator` recede into muted ink the way punctuation
 * already does in the read-only view. A file therefore looks the same whether
 * it is being read or edited, which is the point — the tab does not visibly
 * change medium when you click into it.
 */
export function defineTheme(): void {
  const punctuation = bare("--hl-token-punctuation", "5a6560");
  const constant = bare("--hl-token-constant", "95501f");
  const fn = bare("--hl-token-function", "2a5d6b");
  const ink = hash("--ink", "#17201c");
  const edge = hash("--paper-edge", "#e6eae6");

  monaco.editor.defineTheme(THEME, {
    // `vs` vs `vs-dark` decides the *unstyled* defaults — widget chrome, the
    // find box, scrollbar shadows — so it has to follow the mode even though
    // every colour we care about is overridden below.
    base: document.documentElement.getAttribute("data-mode") === "dark" ? "vs-dark" : "vs",
    inherit: true,
    rules: [
      { token: "", foreground: bare("--hl-foreground", "17201c") },
      { token: "keyword", foreground: bare("--hl-token-keyword", "3d4fa0") },
      { token: "keyword.control", foreground: bare("--hl-token-keyword", "3d4fa0") },
      { token: "string", foreground: bare("--hl-token-string", "3b6a4e") },
      { token: "string.escape", foreground: constant },
      { token: "comment", foreground: bare("--hl-token-comment", "5a6560"), fontStyle: "italic" },
      { token: "number", foreground: constant },
      { token: "regexp", foreground: constant },
      { token: "constant", foreground: constant },
      { token: "type", foreground: fn },
      { token: "type.identifier", foreground: fn },
      { token: "namespace", foreground: fn },
      { token: "tag", foreground: bare("--hl-token-keyword", "3d4fa0") },
      { token: "attribute.name", foreground: fn },
      { token: "attribute.value", foreground: bare("--hl-token-string", "3b6a4e") },
      { token: "metatag", foreground: punctuation },
      { token: "annotation", foreground: constant },
      { token: "delimiter", foreground: punctuation },
      { token: "operator", foreground: punctuation },
      { token: "variable", foreground: bare("--hl-token-parameter", "17201c") },
      { token: "identifier", foreground: bare("--hl-foreground", "17201c") },
    ],
    colors: {
      // The panel is flat by design (see the header of `tokens.css`), so the
      // editor takes the paper it sits on and is separated by hairlines only.
      "editor.background": hash("--paper", "#ffffff"),
      "editor.foreground": ink,
      "editorGutter.background": hash("--paper", "#ffffff"),
      "editorLineNumber.foreground": hash("--ink-faint", "#8a938e"),
      "editorLineNumber.activeForeground": hash("--ink-muted", "#5a6560"),
      "editorCursor.foreground": ink,
      "editorIndentGuide.background1": edge,
      "editorIndentGuide.activeBackground1": hash("--paper-edge-2", "#b8c0b8"),
      "editorWidget.background": hash("--paper", "#ffffff"),
      "editorWidget.border": hash("--paper-edge-2", "#b8c0b8"),
      "editorSuggestWidget.background": hash("--paper", "#ffffff"),
      "editorSuggestWidget.border": hash("--paper-edge-2", "#b8c0b8"),
      "editorHoverWidget.background": hash("--paper", "#ffffff"),
      "editorHoverWidget.border": hash("--paper-edge-2", "#b8c0b8"),
      "editorOverviewRuler.border": "#00000000",
      "editor.lineHighlightBorder": "#00000000",
      "editor.lineHighlightBackground": hash("--canvas", "#f3f5f3"),
    },
  });
}

/**
 * The Monaco language id for a file path, from the extension and filename
 * tables the grammars register themselves with — so `Dockerfile`, `.zshrc` and
 * `CMakeLists.txt` resolve the same way an extension does, with no second
 * alias table to keep in step with `lib/highlight.ts`.
 */
export function languageForPath(path: string): string {
  const name = path.split(/[\\/]/).pop()?.toLowerCase() ?? "";
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 ? name.slice(dot) : "";
  for (const lang of monaco.languages.getLanguages()) {
    if (lang.filenames?.some((f) => f.toLowerCase() === name)) return lang.id;
    if (lang.extensions?.some((e) => e.toLowerCase() === ext)) return lang.id;
  }
  return "plaintext";
}

/** The line ending a file already uses, so saving a CRLF file back does not
 * silently rewrite every line of it. Monaco normalises a model to one EOL, and
 * without this it would pick the platform default rather than the file's. */
export function eolOf(content: string): monaco.editor.EndOfLineSequence {
  const crlf = (content.match(/\r\n/g) ?? []).length;
  const lf = (content.match(/\n/g) ?? []).length - crlf;
  return crlf > lf
    ? monaco.editor.EndOfLineSequence.CRLF
    : monaco.editor.EndOfLineSequence.LF;
}
