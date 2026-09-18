// Shared domain types for the Poiesis frontend. These mirror the Rust backend's
// serde models; keep field names in sync as backend phases land.

export type Provenance = "local" | "cloud" | "endpoint";

export interface Model {
  id: string;
  name: string;
  provenance: Provenance;
  /** Short qualifier shown in the picker, e.g. "fast", "Anthropic". */
  meta?: string;
  /** Whether the model supports image/PDF vision input. */
  vision?: boolean;
  /** Whether the model can be given tools at all. Absent means "assume yes" —
   * only the cloud catalog reports this, and only OpenRouter reports it
   * per-model. A model with `false` here can still chat; it just can't run the
   * agent loop's tools. */
  tools?: boolean;
  /** True if a local model is downloaded and ready, or a cloud key is present. */
  available?: boolean;
  /** Cloud routing (CLD-3): the provider id and provider-side model id. For an
   * `"endpoint"` model, `provider` carries the endpoint id instead. */
  provider?: string;
  cloudModel?: string;
  /** Endpoint-only: which connected server this model came from, and how
   * much context to send it (`/v1/models` doesn't report a context window,
   * so this is whatever the user set when they added the endpoint). */
  endpointId?: string;
  endpointLabel?: string;
  ctxSize?: number;
  /** What sending a message to this model produces (`PIK-1`). Absent = chat. */
  modality?: "chat" | "image" | "video";
  /** Media only: the backend that serves it, and its price tag. */
  backendId?: string;
  backendLabel?: string;
  priceLabel?: string;
  supportsEdit?: boolean;
  supportedAspectRatios?: string[];
  /** `PIK-4`: the option lists the advanced disclosure is built from, so an
   * unsupported combination is never offered rather than offered and then
   * silently remapped. */
  supportedResolutions?: string[];
  maxDurationSecs?: number;
  /** Cloud chat only: USD per million tokens in / out, when known. */
  promptPerMtok?: number;
  outputPerMtok?: number;
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
  /** The snippet behind a Code Execution step (`DAT-UI-1`), shown only on
   * demand through the same `⌄` disclosure `matches` uses. */
  code?: { language: string; code: string };
  /** Outside content this step fed to the model, wrapped rather than trusted
   * (`TRU-1`/`TRU-2`). One entry per source a call wrapped — a retrieval call
   * can wrap several file excerpts in one step. Drives the `◇ from outside`
   * chip and its disclosure (`TRU-UI-1`). */
  untrusted?: { label: string; risk: number; flags: string[]; text: string }[];
  /** `HRN-UI-2`: the batch of steps this one is running alongside. Steps that
   * share a group are drawn as one "3 things at once" band, because that is
   * what actually happened — a stack that fills one row at a time would show
   * concurrent work as sequential. */
  parallelGroup?: string;
  /** `RPC-1`: the step this one happened inside. A snippet in the sandbox can
   * call tools itself, and forty of those arriving as ordinary rows would read
   * as forty things the agent decided to do. Drawn indented under their step,
   * they read as what they are: the work of the one step above. */
  nestedUnder?: string;
  /** `HRN-8`/`HRN-UI-4`: this step's output was too big to paste, so it was
   * kept on disk. The model saw a preview; this is the whole thing. */
  kept?: { reference: string; bytes: number; text: string };
  /** `COD-UI-2`: a project task this step ran. Live while it runs (the newest
   * line it printed), then the outcome and its diagnostics. */
  task?: {
    name: string;
    argv: string[];
    cwd: string;
    kind: import("./api").TaskKind;
    lastLine?: string;
    outcome?: string;
    exitCode?: number | null;
    durationMs?: number;
    diagnostics?: import("./api").Diagnostic[];
    tail?: string;
  };
  status: "running" | "done" | "error";
}

/** One delegated child agent, as the Fleet card and Agents tab see it
 * (`SUB-UI-1`). Assembled live from `sub_spawned` / `sub` / `sub_ended`, and
 * rehydrated from `subagent_runs` after a reload.
 *
 * `steps` and `text` are the child's *own* — a child's step never lands in the
 * lead's timeline, which is the whole reason its events arrive wrapped. */
