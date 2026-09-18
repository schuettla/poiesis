import { describe, expect, it } from "vitest";
import type { FileChange } from "./api";
import { languageForPath, languageForTitle, tokenize, tokenizeDiff } from "./highlight";

const text = (line: { content: string }[]) => line.map((t) => t.content).join("");

describe("syntax highlighting", () => {
  it("finds a language from an extension, an alias or a file name", async () => {
    expect(await languageForPath("src/cart.ts")).toBe("typescript");
    expect(await languageForPath("C:\\code\\main.rs")).toBe("rust");
    expect(await languageForPath("include/util.hpp")).toBe("cpp");
    expect(await languageForPath("Dockerfile")).toBe("dockerfile");
    expect(await languageForPath("notes.txt")).toBeNull();
    expect(await languageForPath("LICENSE")).toBeNull();
  });

  it("reads a code artifact's language from its title", async () => {
    expect(await languageForTitle("fizzbuzz.py")).toBe("python");
    expect(await languageForTitle("A parser in Rust")).toBe("rust");
    // Short aliases are words in prose, not languages.
    expect(await languageForTitle("A small c helper")).toBeNull();
  });

  it("colours tokens from the app's palette variables, one array per line", async () => {
    const tokens = await tokenize('const a = "x";\n// done\n', "ts");
    expect(tokens).not.toBeNull();
    expect(tokens!.map(text)).toEqual(['const a = "x";', "// done", ""]);
    const keyword = tokens![0].find((t) => t.content === "const");
    expect(keyword?.color).toBe("var(--hl-token-keyword)");
    const comment = tokens![1].find((t) => t.content.includes("done"));
    expect(comment?.color).toBe("var(--hl-token-comment)");
  });

  it("leaves unknown languages and huge files plain", async () => {
    expect(await tokenize("hello", "not-a-language")).toBeNull();
    expect(await tokenize("x\n".repeat(9000), "ts")).toBeNull();
  });

  it("colours each diff line in its own side's grammar, in hunk order", async () => {
    const file: FileChange = {
      path: "src/format.ts",
      display: "src/format.ts",
      status: "modified",
      from: null,
      added: 1,
      removed: 1,
      binary: false,
      too_large: false,
      entry_ids: [],
      last_at: 0,
      hunks: [
        {
          old_start: 1,
          old_lines: 3,
          new_start: 1,
          new_lines: 3,
          lines: [
            { kind: "context", text: "export function f() {", old_no: 1, new_no: 1 },
            { kind: "removed", text: "  return 1;", old_no: 2, new_no: null },
            { kind: "added", text: '  return "1";', old_no: null, new_no: 2 },
            { kind: "context", text: "}", old_no: 3, new_no: 3 },
          ],
        },
      ],
    };
    const tokens = await tokenizeDiff(file, "typescript");
    expect(tokens).not.toBeNull();
    expect(tokens![0].map(text)).toEqual(file.hunks[0].lines.map((l) => l.text));
    expect(tokens![0][2].some((t) => t.color === "var(--hl-token-string-expression)")).toBe(true);
  });
});
