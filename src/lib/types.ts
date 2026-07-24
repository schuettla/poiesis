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
}

export type View = "chat" | "models" | "engine" | "apps" | "settings" | "library";
export type Mode = "light" | "dark";
export type ModelFilter = "all" | "local";