export interface SubRun {
  runId: string;
  /** The child's own conversation: its transcript, artifacts and blocks. */
  conversationId: string;
  /** The conversation whose turn started it. */
  parentConversationId: string;
  /** The agent type: a persona name, or "general". */
  agent: string;
  task: string;
  status: import("./api").SubRunStatus;
  /** Why it ended, once it has (`HRN-3`'s vocabulary). */
  stopReason?: import("./api").StopReason;
  steps: AgentStep[];
  /** The child's prose so far, and at the end its whole report. */
  text: string;
  startedAt: number;
  endedAt?: number;
  /** Elapsed milliseconds as the backend measured them, once it has ended. */
  ms?: number;
  /** True while a steer sent to this child has not been picked up yet. */
  /** `HRN-UI-2`: step ids this child announced as one parallel batch, mapped
   * to their group, so its timeline bands them the same way the lead's does. */
  parallelPending?: Record<string, string>;
  steerPending?: boolean;
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
  | "table"
  | "diagnostics"
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
  kind: "image" | "pdf" | "video";
  name: string;
  /** Filesystem path (file-picker / native drop). Empty for inline data. */
  path: string;
  /** Inline data URI for clipboard-paste / browser-drop images (no path). */
  dataUri?: string;
  /** The artifact this attachment renders, for the action row (Refine, Save, …). */
  artifactId?: string;
  width?: number;
  height?: number;
  durationSecs?: number;
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
  /** Set while a generation is in flight so the stream can hold a tile at the
   *  final aspect ratio instead of reflowing when the media lands (`STR-2`).
   *  Cleared the moment the attachment arrives — or the turn fails. */
  pendingMedia?: {
    modality: "image" | "video";
    aspectRatio?: string;
    startedAt: number;
    /** The background job (`JOB-1`), so the tile can offer Cancel. */
    jobId?: string;
  };
  /** True while the assistant turn is still streaming. */
  streaming?: boolean;
  /** `HRN-UI-1`: a user turn typed while the agent was already working.
   * `pending` until the run picks it up at the top of its next iteration,
   * `delivered` after. Nothing is queued for a later turn — the run reads it
   * mid-flight, which is why the mark says so. */
  midRun?: "pending" | "delivered";
  /** `HRN-3`: why an assistant turn stopped. Absent or `completed` means the
   * model finished; anything else means the text is what the run had in hand. */
  stopReason?: import("./api").StopReason;
  /** `SUB-UI-1`: run ids of the agents this turn handed work to, in the order
   * the lead asked for them. Drives the Fleet card. */
  subRunIds?: string[];
  /** `PLN-UI-1`: the plan the run behind this turn wrote and worked through.
   * Absent for a turn that never planned — most of them. Persisted with the
   * turn (`PLN-UI-5`), so reopening the conversation brings it back. */
  plan?: import("./api").PlanView;
  createdAt: number;
}

export interface Conversation {
  id: string;
  /** `SUB-3`: set when this conversation is a delegated child's workspace. The
   * Rail hides these — they belong to the turn that started them. */
  parentConversationId?: string | null;
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
  /** Skill this conversation was started from (`SKL-5`, was RCP-UI-3). In-memory only —
   * a label on this session, not a fact worth persisting. */
  skillName?: string;
  /** The real folder on disk this conversation works in, if one is attached.
   * The agent's file tools resolve relative paths against it. */
  folderPath?: string | null;
  /** How much the agent may change inside that folder. Reads are always free. */
  folderTrust?: FolderTrust;
  /** `PRJ-1`: the project this session belongs to, or null for a loose chat.
   * With one set, the project owns the folder and the trust level and the two
   * fields above are only the fallback. */
  projectId?: string | null;
}

/** Per-conversation file-access level, chosen in the Workbench panel. */
export type FolderTrust = "read-only" | "confirm" | "auto";

/** `PRJ-1`: a named group of sessions that share a context.
 *
 * A working directory is something a project may *have* (`PRJ-1a`), not what
 * it is — a project about a book, a job or a person is as real as one about a
 * repository, and none of those live in a folder.
 *
 * Nobody has to create one either way. Attaching a folder to a loose chat
 * creates or joins the project for that folder, so the user who never thinks
 * about projects still gets one — and gets the trust they granted back, the
 * second time they open that folder. */
export interface Project {
  id: string;
  name: string;
  /** Canonical, and unique across projects. `null` for a project that is not
   * about a directory (`PRJ-1a`), which is most of them. */
  rootPath: string | null;
  /** `PRJ-7`: free text carried into every session in this project. */
  instructions?: string | null;
  trust: FolderTrust;
  /** `COD-7`: the project's own policy, or `inherit` for the Settings default. */
  execPolicy: "off" | "ask" | "allow" | "inherit";
  /** `COD-1` detection result, as stored JSON. The header reads the parsed
   * card through `projectCards` instead. */
  cardJson?: string | null;
  /** `PRJ-UI-2`: the open tab set for this project, filling `SHL-17`'s scope
   * seam. Kept in memory so switching projects swaps what is open without a
   * round trip to the database. */
  tabsJson?: string | null;
  /** Hidden from the Rail. Nothing on disk is ever touched — there is no
   * delete, because the word would be read as "delete my code". */
  archived: boolean;
  updatedAt: number;
}

/** What the Workbench viewer is showing. Files are identified by path;
 * artifacts by id — two origins, one selection. */
export interface WorkbenchSelection {
  kind: "file" | "artifact";
  id: string;
}

