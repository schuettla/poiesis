import { describe, expect, it } from "vitest";
import { classifyModelLink, noGgufMessage } from "./addLink";

describe("the add-by-link field (MOD-7)", () => {
  it("takes a Hugging Face repo id", () => {
    expect(classifyModelLink("bartowski/Qwen2.5-7B-Instruct-GGUF", "chat")).toEqual({
      kind: "hf-repo",
      repo: "bartowski/Qwen2.5-7B-Instruct-GGUF",
      label: "Hugging Face repo · bartowski/Qwen2.5-7B-Instruct-GGUF",
    });
  });

  it("takes a full huggingface.co link and reduces it to the repo", () => {
    const k = classifyModelLink("https://huggingface.co/bartowski/Qwen2.5-7B-Instruct-GGUF/tree/main", "chat");
    expect(k).toMatchObject({ kind: "hf-repo", repo: "bartowski/Qwen2.5-7B-Instruct-GGUF" });
  });

  it("takes a GitHub repo link", () => {
    expect(classifyModelLink("https://github.com/owner/models", "chat")).toMatchObject({
      kind: "github",
      repo: "owner/models",
    });
  });

  it("sends a direct .gguf link straight to download", () => {
    const k = classifyModelLink(
      "https://huggingface.co/a/b/resolve/main/model-Q4_K_M.gguf?download=true",
      "chat"
    );
    expect(k).toMatchObject({ kind: "file", filename: "model-Q4_K_M.gguf" });
  });

  it("says what's wrong with junk", () => {
    expect(classifyModelLink("hello there", "chat").kind).toBe("invalid");
    expect(classifyModelLink("https://example.com/page", "chat").kind).toBe("invalid");
    expect(classifyModelLink("   ", "chat").kind).toBe("empty");
  });

  it("on the images tab takes checkpoint files, not repos", () => {
    expect(classifyModelLink("https://x.io/sdxl.safetensors", "media")).toMatchObject({ kind: "file" });
    expect(classifyModelLink("https://x.io/flux.ckpt", "media")).toMatchObject({ kind: "file" });
    expect(classifyModelLink("owner/repo", "media").kind).toBe("invalid");
    expect(classifyModelLink("https://x.io/model.gguf", "media")).toMatchObject({ kind: "file" });
  });

  it("explains a repo with no GGUF files", () => {
    expect(noGgufMessage("meta/llama")).toContain("Look for a repo ending in -GGUF");
  });
});
