import { describe, expect, it } from "vitest";
import type { Artifact } from "./api";
import {
  dropMention,
  insertMention,
  libraryAdd,
  mentionAt,
  mentionsStillIn,
  rankFiles,
  rankLibrary,
} from "./mentions";

const art = (id: string, title: string, created_at: number, kind = "page"): Artifact =>
  ({ id, title, kind, content: `/p/${id}.png`, created_at }) as Artifact;

describe("mentionAt", () => {
  it("finds the word after an @ at the start or after a space", () => {
    expect(mentionAt("@rea", 4)).toEqual({ start: 0, end: 4, query: "rea" });
    expect(mentionAt("fix @src/a", 10)).toEqual({ start: 4, end: 10, query: "src/a" });
    expect(mentionAt("fix @", 5)).toEqual({ start: 4, end: 5, query: "" });
  });

  it("is not an email address, and not a finished word", () => {
    expect(mentionAt("mail me@home.com", 16)).toBeNull();
    expect(mentionAt("fix @a.ts now", 13)).toBeNull();
  });

  it("takes the rest of the word the caret is inside", () => {
    expect(mentionAt("fix @src/main.rs now", 8)).toEqual({ start: 4, end: 16, query: "src" });
  });
});

describe("rankFiles", () => {
  const files = ["README.md", "docs/readme-old.md", "src/main.rs", "src/components/Reader.tsx"];

  it("lists the first files when nothing is typed", () => {
    expect(rankFiles(files, "", 2)).toEqual(["README.md", "docs/readme-old.md"]);
  });

  it("puts a name that starts with it before a name that holds it", () => {
    expect(rankFiles(files, "read", 8)).toEqual(["README.md", "docs/readme-old.md", "src/components/Reader.tsx"]);
    expect(rankFiles(files, "ader", 8)).toEqual(["src/components/Reader.tsx"]);
  });

  it("matches a path, either slash, and letters in order", () => {
    expect(rankFiles(files, "src\\main", 8)).toEqual(["src/main.rs"]);
    // Letters in order also reach a longer path; the shorter one comes first.
    expect(rankFiles(files, "smr", 8)).toEqual(["src/main.rs", "src/components/Reader.tsx"]);
    expect(rankFiles(files, "zzz", 8)).toEqual([]);
  });
});

describe("rankLibrary", () => {
  const items = [art("1", "Trip plan", 10), art("2", "Plan B", 30), art("3", "Budget", 20)];

  it("is the newest first when nothing is typed", () => {
    expect(rankLibrary(items, "", 2).map((a) => a.id)).toEqual(["2", "3"]);
  });

  it("puts a title that starts with it first", () => {
    expect(rankLibrary(items, "plan", 8).map((a) => a.id)).toEqual(["2", "1"]);
  });
});

describe("editing the text", () => {
  it("inserts the path and a space, and puts the caret after it", () => {
    const token = mentionAt("fix @re please", 7)!;
    expect(insertMention("fix @re please", token, "@README.md")).toEqual({
      text: "fix @README.md please",
      caret: 15,
    });
    const end = mentionAt("fix @re", 7)!;
    expect(insertMention("fix @re", end, "@README.md")).toEqual({ text: "fix @README.md ", caret: 15 });
  });

  it("drops a mention without leaving a double space", () => {
    const token = mentionAt("see @tr now", 7)!;
    expect(dropMention("see @tr now", token)).toEqual({ text: "see now", caret: 4 });
  });

  it("sends only the files still in the text", () => {
    const a = { kind: "file", id: "src/a.ts", label: "src/a.ts" } as const;
    const b = { kind: "file", id: "b.md", label: "b.md" } as const;
    expect(mentionsStillIn("fix @src/a.ts", [a, b])).toEqual([a]);
    expect(mentionsStillIn("fix it", [a, b])).toEqual([]);
  });
});

describe("libraryAdd", () => {
  it("sends a picture as an attachment and anything else as a pointer", () => {
    expect(libraryAdd(art("9", "Logo", 1, "image"))).toEqual({
      attachment: { id: "lib-9", kind: "image", name: "Logo", path: "/p/9.png" },
    });
    expect(libraryAdd(art("8", "Notes", 1))).toEqual({ ref: { kind: "artifact", id: "8", label: "Notes" } });
  });
});