/** One single thing open as a tab in the header strip (`SHL-22`). `file` and
 * `artifact` mirror `WorkbenchSelection` — the same two origins, one
 * selection, able to sit open as more than one at a time. `run` is one child
 * agent, not the fleet. An overview of many of them is never an item: it is a
 * `DockView`, navigated inside the sidebar.
 *
 * An open item remembers the chat it belongs to: `conversationId` is stamped
 * when the tab opens, and the strip only shows the live chat's items, since
 * the strip stands over that chat's sidebar. Callers opening something in the
 * current chat can leave it out. */
export type ItemRef = (
  /** `line` is where to scroll (`COD-UI-2`). It is not part of the item's
   * identity: a second click on another line of the same file refocuses the
   * one tab and moves it. */
  | { kind: "file"; id: string; line?: number }
  | { kind: "artifact"; id: string }
  | { kind: "run"; id: string }
  /** `PRJ-UI-3`: one file's patch, full width. The id is the file's absolute
   * path, which still means something after a reload: the change set is
   * rebuilt from the item's own chat. */
  | { kind: "diff"; id: string }
) & { conversationId?: string };

/** The right sidebar's sub-views (`SHL-21`): the overviews it navigates from
 * its own row, the way Settings navigates its sections. `changes` is
 * `PRJ-UI-3`: every file the agent changed, as patches. */
export type DockView = "files" | "artifacts" | "agents" | "browser" | "changes";

/** Every route, as a value rather than only a type.
 *
 * `View` used to be a hand-written union with no runtime counterpart, so
 * nothing could *check* a string against it. The list stays derived from the
 * same source as the type, which is what keeps `HUB_VIEWS` — and anything else
 * that enumerates routes — from drifting away from the routes that exist. */
export const ALL_VIEWS = [
  "chat",
  "models",
  "providers",
  "runtime",
  "apps",
  "settings",
  "library",
  "self",
  "tasks",
  "activity",
  "skills",
  "workingdir",
  "mail",
  "tools",
  "usage",
  "about",
  // `PRJ-UI-4`: the project view. A route rather than a window takeover, so it
  // opens beside your chats and closes without losing them. Which project it
  // shows is `activeProjectId`, the same way `chat` reads
  // `activeConversationId` — a route names a surface, not an instance.
  "project",
  // The projects overview: every project as a card, reached from the Rail's
  // "Projects" button. `project` (above) is one project's own page; this is
  // the list you land on before picking one.
  "projects",
] as const;

export type View = (typeof ALL_VIEWS)[number];

/** The settings hub's sections: everything reached from the cog.
 *
 * Data, so it lives here rather than with the component that draws it — the
 * store needs it too (to collapse the whole hub onto one route tab) and
 * importing a route module from the store would drag every settings panel into
 * its module graph. `SettingsHub` re-exports this as `HUB_TABS`. */
export const HUB_SECTIONS: { view: View; label: string; icon: string }[] = [
  { view: "settings", label: "General", icon: "⚙" },
  { view: "models", label: "Models", icon: "▤" },
  { view: "providers", label: "Providers", icon: "⌁" },
  { view: "runtime", label: "Runtime", icon: "◧" },
  { view: "tools", label: "Tools", icon: "⚒" },
  { view: "skills", label: "Skills", icon: "▦" },
  { view: "apps", label: "Apps", icon: "◇" },
  { view: "self", label: "Self", icon: "" },
  { view: "tasks", label: "Tasks", icon: "◷" },
  { view: "mail", label: "Mail", icon: "✉" },
  { view: "activity", label: "Activity", icon: "≡" },
  { view: "usage", label: "Usage", icon: "◔" },
  { view: "workingdir", label: "Working dir", icon: "▥" },
  { view: "about", label: "About", icon: "ⓘ" },
];

/** What a project is called before the user names it (`PRJ-UI-1a`). Also the
 * signal that the project view should open with the name selected for typing.
 * Mirrors the same literal in `commands/projects.rs`, which is where a project
 * created in the desktop app gets its fallback name. */
export const NEW_PROJECT_NAME = "New project";

const HUB_VIEWS = new Set<View>(HUB_SECTIONS.map((t) => t.view));

/** Does this view belong to the hub?
 *
 * `App` used to answer this with its own hand-written list of the same view
 * names, which meant every new section had to be added in two places. Usage
 * was added to the sections and not to that list, so choosing it rendered
 * *nothing* — no hub, no nav, no content, just an empty window — for as long
 * as the section had existed. Derived, so the two cannot drift again. */
export function isHubView(view: View): boolean {
  return HUB_VIEWS.has(view);
}

/* `routeTabFor` and `isView` lived here to serve a persisted list of open route
   tabs. `SHL-24` removed that list — a route is a destination again — and with
   it the only thing that ever needed to validate a view name read off disk. */
export type Mode = "light" | "dark";
/** Shared by the picker and the Models page (`MOD-6`), so choosing "On this
 * PC" in one place holds in both. */
export type ModelFilter = "all" | "local" | "cloud";

/** The Runtime page's tabs (`RTM-8`). `servers` is the deep link used by the
 * picker and the Models page's "Your server" groups. */
export type RuntimeTab = "chat" | "images" | "servers" | "recall";
