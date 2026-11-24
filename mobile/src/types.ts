/**
 * What the phone knows about the desktop's data. Hand written, not shared with the desktop's `lib/api.ts`
 * (plan, decision 8). The event shapes are checked against `src/fixtures/*.json`, which the desktop's own
 * test keeps in step with what Rust sends: when an event changes there, both tests say so.
 */

export type LinkState = "connecting" | "connected" | "notfound";

export interface PhoneState {
  paired: boolean;
  name: string | null;
  link: LinkState;
}

export interface ChatRow {
  id: string;
  title: string;
  updated_at: number;
  remote_sender: string | null;
  remote_transport: string | null;
}

export interface Attachment {
  kind: string;
  name: string;
  path: string;
}

export interface PhoneMessage {
  id: string;
  role: string;
  content: string;
  created_at: number;
  attachments?: Attachment[];
}

export interface PhoneArtifact {
  id: string;
  title: string;
  kind: string;
  content: string;
  created_at: number;
}

/** What a permission event asks. Only the fields a card shows. */
export interface Approval {
  id: string;
  summary: string;
  path: string;
  diff?: string;
  argv?: string[];
  project?: string;
  /** The chat the question belongs to, when the phone knows it. */
  conversationId: string | null;
}

/** An entry the desktop keeps for the person: a change to accept or a question to answer. */
export interface Waiting {
  id: string;
  [key: string]: unknown;
}

/** The events of a turn the phone reads. Everything else is ignored (plan, II.2). */
export type PhoneEvent =
  | { type: "chat"; conversation_id: string }
  | { type: "steered" }
  | { type: "remote_notice"; text: string }
  | { type: "turn_saved"; user_message_id: string | null; assistant_message_id: string }
  | { type: "turn_finalized"; message_id: string }
  | { type: "step_start"; verb: string }
  | { type: "token"; text: string }
  | { type: "error"; message: string }
  | { type: "permission"; request: Omit<Approval, "conversationId"> }
  | { type: "permission_gone"; id: string }
  | { type: "artifact"; id: string; title: string; kind: string; content: string }
  | { type: "block"; id: string; kind: string; title: string }
  | { type: "sub"; run_id: string };

const str = (v: unknown): v is string => typeof v === "string";

/** Read one event from the desktop, or `null` for a type the phone does not use or a broken one. */
export function readEvent(raw: unknown): PhoneEvent | null {
  if (!raw || typeof raw !== "object") return null;
  const e = raw as Record<string, unknown>;
  switch (e.type) {
    case "chat":
      return str(e.conversation_id) ? { type: "chat", conversation_id: e.conversation_id } : null;
    case "steered":
      return { type: "steered" };
    case "remote_notice":
      return str(e.text) ? { type: "remote_notice", text: e.text } : null;
    case "turn_saved":
      return str(e.assistant_message_id)
        ? {
            type: "turn_saved",
            user_message_id: str(e.user_message_id) ? e.user_message_id : null,
            assistant_message_id: e.assistant_message_id,
          }
        : null;
    case "turn_finalized":
      return str(e.message_id) ? { type: "turn_finalized", message_id: e.message_id } : null;
    case "step_start":
      return str(e.verb) ? { type: "step_start", verb: e.verb } : null;
    case "token":
      return str(e.text) ? { type: "token", text: e.text } : null;
    case "error":
      return str(e.message) ? { type: "error", message: e.message } : null;
    case "permission": {
      const r = e.request as Record<string, unknown> | undefined;
      if (!r || !str(r.id) || !str(r.summary)) return null;
      return {
        type: "permission",
        request: {
          id: r.id,
          summary: r.summary,
          path: str(r.path) ? r.path : "",
          diff: str(r.diff) ? r.diff : undefined,
          argv: Array.isArray(r.argv) ? r.argv.filter(str) : undefined,
          project: str(r.project) ? r.project : undefined,
        },
      };
    }
    case "permission_gone":
      return str(e.id) ? { type: "permission_gone", id: e.id } : null;
    case "artifact":
      return str(e.id) && str(e.title) && str(e.kind) && str(e.content)
        ? { type: "artifact", id: e.id, title: e.title, kind: e.kind, content: e.content }
        : null;
    case "block":
      return str(e.id) && str(e.kind) && str(e.title) ? { type: "block", id: e.id, kind: e.kind, title: e.title } : null;
    case "sub":
      return str(e.run_id) ? { type: "sub", run_id: e.run_id } : null;
    default:
      return null;
  }
}

/** The one line the status strip shows, in words. Never ids, never relay names. */
export function statusLine(link: LinkState, desktop: string | null): string {
  switch (link) {
    case "connected":
      return desktop ? `Connected to ${desktop}` : "Connected";
    case "connecting":
      return "Looking for your desktop ...";
    case "notfound":
      return "Your desktop is asleep or Poiesis is not running.";
  }
}
