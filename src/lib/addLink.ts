/** `MOD-7` door 2: one field that takes whatever the user pasted and says what
 * it is before anything is fetched. Pure, so every accepted shape is tested. */

export type LinkKind =
  | { kind: "hf-repo"; repo: string; label: string }
  | { kind: "github"; repo: string; label: string }
  | { kind: "file"; url: string; filename: string; label: string }
  | { kind: "invalid"; label: string }
  | { kind: "empty"; label: "" };

const REPO_ID = /^[A-Za-z0-9][\w.-]*\/[\w.-]+$/;
const CHAT_EXT = [".gguf"];
const IMAGE_EXT = [".safetensors", ".gguf", ".ckpt"];

/** What the pasted text is. `tab` decides which file types count: the chat
 * tab takes GGUF; the images tab takes diffusion checkpoints. */
export function classifyModelLink(raw: string, tab: "chat" | "media"): LinkKind {
  const input = raw.trim();
  if (!input) return { kind: "empty", label: "" };
  const exts = tab === "chat" ? CHAT_EXT : IMAGE_EXT;
  const extList = exts.join(", ");

  if (/^https?:\/\//i.test(input)) {
    let url: URL;
    try {
      url = new URL(input);
    } catch {
      return { kind: "invalid", label: "That doesn't look like a complete link." };
    }
    const path = url.pathname.replace(/\/+$/, "");
    const filename = decodeURIComponent(path.split("/").pop() ?? "");
    const lower = filename.toLowerCase();
    if (exts.some((e) => lower.endsWith(e))) {
      return { kind: "file", url: input, filename, label: `Direct file · ${filename}` };
    }
    const host = url.hostname.replace(/^www\./, "");
    const parts = path.split("/").filter(Boolean);
    if (host === "huggingface.co" && parts.length >= 2 && tab === "chat") {
      const repo = `${parts[0]}/${parts[1]}`;
      return { kind: "hf-repo", repo, label: `Hugging Face repo · ${repo}` };
    }
    if (host === "github.com" && parts.length >= 2 && tab === "chat") {
      const repo = `${parts[0]}/${parts[1]}`;
      return { kind: "github", repo, label: `GitHub repo · ${repo}` };
    }
    return {
      kind: "invalid",
      label:
        tab === "chat"
          ? "Paste a Hugging Face or GitHub repo, or a link that ends in .gguf."
          : `Paste a direct link to a model file (${extList}).`,
    };
  }

  if (REPO_ID.test(input)) {
    if (tab === "media") {
      return { kind: "invalid", label: `Paste a direct link to a model file (${extList}).` };
    }
    return { kind: "hf-repo", repo: input, label: `Hugging Face repo · ${input}` };
  }
  return {
    kind: "invalid",
    label:
      tab === "chat"
        ? "Paste a repo like owner/name, a huggingface.co link, or a link that ends in .gguf."
        : `Paste a direct link to a model file (${extList}).`,
  };
}

/** The sentence shown when a repo holds no GGUF files. */
export function noGgufMessage(repo: string): string {
  return `No GGUF files in ${repo}. It may hold the original weights. Look for a repo ending in -GGUF.`;
}
