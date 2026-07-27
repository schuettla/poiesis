// Shared domain types for the Nexus frontend. These mirror the Rust backend's
// serde models; keep field names in sync as backend phases land.

export type Provenance = "local" | "cloud";

export interface Model {
  id: string;
  name: string;
  provenance: Provenance;
  /** Short qualifier shown in the picker, e.g. "fast", "Anthropic". */
  meta?: string;
  /** Whether the model supports image/PDF vision input. */
  vision?: boolean;
  /** True if a local model is downloaded and ready, or a cloud key is present. */
  available?: boolean;
  /** Cloud routing (CLD-3): the provider id and provider-side model id. */
  provider?: string;
  cloudModel?: string;
}

export type Role = "user" | "assistant";

/** A single visible step in an agent run timeline (CHT-9). */
export interface AgentStep {
  id: string;
  /** Plain past-tense verb, e.g. "searched", "read", "edited". */
  verb: string;
  /** Concrete target, e.g. "src/client.rs", "crates.io". */
  target: string;
  /** Optional trailing result, e.g. "— 3 matches". */
  result?: string;
  /** Provenance for a recall step (RCL-UI): what was found, and where. */
  matches?: import("./api").SearchHit[];
  status: "running" | "done" | "error";
}

/** The typed workspace-block kinds the renderer understands (Generative UI).
 * "surface" is reserved: the conversation's live composed interface tree. */
export type BlockKind =
  | "comparison"
  | "collection"
  | "plan"
  | "form"
  | "progress"
  | "document"
  | "surface";

/** One node of the agent-composed interface tree (the dynamic Workspace
 * surface). `type` selects a primitive (stack, grid, section, text, metric,
 * badge, progress, link, item, choice, input, toggle, button, divider); all
 * other fields are primitive-specific and treated leniently by the renderer. */
export interface UINode {
  type: string;
  id?: string;
  children?: UINode[];
  [key: string]: unknown;
}

/** A typed, interactive block rendered inline in an assistant turn. `data` and
 * `state` are kind-specific and treated leniently by the renderer. `messageId`
 * anchors it to the assistant message it belongs to. */
export interface BlockView {
  id: string;
  kind: BlockKind;
  title: string;
  data: unknown;
  state?: unknown;
  messageId?: string | null;
}

export interface Attachment {
  id: string;
  kind: "image" | "pdf";
  name: string;
  /** Filesystem path (file-picker / native drop). Empty for inline data. */
  path: string;
  /** Inline data URI for clipboard-paste / browser-drop images (no path). */
  dataUri?: string;
}

export interface Message {
  id: string;
  role: Role;
  /** For user turns: their text. For assistant turns: the prose conclusion. */
  text: string;
  /** Assistant turns carry the model used and its agent-run timeline. */
  model?: Pick<Model, "name" | "provenance">;
  steps?: AgentStep[];
  /** Typed workspace blocks the assistant produced in this turn (Generative UI). */
  blocks?: BlockView[];
  attachments?: Attachment[];
  /** Change proposals the agent raised during this turn (SOUL-UI-2). Ids only:
   *  the card reads the live proposal so it disappears once answered. */
  proposalIds?: string[];
  /** Artifact ids produced during this turn (CHT-6): rendered as clickable
   *  chips that open the Workbench on the right. */
  artifactIds?: string[];
  /** `file_trash` ids for files this turn changed on disk — rendered as rows
   * that open the file and offer Undo. In-memory only: after a reload the
   * Workbench's "Recent changes" strip is the durable record. */
  fileChangeIds?: string[];
  /** True while the assistant turn is still streaming. */
  streaming?: boolean;
  createdAt: number;
}

export interface Conversation {
  id: string;
  title: string;
  updatedAt: number;
  messages: Message[];
  /** Persona applied to this conversation (CHT-4), if any. */
  personaId?: string | null;
  /** One-off per-conversation overrides (CHT-7). */
  overrides?: { temperature?: number };
  /** Pinned to workspace mode (W): the composed interface is this session's
   * primary surface, the message stream is a demoted log. */
  workspace?: boolean;
  /** Rolling summary standing in for the older turns when talking to the model
   * (CTX-3). Every message is still stored and still shown. */
  summary?: string | null;
  /** Newest message the summary covers; turns after it are sent verbatim. */
  summaryUptoMessageId?: string | null;
  /** When the agent last reflected on this conversation (REF-2), or null if
   * it hasn't yet. Drives the auto-reflection trigger on leaving. */
  reflectedAt?: number | null;
  /** Recipe this conversation was started from (RCP-UI-3). In-memory only —
   * a label on this session, not a fact worth persisting. */
  recipeName?: string;
  /** The real folder on disk this conversation works in, if one is attached.
   * The agent's file tools resolve relative paths against it. */
  folderPath?: string | null;
  /** How much the agent may change inside that folder. Reads are always free. */
  folderTrust?: FolderTrust;
}

/** Per-conversation file-access level, chosen in the Workbench panel. */
export type FolderTrust = "read-only" | "confirm" | "auto";

/** What the Workbench viewer is showing. Files are identified by path;
 * artifacts by id — two origins, one selection. */
export interface WorkbenchSelection {
  kind: "file" | "artifact";
  id: string;
}

export type View = "chat" | "models" | "engine" | "apps" | "settings" | "library" | "self";
export type Mode = "light" | "dark";
export type ModelFilter = "all" | "local";
