import { create } from "zustand";
import type {
  AgentStep,
  Attachment,
  BlockView,
  Conversation,
  FolderTrust,
  Message,
  Mode,
  Model,
  ModelFilter,
  Provenance,
  View,
  WorkbenchSelection,
} from "./types";
import { mockConversations, mockModels } from "./mockData";
import * as api from "./api";
import { budgetTurns, withSummary, KEEP_RECENT, KEEP_RECENT_WORKSPACE } from "./context";

const SYSTEM_PROMPT_KEY = "system_prompt";
const READING_SCALE_KEY = "reading_scale";
const TELEMETRY_KEY = "telemetry_enabled";
const AUTOCOMPACT_KEY = "context.autocompact";
const MEMORY_ONBOARDED_KEY = "memory.onboarded";
const REFLECT_AUTO_KEY = "reflection.auto";
const SELF_BORN_KEY = "self.born";
const SELF_INTRODUCED_KEY = "self.introduced";
const DOCK_OPEN_KEY = "workbench.open";
const DOCK_WIDTH_KEY = "workbench.width";
const EXPERT_KEY = "ui.expert";
const RECALL_DECLINED_KEY = "recall.declined";
/** SMP-4c: folder reading explains itself once, the first time it happens. */
const INDEX_EXPLAINED_KEY = "index.explained";
/** SMP-7a: keys the generalized `maybeFirstTime` helper tracks, each backed by
 * its own `onboarded.<key>` setting. `folder` (`SMP-4c`) and the original
 * memory-write explainer (`MEM-UI-4`, `memoryOnboarded`) predate this helper
 * and render inline where the ability itself is shown rather than as a toast
 * — left as they are rather than forced through a shared shell that doesn't
 * fit their context. */
const FIRST_TIME_KEYS = ["recall", "retrieval", "digest", "proposal"] as const;
/** How long a first-time explanation toast stays up before clearing itself —
 * independent of whether it's still mounted, so it can never get stuck behind
 * a memory-write or heal toast that outlives it. */
const EXPLAIN_DWELL_MS = 6000;
/** Matches the `--dock-w` initial value in App.css. */
const DEFAULT_DOCK_WIDTH = 340;
/** Below this, a conversation is too slight to have taught anything (REF-3). */
const REFLECT_MIN_MESSAGES = 8;
/** PRO-4: last calendar date the daily rebuild tick ran, so it fires at most
 * once per day the app is actually open — simpler than routing a per-user
 * fact-rebuild through `SCH`'s job scheduler, which is for named, editable
 * jobs, not internal maintenance ticks like this one. */
const PROFILE_CHECKED_KEY = "profile.checked_on";
/** PRO-4: how long to wait after a global fact changes before rebuilding —
 * long enough that a burst of edits (e.g. `Tidy up`) coalesces into one call. */
const PROFILE_DEBOUNCE_MS = 8000;
let profileDebounceTimer: ReturnType<typeof setTimeout> | null = null;

/** The self-change classes the Autonomy card offers (AUT-1). `fallback` mirrors
 * the backend's `AUTONOMY_DEFAULTS`; `rungs` hides options a class can't honour
 * — facts have no proposal UI, so they are auto-with-undo or off.
 *
 * `profile` deliberately has no entry here (it still defaults to `auto` in the
 * backend): SMP-5b says the synthesis never appears as "a settings entry" —
 * adding a toggle here would be exactly that. */
export const AUTONOMY_CLASSES: {
  id: string;
  label: string;
  blurb: string;
  fallback: string;
  rungs: string[];
}[] = [
  {
    id: "facts",
    label: "Remembering facts about you",
    blurb: "What I save when you tell me something durable. Every save is undoable.",
    fallback: "auto",
    rungs: ["auto", "off"],
  },
  {
    id: "lessons",
    label: "Learning from my mistakes",
    blurb: "Lessons I draw from finished conversations. Also undoable.",
    fallback: "auto",
    rungs: ["auto", "ask", "off"],
  },
  {
    id: "soul",
    label: "Changing my standing instructions",
    blurb: "How I should always behave. I always ask first.",
    fallback: "ask",
    rungs: ["ask", "off"],
  },
  {
    id: "consolidate",
    label: "Tidying up my memory",
    blurb: "Merging and pruning what I remember. You review the whole tidy-up.",
    fallback: "ask",
    rungs: ["ask", "off"],
  },
  {
    id: "email_send",
    label: "Sending mail on your behalf",
    blurb: "Mail leaving this machine. I can't unsend it, so I always ask first unless you turn this on.",
    fallback: "ask",
    rungs: ["auto", "ask", "off"],
  },
  {
    id: "skills",
    label: "Keeping Agent Skills",
    blurb: "New procedures I write for myself. I always ask first.",
    fallback: "ask",
    rungs: ["ask", "off"],
  },
  {
    id: "screen",
    label: "Taking screenshots",
    blurb: "A picture of your screen can contain anything, so I always ask first unless you turn this on.",
    fallback: "ask",
    rungs: ["auto", "ask", "off"],
  },
];
const DEFAULT_SYSTEM_PROMPT =
  "You are Poiesis Agent, a local-first assistant that maintains itself: you keep durable memory, learn lessons from your own mistakes, and propose — never impose — changes to how you work. Be concise and clear.";

interface AppState {
  bootstrapped: boolean;
  /** True once the model lists (library, cloud, media) have actually come
   * back at least once. `bootstrapped` flips before they do, so anything
   * that reasons about "the user has no models and no keys" has to wait for
   * this instead — otherwise it judges an empty list that simply hasn't
   * loaded yet. */
  modelsLoaded: boolean;

  // theme
  mode: Mode;
  setMode: (m: Mode) => void;

  // navigation
  view: View;
  setView: (v: View) => void;
  railCollapsed: boolean;
  toggleRail: () => void;

  // model picker + library
  models: Model[];
  libraryModels: api.ModelEntry[];
  cloudModels: api.CloudModel[];
  /** Images & video group (`PIK-1`) — every model any credentialed backend
   * offers, local and hosted together. */
  mediaModels: api.MediaModel[];
  providers: api.ProviderInfo[];
  /** A user's own connected model servers (Ollama, LM Studio, ...) and the
   * models they currently offer, for the picker's "Your own servers" group. */
  endpoints: api.EndpointInfo[];
  endpointModels: api.EndpointModel[];
  selectedModelId: string;
  /** The chat model to fall back to when "← Back to chat" is pressed, or a
   * media selection is cleared (`PIK-2`). Set whenever a chat model is chosen;
   * never a media one, so it always names something `run_agent` can use. */
  lastChatModelId: string;
  modelFilter: ModelFilter;
  engineReady: boolean;
  /** Which library model the running engine currently holds (null = none). */
  loadedModelId: string | null;
  loadingModel: { id: string; label: string } | null;
  selectModel: (id: string) => void;
  setModelFilter: (f: ModelFilter) => void;
  refreshLibrary: () => Promise<void>;
  refreshCloud: () => Promise<void>;
  refreshMediaModels: () => Promise<void>;
  refreshEndpoints: () => Promise<void>;
  loadModelById: (id: string) => Promise<void>;
  stopEngine: () => Promise<void>;

  /** Catalog downloads in flight, keyed by catalog entry id — percent
   * complete, or "done" briefly while the library refreshes. Lives in the
   * store rather than the Models view's local state so leaving and returning
   * to that view shows the real state instead of a bare "Download" button
   * that invites a duplicate click (which used to add the same model to the
   * library a second time). */
  modelDownloads: Record<string, number | "done">;
  downloadCatalogModel: (entry: api.CatalogEntry) => Promise<void>;

  // conversations
  conversations: Conversation[];
  activeConversationId: string | null;
  busy: boolean;
  systemPrompt: string;
  /** Whether built-in toolsets are offered to the model (TOOL-3, TOOL-6). */
  toolsEnabled: boolean;
  setToolsEnabled: (on: boolean) => void;
  /** Workspace mode: the chat view flips to the composed-interface layout —
   * the agent's UI is the interaction point, the message stream is a log. */
  workspaceMode: boolean;
  setWorkspaceMode: (on: boolean) => void;
  /** Generate an image from `prompt` and show it inline in the chat — the
   * inferred route (`PIK-3`) and the composer's legacy direct path both land
   * here; it always resolves to whichever backend is available (9F). */
  createImage: (prompt: string, modelPath?: string | null) => Promise<void>;
  /** The declared route (`PIK-2`/Path E): generate against an exact model the
   * user picked in the chooser, with the target bar's aspect ratio and any
   * reference (an explicit attachment, or the implicit one from `EDT-2`). */
  createMedia: (args: {
    prompt: string;
    modelId: string;
    aspectRatio?: string;
    /** `PIK-4`'s advanced knobs. Every one is optional, and a backend that
     * can't honour one reports it rather than failing. */
    resolution?: string;
    seed?: number;
    steps?: number;
    negative?: string;
    durationSecs?: number;
    references?: string[];
    parentArtifactId?: string;
  }) => Promise<void>;
  /** `CST-1`: which backends the user has already said yes to paying for, this
   * install. Persisted client-side — it is a UI trust decision, not a fact the
   * agent's memory or the DB needs to know. */
  mediaConsent: Record<string, boolean>;
  /** A cloud generation is waiting on consent. `resolve(true)` proceeds and
   * remembers the choice for this backend; `resolve(false)` cancels this one
   * call only. */
  pendingMediaConsent: { backendId: string; backendLabel: string; priceLabel?: string; resolve: (ok: boolean) => void } | null;
  /** The artifact the previous assistant turn produced, if it was media and it
   * was within the last few turns — the implicit reference `EDT-2` offers for
   * a bare "make it warmer". Cleared once too much has happened since. */
  lastMediaArtifact: { id: string; path: string; conversationId: string; turnsAgo: number } | null;
  clearImplicitReference: () => void;
  /** `PIK-4`: the seed the last generation actually came out with, which is
   * what *reuse last seed* reuses. Only a value the provider reported back
   * counts — a requested seed a backend ignored would make "reproducible"
   * a lie. */
  lastMediaSeed: number | null;
  /** The media block's **Refine** (`STR-2`) asking the composer to pin an
   * intent, show this artifact as its reference chip, and take focus. The
   * nonce is what makes a second click on the same artifact register. */
  composerPin: { intent: "image" | "video"; nonce: number } | null;
  refineArtifact: (artifact: api.Artifact) => void;
  /** Generations in flight (`JOB-1`), by job id — which turn each one belongs
   * to, so a result arriving minutes later lands in the right place. Not
   * persisted: the backing rows are, and a reload re-reads them. */
  mediaJobs: Record<string, { conversationId: string; messageId: string; stepId: string }>;
  /** `STR-4`: the latest partial image per running job, as a data URI. Held
   * outside the message so a stream of partials doesn't rewrite the
   * transcript on every frame. */
  mediaPartials: Record<string, string>;
  /** Apply a job's completion (or failure, or cancellation) to its turn. */
  applyMediaJobEvent: (event: api.MediaJobEvent) => void;
  /** Stop a running generation. */
  cancelMediaJob: (jobId: string) => Promise<void>;

  // personas (CHT-4 / CHT-7)
  personas: api.Persona[];
  refreshPersonas: () => Promise<void>;
  createPersona: (args: {
    name: string;
    systemPrompt: string;
    modelId?: string | null;
    temperature?: number;
    toolsJson?: string | null;
    skillsJson?: string | null;
  }) => Promise<void>;
  updatePersona: (persona: api.Persona) => Promise<void>;
  deletePersona: (id: string) => Promise<void>;
  setDefaultPersona: (id: string) => Promise<void>;
  /** Apply (or clear) a persona on the active conversation. */
  applyPersona: (conversationId: string, personaId: string | null) => Promise<void>;
  /** Set a one-off per-conversation temperature override (CHT-7). */
  setConversationTemperature: (conversationId: string, temperature: number | null) => Promise<void>;
  /** What's shaping the current answer (WHY-1/4) — `undefined` when the panel
   * is closed. `messageId` unset means the live/composer view; set means the
   * "why this answer?" view for one past message. */
  contextPanelTarget: { conversationId: string; messageId?: string } | undefined;
  openContextPanel: (target: { conversationId: string; messageId?: string }) => void;
  closeContextPanel: () => void;

  // accessibility + privacy (§5.5, §6.3)
  readingScale: number;
  setReadingScale: (scale: number) => Promise<void>;
  telemetryEnabled: boolean;
  setTelemetryEnabled: (on: boolean) => Promise<void>;
  /** "Show me everything" (SMP-1a) — reveals engine internals, per-note and
   * per-persona controls, indexed-folder management, and raw prompt layers.
   * Off by default: Simple mode should read as a complete product. */
  expert: boolean;
  setExpert: (on: boolean) => Promise<void>;

  // durable memory (MEM)
  /** The always-injected index + standing instructions (MEM-3). */
  memoryContext: api.MemoryContext;
  refreshMemoryContext: () => Promise<void>;
  /** Self-changes the agent proposed and the user hasn't answered (SOUL-3). */
  changeProposals: api.ChangeProposal[];
  refreshChangeProposals: () => Promise<void>;
  resolveChangeProposal: (id: string, accept: boolean) => Promise<void>;
  /** `MAIL-UI-2`'s `Edit`: rewrite a pending proposal's text before accepting. */
  updateChangeProposalText: (id: string, text: string) => Promise<void>;
  /** A tidy-up the user hasn't answered — feeds the Settings badge (SOUL-UI-3). */
  consolidationPending: boolean;
  /** The most recent memory write, for the undoable toast (MEM-UI-3). `op` and
   *  `undoToken` decide what Undo means: undo a save by forgetting it, a forget
   *  by restoring it from trash. */
  memoryToast: {
    op: string;
    name: string;
    description: string;
    collection: string;
    undoToken: string;
  } | null;
  dismissMemoryToast: () => void;
  undoMemoryWrite: () => Promise<void>;
  /** True until the first-write explainer has been shown once (MEM-UI-4). */
  memoryOnboarded: boolean;
  /** SMP-7: one ability explaining itself once, the first time it actually
   *  happens — `recall`, `retrieval`, `digest`, `proposal`. At most one such
   *  explanation per session (`SMP-7c`); a second candidate simply waits for
   *  next time rather than queuing behind the first. */
  explainToast: string | null;
  firstTimeFlags: Record<string, boolean>;
  /** Whether `firstTimeFlags` has come back from disk. Nothing explains itself
   * before it has: an empty map is indistinguishable from "never explained". */
  firstTimeFlagsLoaded: boolean;
  firstTimeShownThisSession: boolean;
  maybeFirstTime: (key: string, message: string) => void;
  /** `SMP-7d`: forget every first-time flag, from Everything mode. */
  resetFirstTimeExplanations: () => Promise<void>;
  /** Whether the Memory toolset is on — gates both the tool and the injection. */
  memoryToolEnabled: boolean;
  refreshMemoryToolset: () => Promise<void>;
  /** PRO-4: call after any change to a global-scoped fact. Debounces 8s, then
   * attempts an automatic rebuild — a no-op below the volume gate or with the
   * `profile` autonomy rung off. */
  noteGlobalFactChange: () => void;
  /** The automatic rebuild trigger itself (debounce and daily tick both land
   * here). Silent on every "decided not to" outcome; only a genuine new
   * synthesis raises the toast (PRO-UI-5). */
  maybeAutoRebuildProfile: () => Promise<void>;

  /** SMP-2: the first-need prompt to install the recall helper. `null` when
   * nothing is being offered; "asking" while the two-button prompt shows,
   * "installing" while the download runs, "installed" for the one-time
   * confirmation once it finishes. */
  recallOffer: { stage: "asking" | "installing" | "installed"; progress?: api.DownloadProgress } | null;
  /** "Not now" is permanent (SMP-2b), not per-session — read once at bootstrap. */
  recallDeclined: boolean;
  /** Show the first-need prompt if the recall helper isn't installed and
   * wasn't already declined. Safe to call on every folder attach and memory
   * write (SMP-2b) — a no-op once it's already showing, installed, or
   * declined. */
  maybeOfferRecall: () => Promise<void>;
  acceptRecallOffer: () => Promise<void>;
  declineRecallOffer: () => Promise<void>;

  // the autopoietic layer (Phase 11)
  /** What the organism is doing right now, for the living mark (PRES-1). */
  presence: "idle" | "active" | "reflecting" | "healing";
  /** Conversations currently being reflected on — the rail shows them digesting
   * (PRES-2). In-memory only. */
  reflectingIds: string[];
  /** Conversations this session's reflection actually learned from (PRES-2). */
  digestedIds: string[];
  /** Run reflection over one conversation and surface what it learned (REF-3).
   * `learned` was written; `proposed` is waiting on the user. */
  reflectConversation: (
    conversationId: string
  ) => Promise<{ learned: number; proposed: number }>;
  /** Counts + health for the Self view (ORG-1). */
  vitality: api.Vitality | null;
  lessons: api.Fact[];
  refreshSelf: () => Promise<void>;
  forgetLesson: (name: string) => Promise<void>;
  /** 7-day per-tool reliability for the running model; feeds the caution lines
   * the agent gets in its own prompt (HEAL-2). */
  toolHealth: api.ToolHealth[];
  refreshToolHealth: () => Promise<void>;
  /** A one-line notice from the watchdog (HEAL-1), or null. */
  healToast: string | null;
  dismissHealToast: () => void;
  /** `TTL-2`: a one-line notice that short-lived facts were let go. */
  expirySweptToast: string | null;
  dismissExpirySweptToast: () => void;
  /** `GLD-2`: a one-line confession that a self-change was checked and put back. */
  goldenRevertedToast: string | null;
  dismissGoldenRevertedToast: () => void;
  /** `MAIL-3`: a receipt that a message actually left the machine at the
   * `auto` rung — there's no undo, so this is announcement only. */
  mailSentToast: string | null;
  dismissMailSentToast: () => void;
  /** The Health tab's Golden section (`GLD-UI-1`). */
  goldenStatus: api.GoldenStatus | null;
  /** Why the last check couldn't run (usually: no model loaded), or "". */
  goldenError: string;
  checkingGolden: boolean;
  checkGoldenNow: () => Promise<void>;
  /** Reflect automatically on leaving a conversation (setting `reflection.auto`). */
  autoReflect: boolean;
  setAutoReflect: (on: boolean) => Promise<void>;
  /** How much Poiesis may change without asking, per class (AUT-1). */
  autonomy: Record<string, string>;
  setAutonomy: (cls: string, rung: string) => Promise<void>;
  /** When this Poiesis first ran, for the growth narrative (PRES-3). */
  selfBorn: number | null;
  /** True once the first-run introduction has been answered (PRES-6). */
  selfIntroduced: boolean;
  dismissIntroduction: () => Promise<void>;
  /** Start a new workspace conversation from a saved procedure (RCP-UI-2). */
  startFromSkill: (skill: api.SkillView) => Promise<void>;

  // Agent Skills (SKL): discovered skills, for the system-prompt disclosure,
  // the Composer's `/` drop-up, and the Skills settings tab.
  skills: api.SkillView[];
  refreshSkills: () => Promise<void>;
  setSkillEnabled: (source: string, name: string, enabled: boolean) => Promise<void>;
  forgetSkill: (name: string) => Promise<void>;

  // scheduled jobs (SCH): the quiet night shift
  scheduledJobs: api.ScheduledJob[];
  /** The job currently in the one run slot (SCH-1), if any. */
  runningJob: api.RunningJob | null;
  /** The most recent nightly first-person summary (SCH-UI-1), if one exists. */
  digest: api.Digest | null;
  refreshScheduler: () => Promise<void>;
  createScheduledJob: (input: api.ScheduledJobInput) => Promise<void>;
  updateScheduledJob: (id: string, input: api.ScheduledJobInput) => Promise<void>;
  deleteScheduledJob: (id: string) => Promise<void>;
  /** SCH-UI-3's "Run now". Resolves with the job's short result summary. */
  runScheduledJobNow: (id: string) => Promise<string>;
  /** SCH-UI-4's Stop, for the job currently occupying the run slot. */
  stopScheduledJob: () => Promise<void>;
  /** Mark the digest read (SCH-UI-2) — clears the mark's slow pulse. */
  dismissDigest: () => Promise<void>;
  /** A task being made out of an open chat ("Schedule this" in the Workbench).
   * Handed to the Tasks section, which opens its editor prefilled. Held in the
   * store rather than passed as a route param because the two surfaces are in
   * different columns of the app shell with no router between them. */
  taskDraft: { name: string; prompt: string; conversationId: string } | null;
  scheduleConversation: (conversationId: string) => void;
  clearTaskDraft: () => void;

  // context homeostasis (CTX)
  /** Context window of the current model, for the composer meter. */
  contextBudget: number;
  /** Summarize older turns instead of hard-dropping them (setting `context.autocompact`). */
  autoCompact: boolean;
  setAutoCompact: (on: boolean) => Promise<void>;
  refreshContextBudget: () => Promise<void>;

  // library / all artifacts
  allArtifacts: api.Artifact[];
  refreshAllArtifacts: () => Promise<void>;
  viewArtifact: (artifact: api.Artifact) => Promise<void>;
  /** A user-attached image doesn't always have an artifact behind it — a
   * pasted screenshot never does — so `STR-3`'s thumbnail click opens a plain
   * full-size lightbox instead of routing through the Workbench viewer. */
  imageLightbox: { path?: string; dataUri?: string; alt?: string } | null;
  viewArtifactByPath: (path: string, dataUri?: string, alt?: string) => void;
  closeImageLightbox: () => void;

  bootstrap: () => Promise<void>;
  setActiveConversation: (id: string) => Promise<void>;
  newConversation: () => Promise<void>;
  renameConversation: (id: string, title: string) => Promise<void>;
  deleteConversation: (id: string) => Promise<void>;
  sendMessage: (text: string, attachments?: Attachment[]) => Promise<void>;
  /** Send a structured block interaction as a new turn (Generative UI, Phase B). */
  sendBlockAction: (
    blockId: string,
    humanText: string,
    payload: Record<string, unknown>
  ) => Promise<void>;
  /** Persist a block's local interaction state (filters, sort, unsent edits). */
  setBlockState: (blockId: string, state: unknown) => void;

  /** The live agent-composed interface per conversation (render_ui). One
   * surface per conversation; `data` is the UINode tree, `state` the user's
   * bound values. */
  surfaces: Record<string, BlockView | undefined>;
  /** `BRW-UI-1`: the live browsing session per conversation, if one is open.
   * Absent (not just empty) means no session — the panel only shows while
   * one is live. */
  browserSessions: Record<string, api.BrowserPanelState | undefined>;
  /** "Stop browsing" — drops the session and marks the panel closed, so it
   * says "I closed the page." instead of vanishing mid-sentence. */
  stopBrowsing: (conversationId: string) => Promise<void>;
  /** Clear a closed panel away once the user has read it. */
  dismissBrowserPanel: (conversationId: string) => void;
  /** Re-read the live session from the backend — on reload, and after a
   * conversation switch, the store knows nothing but Chrome may still be up. */
  refreshBrowserSession: (conversationId: string) => Promise<void>;
  /** Persist the surface's bound state (inputs, choices, toggles) — local-only,
   * no model turn; the state rides along with the next action or message. */
  setSurfaceState: (state: Record<string, unknown>) => void;
  /** A surface `action` node was activated: send a turn carrying the action,
   * its payload, and all bound state. */
  sendSurfaceAction: (humanText: string, payload: Record<string, unknown>) => Promise<void>;
  stopGenerating: () => void;
  setSystemPrompt: (prompt: string) => Promise<void>;

  /** Durable per-conversation session state (Generative UI, Phase C). */
  sessionState: Record<string, Record<string, unknown>>;
  clearSessionStateKey: (key: string) => void;

  /** Workspace mode (W2): block interactions queued locally instead of each
   * spending a model turn; drained into the next real user message. */
  pendingActions: Record<string, string[]>;

  // permission consent (§5.4.4)
  pendingPermissions: api.PermissionRequest[];
  resolvePermission: (id: string, decision: api.Decision) => Promise<void>;

  // ---- Workbench (right dock): the working folder and this chat's artifacts ----
  //
  // Files and artifacts are two origins of one idea — stuff the agent made or
  // touched — so they share one panel, one tree and one viewer. Artifacts live
  // in the DB until the user saves one into the folder, at which point it
  // promotes to a real file and leaves "Made in this chat".

  artifacts: Record<string, api.Artifact[]>;
  /** Is the dock showing? Persisted across restarts. */
  dockOpen: boolean;
  toggleDock: () => void;
  setDockOpen: (open: boolean) => void;
  /** What the viewer is showing, file or artifact. */
  selected: WorkbenchSelection | null;
  selectNode: (selection: WorkbenchSelection | null) => void;
  /** Open the dock on a specific artifact — from a timeline chip or a document
   * block. The artifact's own row in the tree scrolls into view. */
  openArtifact: (artifactId: string) => void;
  /** The viewer blown up to a full-screen overlay, for content 340px can't hold. */
  viewerExpanded: boolean;
  setViewerExpanded: (expanded: boolean) => void;
  /** Dock width in px, set by dragging its edge. Persisted across restarts. */
  dockWidth: number;
  setDockWidth: (px: number) => void;
  /** True while the divider is being dragged, so the shell drops its easing. */
  dockDragging: boolean;
  setDockDragging: (dragging: boolean) => void;
  showHidden: boolean;
  toggleShowHidden: () => void;

  /** Lazily-loaded directory children, keyed by absolute path. */
  folderTree: Record<string, api.FileNode[]>;
  expandedDirs: string[];
  toggleDir: (path: string) => Promise<void>;
  /** Paths the agent changed this session → when, for the tree's `●` marker. */
  touchedFiles: Record<string, number>;
  /** Reversible operations for the "Recent changes" strip. */
  trash: api.TrashEntry[];
  /** Why the last folder attach was refused, shown inline. */
  folderError: string | null;

  /** IDX-UI-1: the attached folder's index status. `null` = no folder
   * attached, or it's never been read ("I haven't read this folder yet"). */
  indexState: api.IndexRootView | null;
  /** Live counting line while a build runs (IDX-7); cleared when it ends. */
  indexProgress: api.IndexProgress | null;
  /** Set on a failed build; cleared on the next attempt. */
  indexError: string | null;
  /** SMP-4c: whether folder reading has already explained itself once. Until
   * it has, the reading line carries a one-sentence "what this is". */
  indexExplained: boolean;
  refreshIndexStatus: () => Promise<void>;
  /** SMP-4a: start reading a freshly attached folder, unless it was read
   * before or the user stopped it (SMP-4d). Silent when it decides not to. */
  maybeAutoIndex: () => Promise<void>;
  buildFolderIndex: () => Promise<void>;
  cancelFolderIndex: () => Promise<void>;
  forgetFolderIndex: (path: string) => Promise<void>;

  /** `PHS-UI-1`: the last "Find duplicates" scan's groups, the folder it
   * scanned, and any error — `null` groups means no scan has run yet. */
  duplicateGroups: api.DuplicateGroup[] | null;
  duplicateScanPath: string | null;
  duplicatesLoading: boolean;
  duplicatesError: string | null;
  findDuplicatesIn: (path: string) => Promise<void>;
  /** Trash every file in the group except `keep` — the existing trash path,
   * so `RecentChanges.tsx` can undo it. */
  keepDuplicate: (group: api.DuplicateGroup, keep: string) => Promise<void>;
  dismissDuplicates: () => void;

  attachFolder: () => Promise<void>;
  detachFolder: () => Promise<void>;
  setFolderTrust: (trust: FolderTrust) => Promise<void>;
  /** Reload one directory's children, or the whole tree when omitted. */
  refreshTree: (path?: string) => Promise<void>;
  refreshTrash: () => Promise<void>;
  undoFileOp: (id: string) => Promise<void>;
  saveArtifactToFolder: (artifactId: string, dest: string) => Promise<void>;
  /** Hand a file to the OS — open it, or show it in the file manager. */
  openInSystem: (path: string) => Promise<void>;
  revealInSystem: (path: string) => Promise<void>;
}

function applyMode(mode: Mode) {
  document.documentElement.setAttribute("data-mode", mode);
}

/** Reading-size choices for the serif reading column (§5.5). */
export const READING_SCALES = [
  { label: "Smaller", value: 0.85 },
  { label: "Small", value: 0.92 },
  { label: "Standard", value: 1 },
  { label: "Large", value: 1.15 },
  { label: "Larger", value: 1.3 },
];

function applyReadingScale(scale: number) {
  document.documentElement.style.setProperty("--reading-scale", String(scale));
}

const PROVIDER_LABELS: Record<string, string> = {
  openai: "OpenAI",
  openrouter: "OpenRouter",
  anthropic: "Anthropic",
};

function localToModels(lib: api.ModelEntry[]): Model[] {
  return lib.map((e) => ({
    id: e.id,
    name: e.name,
    provenance: "local",
    meta: e.quant ?? undefined,
    vision: e.vision,
    available: true,
  }));
}

function cloudToModels(cm: api.CloudModel[]): Model[] {
  return cm.map((m) => ({
    id: `cloud:${m.id}`,
    name: m.name,
    provenance: "cloud",
    meta: PROVIDER_LABELS[m.provider] ?? m.provider,
    vision: m.vision,
    tools: m.tools,
    available: true,
    provider: m.provider,
    cloudModel: m.model,
  }));
}

function endpointToModels(ems: api.EndpointModel[]): Model[] {
  return ems.map((m) => ({
    id: m.id,
    name: m.name,
    // Runs on a server the user already has going on their own machine (or
    // one they've pointed at), so this counts as "on this device" for
    // filtering purposes even though it's routed like a cloud model.
    provenance: "endpoint",
    meta: m.endpoint_label,
    vision: m.vision,
    tools: m.tools,
    ctxSize: m.ctx_size,
    available: true,
    provider: m.endpoint_id,
    endpointId: m.endpoint_id,
    endpointLabel: m.endpoint_label,
    cloudModel: m.model,
  }));
}

function mediaToModels(media: api.MediaModel[]): Model[] {
  return media.map((m) => ({
    id: m.id,
    name: m.name,
    // A hosted media backend leaves the machine exactly like a hosted chat
    // model does; the local one doesn't. Same dot, same meaning, no special case.
    provenance: m.backend_id === "local" ? "local" : "cloud",
    meta: `${m.modality} · ${m.backend_label}`,
    available: true,
    modality: m.modality,
    backendId: m.backend_id,
    backendLabel: m.backend_label,
    priceLabel: m.price_label ?? undefined,
    supportsEdit: m.supports_edit,
    supportedAspectRatios: m.supported_aspect_ratios,
    supportedResolutions: m.supported_resolutions,
    maxDurationSecs: m.max_duration_secs ?? undefined,
  }));
}

function composeModels(
  lib: api.ModelEntry[],
  cloud: api.CloudModel[],
  media: api.MediaModel[] = [],
  endpoints: api.EndpointModel[] = []
): Model[] {
  return [...localToModels(lib), ...endpointToModels(endpoints), ...cloudToModels(cloud), ...mediaToModels(media)];
}

/** Keep the selection pointing at something that still exists.
 *
 * Models disappear underneath us — an image checkpoint deleted on the Models
 * screen, a provider key removed, an engine uninstalled. A `selectedModelId`
 * left naming one of those resolves to an arbitrary fallback in the picker
 * while the id actually sent to the backend names nothing, so every refresher
 * that recomposes `models` has to re-settle the selection through here. */
function reconcileSelection(
  models: Model[],
  libraryModels: api.ModelEntry[],
  selectedModelId: string,
  lastChatModelId: string
): Partial<AppState> {
  // Prefer the library default, then any chat model — never silently land on
  // a media model, which would change what pressing send does.
  const fallback =
    libraryModels.find((e) => e.is_default)?.id ??
    models.find((m) => !m.modality || m.modality === "chat")?.id ??
    models[0]?.id;
  const patch: Partial<AppState> = {};
  if (!models.some((m) => m.id === selectedModelId) && fallback) {
    patch.selectedModelId = fallback;
  }
  if (!models.some((m) => m.id === lastChatModelId) && fallback) {
    patch.lastChatModelId = fallback;
  }
  return patch;
}

/**
 * In-flight engine load, shared so concurrent/repeat requests for the same
 * model reuse one load instead of spawning multiple `llama-server` processes
 * (e.g. React StrictMode double-invokes, or selecting then immediately sending).
 */
let inflightLoad: { id: string; promise: Promise<void> } | null = null;

function toMessage(m: api.DbMessage): Message {
  return {
    id: m.id,
    role: m.role,
    text: m.content,
    model: m.model_name
      ? { name: m.model_name, provenance: (m.model_provenance ?? "local") as Provenance }
      : undefined,
    steps: api.parseSteps(m.steps_json),
    attachments: m.attachments?.length
      ? m.attachments.map((a) => ({
          id: a.id,
          kind: (a.kind === "pdf" ? "pdf" : a.kind === "video" ? "video" : "image") as Attachment["kind"],
          name: a.name,
          path: a.path,
          // Dimensions aren't stored here — `ChatMedia` reads them off the
          // artifact's metadata, which is the one place they're recorded.
          artifactId: a.artifact_id ?? undefined,
        }))
      : undefined,
    createdAt: m.created_at,
  };
}

function toBlockView(b: api.DbBlock): BlockView {
  return {
    id: b.id,
    kind: b.kind as BlockView["kind"],
    title: b.title,
    data: safeParse(b.data_json),
    state: b.state_json ? safeParse(b.state_json) : undefined,
    messageId: b.message_id,
  };
}

function safeParse(json: string): unknown {
  try {
    return JSON.parse(json);
  } catch {
    return {};
  }
}

function toConversation(c: api.DbConversation): Conversation {
  return {
    id: c.id,
    title: c.title,
    updatedAt: c.updated_at,
    messages: [],
    personaId: c.persona_id,
    overrides: parseOverrides(c.overrides_json),
    workspace: c.workspace,
    summary: c.summary,
    summaryUptoMessageId: c.summary_upto_message_id,
    reflectedAt: c.reflected_at,
    folderPath: c.folder_path,
    folderTrust: (c.folder_trust as FolderTrust) ?? "confirm",
  };
}

/** Read the optional `{temperature}` param a persona carries in its params_json. */
export function personaTemperature(p: api.Persona | undefined): number | undefined {
  if (!p?.params_json) return undefined;
  try {
    const v = JSON.parse(p.params_json) as { temperature?: number };
    return typeof v.temperature === "number" ? v.temperature : undefined;
  } catch {
    return undefined;
  }
}

function parseOverrides(json: string | null): Conversation["overrides"] {
  if (!json) return undefined;
  try {
    const v = JSON.parse(json) as { temperature?: number };
    return typeof v.temperature === "number" ? { temperature: v.temperature } : undefined;
  } catch {
    return undefined;
  }
}

function serializeOverrides(overrides: Conversation["overrides"]): string | null {
  return overrides && typeof overrides.temperature === "number"
    ? JSON.stringify({ temperature: overrides.temperature })
    : null;
}

function deriveTitle(text: string): string {
  const t = text.trim().replace(/\s+/g, " ");
  return t.length > 48 ? `${t.slice(0, 48)}…` : t || "New chat";
}

/** Mirrors the backend's `media::ellipsize` (`FIX-1`) so a composer-made
 * image's step target matches what the same prompt would show via the tool
 * path. Uses code points (`Array.from`), not `.slice`, for the same reason
 * the Rust side iterates `char_indices()` — a naive UTF-16 cut can land
 * inside a surrogate pair for a multi-codepoint emoji. */
function ellipsizeClient(s: string, maxChars: number): string {
  const chars = Array.from(s);
  return chars.length <= maxChars ? s : `${chars.slice(0, maxChars).join("")}…`;
}

const MEDIA_CONSENT_KEY = "poiesis.media.consent";

/** Which cloud media backends the user has already agreed to pay for, this
 * install (`CST-1`). A UI trust decision, not a fact worth the DB or the
 * agent's memory — so it lives in `localStorage`, not `settings`. */
function loadMediaConsent(): Record<string, boolean> {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(MEDIA_CONSENT_KEY);
    return raw ? (JSON.parse(raw) as Record<string, boolean>) : {};
  } catch {
    return {};
  }
}
function saveMediaConsent(consent: Record<string, boolean>) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(MEDIA_CONSENT_KEY, JSON.stringify(consent));
  } catch {
    /* ignore */
  }
}

/** An artifact's `meta_json`, parsed defensively. It is written by us, but a
 * throw here would take the whole transcript down with it — a caption is never
 * worth that, so a malformed row degrades to no metadata instead. */
export function parseArtifactMeta(metaJson?: string | null): Record<string, unknown> {
  if (!metaJson) return {};
  try {
    const parsed: unknown = JSON.parse(metaJson);
    return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

/** The inline attachment that renders a media artifact in the message stream.
 * Both creation paths build it from here, which is what makes them
 * presentation-identical (`STR-1`) rather than merely similar. */
export function mediaAttachmentFor(artifact: api.Artifact): Attachment {
  const meta = parseArtifactMeta(artifact.meta_json);
  const ext = artifact.content.split(".").pop()?.toLowerCase();
  return {
    id: `media-${artifact.id}`,
    kind: artifact.kind === "video" ? "video" : "image",
    name: `${artifact.kind}.${ext && ext.length <= 4 ? ext : "png"}`,
    path: artifact.content,
    artifactId: artifact.id,
    width: typeof meta.width === "number" ? meta.width : undefined,
    height: typeof meta.height === "number" ? meta.height : undefined,
    durationSecs: typeof meta.duration_secs === "number" ? meta.duration_secs : undefined,
  };
}

/** The model header a media artifact should carry: the provider that actually
 * made it, and whether that left the machine. */
export function mediaModelFor(artifact: api.Artifact): { name: string; provenance: Provenance } {
  const meta = parseArtifactMeta(artifact.meta_json);
  const modelId = typeof meta.model_id === "string" ? meta.model_id : "";
  return {
    name: typeof meta.provider_label === "string" ? meta.provider_label : "Image",
    provenance: modelId.startsWith("local:") ? "local" : "cloud",
  };
}

// Context-window constants (CTX-4). Declared above the store because the store's
// initial `contextBudget` reads DEFAULT_LOCAL_CTX at creation time — a `const`
// referenced before its declaration is a temporal-dead-zone crash at import.
/** Published context windows for hosted models, by provider. */
const CLOUD_CTX: Record<string, number> = {
  anthropic: 200_000,
  openai: 128_000,
  openrouter: 32_000,
};
/** Used before the engine reports its real window, and in browser preview. */
const DEFAULT_LOCAL_CTX = 4096;

export const useAppStore = create<AppState>((set, get) => ({
  bootstrapped: false,
  modelsLoaded: false,

  mode: "light",
  setMode: (mode) => {
    applyMode(mode);
    set({ mode });
  },

  view: "chat",
  setView: (view) => set({ view }),
  railCollapsed: false,
  toggleRail: () => set((s) => ({ railCollapsed: !s.railCollapsed })),

  models: mockModels,
  libraryModels: [],
  cloudModels: [],
  providers: [],
  selectedModelId: mockModels[0].id,
  modelFilter: "all",
  engineReady: false,
  loadedModelId: null,
  loadingModel: null,
  setModelFilter: (modelFilter) => set({ modelFilter }),

  lastChatModelId: mockModels[0].id,

  // Selecting a local model loads it into the engine; cloud models (later)
  // just become the active choice without spawning anything.
  selectModel: (id) => {
    const m = get().models.find((x) => x.id === id);
    set({ selectedModelId: id, ...(!m?.modality || m.modality === "chat" ? { lastChatModelId: id } : {}) });
    // The context window is a property of the model, so the meter follows it.
    get().refreshContextBudget();
    get().refreshMemoryContext();
    get().refreshChangeProposals();
    get().refreshMemoryToolset();
    // Tool reliability is per model too (HEAL-2): a tool a 3B fumbles isn't
    // broken for a cloud model, so the cautions must not carry over.
    get().refreshToolHealth();
    // A media id has no engine to load (`PIK-2`) — `libraryModels` would
    // simply miss it, but the guard is explicit so a lookup miss can never
    // silently fall through into spawning `llama-server` on the wrong id.
    if (m?.modality && m.modality !== "chat") return;
    if (api.inTauri() && m?.provenance === "local") {
      const s = get();
      // Already running (or starting) this exact model — nothing to do.
      if (s.loadedModelId === id || s.loadingModel?.id === id) return;
      get().loadModelById(id).catch(() => {
        /* surfaced via engine status; chat send will retry + report */
      });
    }
  },

  refreshLibrary: async () => {
    if (!api.inTauri()) return;
    const lib = await api.listModels();
    set((s) => {
      const models = composeModels(lib, s.cloudModels, s.mediaModels, s.endpointModels);
      return {
        libraryModels: lib,
        models,
        ...reconcileSelection(models, lib, s.selectedModelId, s.lastChatModelId),
      };
    });
  },

  modelDownloads: {},
  downloadCatalogModel: async (entry) => {
    // Already downloading this one — the Rust side also guards against a
    // concurrent duplicate, but bailing out here means a repeat click (after
    // leaving and returning to the Models view, say) doesn't even fire a
    // second request.
    if (get().modelDownloads[entry.id] !== undefined) return;
    set((s) => ({ modelDownloads: { ...s.modelDownloads, [entry.id]: 0 } }));
    try {
      await api.downloadModel(
        { url: entry.url, name: entry.name, quant: entry.quant, vision: entry.vision },
        (p) => {
          const pct = p.total ? Math.round((p.received / p.total) * 100) : 0;
          set((s) => ({ modelDownloads: { ...s.modelDownloads, [entry.id]: pct } }));
        }
      );
      set((s) => ({ modelDownloads: { ...s.modelDownloads, [entry.id]: "done" } }));
      await get().refreshLibrary();
    } finally {
      set((s) => {
        const { [entry.id]: _drop, ...rest } = s.modelDownloads;
        return { modelDownloads: rest };
      });
    }
  },

  mediaModels: [],
  // The picker's "Images & video" group (`PIK-1`). Omitted entirely by the UI
  // when this comes back empty — a fresh install with no engine and no key
  // sees today's picker unchanged, per the plan's own acceptance bar.
  refreshMediaModels: async () => {
    if (!api.inTauri()) return;
    const mediaModels = await api.listMediaModels().catch(() => []);
    set((s) => {
      const models = composeModels(s.libraryModels, s.cloudModels, mediaModels, s.endpointModels);
      return {
        mediaModels,
        models,
        ...reconcileSelection(models, s.libraryModels, s.selectedModelId, s.lastChatModelId),
      };
    });
  },

  endpoints: [],
  endpointModels: [],
  // Load a user's own connected servers + the models they currently offer
  // (best-effort per endpoint — a sleeping Ollama box shouldn't block boot).
  refreshEndpoints: async () => {
    if (!api.inTauri()) return;
    const [endpoints, endpointModels] = await Promise.all([
      api.listEndpoints().catch(() => []),
      api.listEndpointModels().catch(() => []),
    ]);
    set((s) => {
      const models = composeModels(s.libraryModels, s.cloudModels, s.mediaModels, endpointModels);
      return {
        endpoints,
        endpointModels,
        models,
        ...reconcileSelection(models, s.libraryModels, s.selectedModelId, s.lastChatModelId),
      };
    });
  },

  // Load cloud providers + their models for the unified picker (CLD-3, CLD-4).
  refreshCloud: async () => {
    if (!api.inTauri()) return;
    const [providers, cloudModels] = await Promise.all([
      api.listProviders().catch(() => []),
      api.listCloudModels().catch(() => []),
    ]);
    set((s) => {
      const models = composeModels(s.libraryModels, cloudModels, s.mediaModels, s.endpointModels);
      return {
        providers,
        cloudModels,
        models,
        ...reconcileSelection(models, s.libraryModels, s.selectedModelId, s.lastChatModelId),
      };
    });
  },

  loadModelById: async (id) => {
    // Reuse an in-flight load of the same model instead of spawning a second
    // engine process.
    if (inflightLoad && inflightLoad.id === id) return inflightLoad.promise;
    const entry = get().libraryModels.find((e) => e.id === id);
    if (!entry) return;
    const promise = (async () => {
      set({ loadingModel: { id, label: "Starting the engine…" }, engineReady: false });
      try {
        await api.loadModel({ modelPath: entry.path }, (p) => {
          const pct = p.total ? ` ${Math.round((p.received / p.total) * 100)}%` : "";
          set({ loadingModel: { id, label: `${p.label}${pct}` } });
        });
        set({ engineReady: true, selectedModelId: id, loadedModelId: id, loadingModel: null });
      } catch (e) {
        set({ loadingModel: null, engineReady: false, loadedModelId: null });
        throw e;
      } finally {
        inflightLoad = null;
      }
    })();
    inflightLoad = { id, promise };
    return promise;
  },

  stopEngine: async () => {
    if (api.inTauri()) await api.stopEngine().catch(() => {});
    set({ engineReady: false, loadedModelId: null, loadingModel: null });
  },

  conversations: [],
  activeConversationId: null,
  busy: false,
  systemPrompt: DEFAULT_SYSTEM_PROMPT,
  // Tools default ON: without them the model can't reach render_ui/present, yet
  // the conversation history may reference them — it would emit tool-call JSON
  // as prose (the raw-JSON leak) since plain-chat mode never parses fallbacks.
  toolsEnabled: true,
  setToolsEnabled: (toolsEnabled) => set({ toolsEnabled }),
  workspaceMode: false,
  setWorkspaceMode: (workspaceMode) => {
    // The flag lives on the conversation, so toggling pins (or unpins) the
    // active session and shows in the header + sidebar. Global state mirrors it
    // for the current view.
    const convId = get().activeConversationId;
    set((s) => ({
      workspaceMode,
      conversations: convId
        ? s.conversations.map((c) => (c.id === convId ? { ...c, workspace: workspaceMode } : c))
        : s.conversations,
    }));
    if (convId && api.inTauri()) api.setConversationWorkspace(convId, workspaceMode).catch(() => {});
  },
  createImage: async (prompt, modelPath) => {
    await startMediaTurn(set, get, {
      text: prompt,
      modality: "image",
      submit: ({ conversationId, messageId }) =>
        api.generateImage({
          prompt: prompt.trim(),
          conversationId,
          messageId,
          modelPath: modelPath ?? undefined,
        }),
    });
  },

  mediaConsent: loadMediaConsent(),
  pendingMediaConsent: null,
  lastMediaArtifact: null,
  lastMediaSeed: null,
  clearImplicitReference: () => set({ lastMediaArtifact: null }),

  mediaJobs: {},
  mediaPartials: {},

  applyMediaJobEvent: (event) => {
    const tracked = get().mediaJobs[event.job_id];
    // A job whose turn this session doesn't know about — submitted by the
    // agent tool, or before a reload. `message_id` is the durable answer, and
    // the attachment row the worker wrote means a reload would show it anyway.
    const convId = tracked?.conversationId ?? event.conversation_id;
    const messageId = tracked?.messageId ?? event.message_id;
    const stepId = tracked?.stepId;

    set((s) => {
      const { [event.job_id]: _done, ...rest } = s.mediaJobs;
      const { [event.job_id]: _partial, ...partials } = s.mediaPartials;
      return { mediaJobs: rest, mediaPartials: partials };
    });
    if (!convId || !messageId) return;

    const patchStep = (status: AgentStep["status"], result?: string) => {
      const existing = get()
        .conversations.find((c) => c.id === convId)
        ?.messages.find((m) => m.id === messageId);
      const step = existing?.steps?.find((st) => st.id === stepId) ?? existing?.steps?.[0];
      return step ? [{ ...step, status, result }] : undefined;
    };

    if (event.status === "done" && event.artifact) {
      const artifact = event.artifact;
      const { name: providerLabel, provenance } = mediaModelFor(artifact);
      const attachment = mediaAttachmentFor(artifact);
      patchAssistant(set, convId, messageId, {
        streaming: false,
        pendingMedia: undefined,
        model: { name: providerLabel, provenance },
        steps: patchStep("done"),
        attachments: [attachment],
        artifactIds: [artifact.id],
      });
      set((s) => ({
        artifacts: {
          ...s.artifacts,
          [convId]: [...(s.artifacts[convId] ?? []).filter((a) => a.id !== artifact.id), artifact],
        },
        lastMediaArtifact: {
          id: artifact.id,
          path: artifact.content,
          conversationId: convId,
          turnsAgo: 0,
        },
        lastMediaSeed: (() => {
          const seed = parseArtifactMeta(artifact.meta_json).seed;
          return typeof seed === "number" ? seed : s.lastMediaSeed;
        })(),
      }));
      get().refreshAllArtifacts();
      return;
    }

    if (event.status === "cancelled") {
      patchAssistant(set, convId, messageId, {
        text: "",
        streaming: false,
        pendingMedia: undefined,
        steps: patchStep("error", "— stopped"),
      });
      return;
    }

    const message = event.error ?? "the generation failed";
    patchAssistant(set, convId, messageId, {
      text: `That didn't work: ${message}`,
      streaming: false,
      pendingMedia: undefined,
      steps: patchStep("error", `— ${message}`),
    });
  },

  cancelMediaJob: async (jobId) => {
    if (!api.inTauri()) return;
    // The backend announces the cancellation on the same event every other
    // outcome arrives on, so there is nothing to patch here — one path in,
    // one path out.
    await api.cancelMediaJob(jobId).catch(() => {});
  },

  composerPin: null,
  refineArtifact: (artifact) => {
    const convId = artifact.conversation_id ?? get().activeConversationId;
    if (!convId) return;
    set((s) => ({
      lastMediaArtifact: { id: artifact.id, path: artifact.content, conversationId: convId, turnsAgo: 0 },
      composerPin: { intent: "image", nonce: (s.composerPin?.nonce ?? 0) + 1 },
    }));
  },

  createMedia: async ({ prompt, modelId, aspectRatio, resolution, seed, steps, negative, durationSecs, references, parentArtifactId }) => {
    const state = get();
    const model = state.models.find((m) => m.id === modelId);

    // `CST-1`: the first paid generation per backend asks first. Local is
    // never gated — there is nothing to consent to.
    if (model?.provenance === "cloud" && model.backendId && !state.mediaConsent[model.backendId]) {
      const backendId = model.backendId;
      const ok = await new Promise<boolean>((resolvePromise) => {
        const resolve = (accept: boolean) => {
          if (accept) {
            set((s) => {
              const mediaConsent = { ...s.mediaConsent, [backendId]: true };
              saveMediaConsent(mediaConsent);
              return { mediaConsent };
            });
          }
          set({ pendingMediaConsent: null });
          resolvePromise(accept);
        };
        set({
          pendingMediaConsent: {
            backendId,
            backendLabel: model.backendLabel ?? model.name,
            priceLabel: model.priceLabel,
            resolve,
          },
        });
      });
      if (!ok) return;
    }

    await startMediaTurn(set, get, {
      text: prompt,
      modality: model?.modality === "video" ? "video" : "image",
      aspectRatio,
      modelLabel: model?.name,
      provenance: model?.provenance,
      clearImplicitReference: true,
      submit: ({ conversationId, messageId }) =>
        api.generateMedia({
          modelId,
          modality: model?.modality === "video" ? "video" : "image",
          prompt: prompt.trim(),
          conversationId,
          messageId,
          aspectRatio,
          resolution,
          seed,
          steps,
          negative,
          durationSecs,
          references,
          parentArtifactId,
        }),
    });
  },

  personas: [],
  refreshPersonas: async () => {
    if (!api.inTauri()) return;
    try {
      set({ personas: await api.listPersonas() });
    } catch {
      /* ignore */
    }
  },
  createPersona: async ({ name, systemPrompt, modelId, temperature, toolsJson, skillsJson }) => {
    if (!api.inTauri()) return;
    const paramsJson =
      typeof temperature === "number" ? JSON.stringify({ temperature }) : null;
    await api.createPersona({
      name,
      systemPrompt,
      modelId: modelId ?? null,
      paramsJson,
      toolsJson: toolsJson ?? null,
      skillsJson: skillsJson ?? null,
    });
    await get().refreshPersonas();
  },
  updatePersona: async (persona) => {
    if (!api.inTauri()) return;
    await api.updatePersona(persona);
    await get().refreshPersonas();
  },
  deletePersona: async (id) => {
    if (!api.inTauri()) return;
    await api.deletePersona(id);
    // Detach locally from any conversation that pointed at it.
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.personaId === id ? { ...c, personaId: null } : c
      ),
    }));
    await get().refreshPersonas();
  },
  setDefaultPersona: async (id) => {
    if (!api.inTauri()) return;
    await api.setDefaultPersona(id);
    await get().refreshPersonas();
  },
  applyPersona: async (conversationId, personaId) => {
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === conversationId ? { ...c, personaId } : c
      ),
    }));
    if (api.inTauri()) {
      const overridesJson = serializeOverrides(
        get().conversations.find((c) => c.id === conversationId)?.overrides
      );
      await api.setConversationPersona(conversationId, personaId, overridesJson);
    }
    // If the persona pins a model that's in the library/cloud, select it too.
    const persona = get().personas.find((p) => p.id === personaId);
    if (persona?.model_id && get().models.some((m) => m.id === persona.model_id)) {
      get().selectModel(persona.model_id);
    }
  },
  setConversationTemperature: async (conversationId, temperature) => {
    const overrides =
      typeof temperature === "number" ? { temperature } : undefined;
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === conversationId ? { ...c, overrides } : c
      ),
    }));
    if (api.inTauri()) {
      const personaId = get().conversations.find((c) => c.id === conversationId)?.personaId ?? null;
      await api.setConversationPersona(conversationId, personaId, serializeOverrides(overrides));
    }
  },
  contextPanelTarget: undefined,
  openContextPanel: (target) => set({ contextPanelTarget: target }),
  closeContextPanel: () => set({ contextPanelTarget: undefined }),

  pendingPermissions: [],

  readingScale: 1,
  setReadingScale: async (scale) => {
    applyReadingScale(scale);
    set({ readingScale: scale });
    if (api.inTauri()) await api.setSetting(READING_SCALE_KEY, String(scale));
  },
  telemetryEnabled: false,
  setTelemetryEnabled: async (telemetryEnabled) => {
    set({ telemetryEnabled });
    if (api.inTauri()) await api.setSetting(TELEMETRY_KEY, telemetryEnabled ? "true" : "false");
  },
  expert: false,
  setExpert: async (expert) => {
    set({ expert });
    if (api.inTauri()) await api.setSetting(EXPERT_KEY, expert ? "true" : "false");
  },

  memoryContext: { index: "", soul: "", about_you: "", fact_count: 0 },
  refreshMemoryContext: async () => {
    if (!api.inTauri()) return;
    try {
      set({ memoryContext: await api.getMemoryContext() });
    } catch {
      /* memory folder unreadable — the app still works without it */
    }
  },
  changeProposals: [],
  consolidationPending: false,
  refreshChangeProposals: async () => {
    if (!api.inTauri()) return;
    try {
      const changeProposals = await api.listChangeProposals();
      set({ changeProposals });
      if (changeProposals.length > 0) {
        get().maybeFirstTime(
          "proposal",
          "This is a proposal — something I'd like to change about myself, waiting on your yes or no."
        );
      }
    } catch {
      /* non-fatal */
    }
    try {
      const c = await api.getPendingConsolidation();
      set({
        consolidationPending:
          !!c && (c.deletes.length > 0 || c.edits.length > 0 || c.merges.length > 0),
      });
    } catch {
      /* non-fatal */
    }
  },
  resolveChangeProposal: async (id, accept) => {
    // Accepting a soul proposal runs `GLD-2`'s before/after check, which needs
    // the same routing a chat turn gets.
    await api.resolveChangeProposal(id, accept, cloudTarget());
    await get().refreshChangeProposals();
    if (accept) {
      await get().refreshMemoryContext();
      await get().refreshSelf();
    }
  },
  updateChangeProposalText: async (id, text) => {
    await api.updateChangeProposalText(id, text);
    await get().refreshChangeProposals();
  },
  memoryToast: null,
  dismissMemoryToast: () => set({ memoryToast: null }),
  undoMemoryWrite: async () => {
    const toast = get().memoryToast;
    set({ memoryToast: null });
    if (!toast) return;
    try {
      if (toast.op === "profile") {
        // PRO-9: undo restores PROFILE.md from the snapshot the rebuild took
        // of itself, not a fact-trash round trip.
        await api.undoProfileRebuild();
      } else if (toast.op === "forget" && toast.undoToken) {
        // Undo the actual operation: a forget is undone by restoring from
        // trash, anything else (a save) by forgetting the entry it created.
        await api.restoreMemoryFact(toast.undoToken);
      } else {
        await api.forgetMemoryFact(toast.name);
      }
      await get().refreshMemoryContext();
    } catch {
      /* already gone */
    }
  },
  noteGlobalFactChange: () => {
    if (!api.inTauri() || !get().memoryToolEnabled) return;
    if (profileDebounceTimer) clearTimeout(profileDebounceTimer);
    profileDebounceTimer = setTimeout(() => {
      profileDebounceTimer = null;
      get().maybeAutoRebuildProfile();
    }, PROFILE_DEBOUNCE_MS);
  },
  maybeAutoRebuildProfile: async () => {
    if (!api.inTauri()) return;
    try {
      const p = await api.rebuildProfile(false);
      if (!p) return; // below the volume gate, or the rung is off — quietly nothing
      await get().refreshMemoryContext();
      set({
        memoryToast: { op: "profile", name: "", description: "", collection: "profile", undoToken: "" },
      });
    } catch {
      // A failed local call is ambient, not an error the user asked to see —
      // the next debounce or the next daily tick tries again.
    }
  },
  memoryOnboarded: false,

  explainToast: null,
  firstTimeFlags: {},
  firstTimeFlagsLoaded: false,
  firstTimeShownThisSession: false,
  maybeFirstTime: (key, message) => {
    const s = get();
    // Until the flags are back from disk, `firstTimeFlags` is an empty object
    // and every check below would pass — re-explaining something already
    // explained. Bootstrap fires several of these callers before that load
    // resolves, so staying silent is the only honest answer here: an
    // explanation is worth nothing if it can't tell "first time" from "again".
    if (!s.firstTimeFlagsLoaded) return;
    if (s.firstTimeFlags[key]) return; // already explained, ever
    if (s.firstTimeShownThisSession) return; // SMP-7c: at most one per session
    // The toast shell is one slot — don't step on a memory-write or heal
    // notice that's already using it.
    if (s.explainToast !== null || s.memoryToast !== null || s.healToast !== null) return;
    set((st) => ({
      explainToast: message,
      firstTimeShownThisSession: true,
      firstTimeFlags: { ...st.firstTimeFlags, [key]: true },
    }));
    if (api.inTauri()) api.setSetting(`onboarded.${key}`, "true").catch(() => {});
    // Self-clearing on a timer, not on unmount — so it can never get stuck
    // showing after something else replaces it in the toast shell.
    setTimeout(() => {
      if (get().explainToast === message) set({ explainToast: null });
    }, EXPLAIN_DWELL_MS);
  },
  resetFirstTimeExplanations: async () => {
    // `firstTimeFlagsLoaded` stays true: these flags are now known-empty on
    // purpose, which is the opposite of not knowing them yet.
    set({ firstTimeFlags: {}, firstTimeShownThisSession: false });
    if (!api.inTauri()) return;
    await Promise.all(FIRST_TIME_KEYS.map((k) => api.setSetting(`onboarded.${k}`, "false")));
  },

  memoryToolEnabled: true,
  refreshMemoryToolset: async () => {
    if (!api.inTauri()) return;
    try {
      const toolsets = await api.listToolsets();
      const memory = toolsets.find((s) => s.id === "memory");
      set({ memoryToolEnabled: memory?.enabled ?? true });
    } catch {
      /* keep the default */
    }
  },

  recallOffer: null,
  recallDeclined: false,
  maybeOfferRecall: async () => {
    if (!api.inTauri()) return;
    // Already showing, already answered "yes" (installing/installed this
    // session), or permanently declined — nothing to do.
    if (get().recallOffer || get().recallDeclined) return;
    try {
      const status = await api.embedEngineStatus();
      // A second trigger (folder attach + memory write landing close together)
      // can both pass the check above before either sets state — re-check
      // after the await so only the first one to resolve shows the prompt.
      if (status.model_installed || get().recallOffer || get().recallDeclined) return;
      set({ recallOffer: { stage: "asking" } });
    } catch {
      /* engine status unreachable — say nothing rather than guess */
    }
  },
  acceptRecallOffer: async () => {
    set({ recallOffer: { stage: "installing" } });
    try {
      await api.installEmbedEngine((p) => set({ recallOffer: { stage: "installing", progress: p } }));
      set({ recallOffer: { stage: "installed" } });
      setTimeout(() => {
        set((s) => (s.recallOffer?.stage === "installed" ? { recallOffer: null } : {}));
      }, 4000);
    } catch {
      // The Engine → Recall tab (SMP-2a) remains available to retry by hand.
      set({ recallOffer: null });
    }
  },
  declineRecallOffer: async () => {
    set({ recallOffer: null, recallDeclined: true });
    if (api.inTauri()) await api.setSetting(RECALL_DECLINED_KEY, "true").catch(() => {});
  },

  // ---- the autopoietic layer (Phase 11) ----

  presence: "idle",
  reflectingIds: [],
  digestedIds: [],
  reflectConversation: async (conversationId) => {
    if (!api.inTauri()) return { learned: 0, proposed: 0 };
    // Mark it before the call so the rail row starts digesting immediately.
    set((s) => ({
      reflectingIds: [...s.reflectingIds, conversationId],
      presence: "reflecting",
    }));
    // Reflection is a real turn against a real model — route it the same way a
    // chat turn is routed, so a cloud-only setup can still learn.
    const model = get().models.find((m) => m.id === get().selectedModelId);
    const target: api.ChatTarget | undefined = isRemoteModel(model) ? targetFor(model) : undefined;
    let learned = 0;
    let proposed = 0;
    try {
      const result = await api.reflectConversation(conversationId, target);
      learned = result.saved.length;
      proposed = result.proposed.length;
    } catch {
      /* a failed reflection teaches nothing and says nothing */
    }
    set((s) => {
      const stillReflecting = s.reflectingIds.filter((id) => id !== conversationId);
      return {
        reflectingIds: stillReflecting,
        // Only step down to idle if nothing else is going on: a reflection
        // finishing must not stop the mark breathing mid-generation.
        presence: stillReflecting.length
          ? s.presence
          : s.busy
            ? "active"
            : "idle",
        // Only a lesson actually written is something learned. A proposal is
        // still a question, and the rail must not claim otherwise.
        digestedIds: learned > 0 ? [...s.digestedIds, conversationId] : s.digestedIds,
        // The conversation has had its turn either way — don't re-reflect it.
        conversations: s.conversations.map((c) =>
          c.id === conversationId ? { ...c, reflectedAt: Date.now() } : c
        ),
      };
    });
    if (learned > 0 || proposed > 0) {
      get().refreshMemoryContext();
      get().refreshSelf();
      // Proposals only reach the Lessons tab (and the rail badge) once the
      // pending list is refetched.
      if (proposed > 0) get().refreshChangeProposals();
    }
    return { learned, proposed };
  },

  vitality: null,
  lessons: [],
  refreshSelf: async () => {
    if (!api.inTauri()) return;
    const model = get().models.find((m) => m.id === get().selectedModelId);
    const [vitality, lessons, goldenStatus] = await Promise.all([
      api.getVitality(isRemoteModel(model) ? model.cloudModel : undefined).catch(() => null),
      api.listLessons().catch(() => [] as api.Fact[]),
      api.getGoldenStatus().catch(() => null),
    ]);
    set({ vitality, lessons, goldenStatus });
    await get().refreshSkills();
  },
  forgetLesson: async (name) => {
    const undoToken = await api.forgetLesson(name);
    set({ memoryToast: { op: "forget", name, description: name, collection: "lessons", undoToken } });
    await get().refreshSelf();
    await get().refreshMemoryContext();
  },

  skills: [],
  refreshSkills: async () => {
    if (!api.inTauri()) return;
    const conv = get().conversations.find((c) => c.id === get().activeConversationId);
    try {
      set({ skills: await api.listSkills(conv?.folderPath ?? null) });
    } catch {
      // Left as-is: a failed refresh keeps the last known list rather than
      // blanking the prompt's skills block mid-conversation.
    }
  },
  setSkillEnabled: async (source, name, enabled) => {
    // `GLD-2`: switching a skill *on* injects new instructions into every
    // prompt, so the backend checks itself before and after. That's a couple
    // of model passes — the toggle would otherwise look frozen, so say so.
    if (enabled) set({ checkingGolden: true });
    try {
      await api.setSkillEnabled(source, name, enabled, cloudTarget());
    } finally {
      if (enabled) set({ checkingGolden: false });
    }
    await get().refreshSkills();
    if (enabled) await get().refreshSelf();
  },
  forgetSkill: async (name) => {
    await api.forgetSkill(name);
    await get().refreshSkills();
  },

  toolHealth: [],
  refreshToolHealth: async () => {
    if (!api.inTauri()) return;
    const model = get().models.find((m) => m.id === get().selectedModelId);
    // Health is per model: a tool a small local model fumbles isn't broken.
    const name = isRemoteModel(model) ? model.cloudModel : undefined;
    try {
      set({ toolHealth: await api.getToolHealth(name) });
    } catch {
      /* no stats yet is the normal case */
    }
  },

  healToast: null,
  dismissHealToast: () => set({ healToast: null }),
  expirySweptToast: null,
  dismissExpirySweptToast: () => set({ expirySweptToast: null }),
  goldenRevertedToast: null,
  dismissGoldenRevertedToast: () => set({ goldenRevertedToast: null }),
  mailSentToast: null,
  dismissMailSentToast: () => set({ mailSentToast: null }),
  goldenStatus: null,
  goldenError: "",
  checkingGolden: false,
  checkGoldenNow: async () => {
    if (!api.inTauri()) return;
    set({ checkingGolden: true, goldenError: "" });
    try {
      const goldenStatus = await api.checkGolden(cloudTarget());
      set({ goldenStatus });
    } catch (e) {
      // No engine loaded is the common case, and a button that silently does
      // nothing reads as broken — say which it was.
      set({ goldenError: String(e) });
    } finally {
      set({ checkingGolden: false });
    }
  },
  autoReflect: true,
  setAutoReflect: async (autoReflect) => {
    set({ autoReflect });
    if (api.inTauri()) await api.setSetting(REFLECT_AUTO_KEY, autoReflect ? "true" : "false");
  },

  autonomy: {},
  setAutonomy: async (cls, rung) => {
    set((s) => ({ autonomy: { ...s.autonomy, [cls]: rung } }));
    if (api.inTauri()) await api.setSetting(`autonomy.${cls}`, rung);
  },

  selfBorn: null,
  selfIntroduced: false,
  dismissIntroduction: async () => {
    set({ selfIntroduced: true });
    if (api.inTauri()) await api.setSetting(SELF_INTRODUCED_KEY, "true");
  },

  // ---- scheduled jobs (SCH): the quiet night shift ----
  scheduledJobs: [],
  runningJob: null,
  digest: null,
  refreshScheduler: async () => {
    if (!api.inTauri()) return;
    const [scheduledJobs, runningJob, digest] = await Promise.all([
      api.listScheduledJobs().catch(() => [] as api.ScheduledJob[]),
      api.schedulerStatus().catch(() => null),
      api.getSchedulerDigest().catch(() => null),
    ]);
    set({ scheduledJobs, runningJob, digest });
    if (digest) {
      get().maybeFirstTime(
        "digest",
        "This is a digest — a note I leave after reading back over recent conversations on my own."
      );
    }
  },
  createScheduledJob: async (input) => {
    await api.createScheduledJob(input);
    await get().refreshScheduler();
  },
  updateScheduledJob: async (id, input) => {
    await api.updateScheduledJob(id, input);
    await get().refreshScheduler();
  },
  deleteScheduledJob: async (id) => {
    await api.deleteScheduledJob(id);
    await get().refreshScheduler();
  },
  runScheduledJobNow: async (id) => {
    const result = await api.runScheduledJobNow(id);
    await get().refreshScheduler();
    return result;
  },
  stopScheduledJob: async () => {
    await api.stopScheduledJob();
    await get().refreshScheduler();
  },
  taskDraft: null,
  scheduleConversation: (conversationId) => {
    const conv = get().conversations.find((c) => c.id === conversationId);
    // Seed the instructions from what was actually asked here — the first real
    // request in the chat. A task made from a conversation should arrive
    // already saying something, not as an empty box next to a chat you now
    // have to re-read and summarise yourself.
    const firstAsk = conv?.messages.find((m) => m.role === "user")?.text.trim() ?? "";
    set({
      taskDraft: {
        name: conv?.title ?? "New task",
        prompt: firstAsk.slice(0, 2000),
        conversationId,
      },
    });
    get().setView("tasks");
  },
  clearTaskDraft: () => set({ taskDraft: null }),
  dismissDigest: async () => {
    set((s) => (s.digest ? { digest: { ...s.digest, unread: false } } : {}));
    if (api.inTauri()) await api.markDigestRead().catch(() => {});
  },

  startFromSkill: async (skill) => {
    // Most skills are just work, and work is a conversation. Only a skill that
    // actually ships a surface template has anything to put in a workspace, so
    // that — not the act of starting a skill — is what decides the mode. Asked
    // before the conversation exists, because `newConversation` pins the
    // workspace flag onto the row it creates. A fresh chat has no folder yet,
    // hence the `null`.
    let treeJson: string | null = null;
    if (api.inTauri()) {
      try {
        treeJson = await api.skillSurface(skill.name, null);
      } catch {
        /* a bad or missing template shouldn't stop the skill from running */
      }
    }

    const wasWorkspace = get().workspaceMode;
    // The `skill` tool is how the model reads the steps at all (SKL-2 stage 2);
    // with tools off, "start from a skill" would start nothing.
    set({ workspaceMode: !!treeJson, toolsEnabled: true });
    await get().newConversation();
    const convId = get().activeConversationId;
    if (!convId) {
      set({ workspaceMode: wasWorkspace });
      return;
    }
    set((s) => ({
      view: "chat",
      conversations: s.conversations.map((c) =>
        c.id === convId ? { ...c, skillName: skill.name } : c
      ),
    }));
    // Seed the template first, so the workspace is already furnished when the
    // agent's first turn arrives — the skill visibly hatches (PRES-7).
    if (treeJson) {
      try {
        const id = await api.setSurface(convId, treeJson);
        set((s) => ({
          surfaces: {
            ...s.surfaces,
            [convId]: { id, kind: "surface", title: "Workspace", data: JSON.parse(treeJson!) },
          },
        }));
      } catch {
        /* same: a surface that won't load isn't a reason not to run the skill */
      }
    }
    // Naming the skill *is* the kickoff prompt: the model reads the steps
    // itself with the `skill` tool (SKL-2 stage 2), rather than us pasting a
    // body the user would then see twice.
    await get().sendMessage(`Use your "${skill.name}" skill.`);
  },

  contextBudget: DEFAULT_LOCAL_CTX,
  autoCompact: true,
  setAutoCompact: async (autoCompact) => {
    set({ autoCompact });
    if (api.inTauri()) await api.setSetting(AUTOCOMPACT_KEY, autoCompact ? "true" : "false");
  },
  refreshContextBudget: async () => {
    const state = get();
    const model = state.models.find((m) => m.id === state.selectedModelId) ?? state.models[0];
    set({ contextBudget: await resolveBudget(model) });
  },

  bootstrap: async () => {
    if (get().bootstrapped) return;
    if (!api.inTauri()) {
      // Browser preview: use mock data.
      set({
        conversations: mockConversations,
        activeConversationId: mockConversations[0]?.id ?? null,
        bootstrapped: true,
      });
      return;
    }

    const [
      rows,
      prompt,
      readingScaleRaw,
      telemetryRaw,
      autoCompactRaw,
      expertRaw,
      onboardedRaw,
      bornRaw,
      introducedRaw,
      autoReflectRaw,
      dockOpenRaw,
      dockWidthRaw,
      recallDeclinedRaw,
      indexExplainedRaw,
      firstTimeRaw,
      ...autonomyRaw
    ] = await Promise.all([
      api.listConversations(),
      api.getSetting(SYSTEM_PROMPT_KEY),
      api.getSetting(READING_SCALE_KEY),
      api.getSetting(TELEMETRY_KEY),
      api.getSetting(AUTOCOMPACT_KEY),
      api.getSetting(EXPERT_KEY),
      api.getSetting(MEMORY_ONBOARDED_KEY),
      api.getSetting(SELF_BORN_KEY),
      api.getSetting(SELF_INTRODUCED_KEY),
      api.getSetting(REFLECT_AUTO_KEY),
      api.getSetting(DOCK_OPEN_KEY),
      api.getSetting(DOCK_WIDTH_KEY),
      api.getSetting(RECALL_DECLINED_KEY),
      api.getSetting(INDEX_EXPLAINED_KEY),
      // SMP-7a: loaded with the rest rather than a beat later. The refreshes
      // below call `maybeFirstTime`, and a flag that hasn't landed yet reads
      // as "never explained" — so this has to be settled before any of them
      // run, not merely soon after.
      Promise.all(FIRST_TIME_KEYS.map((k) => api.getSetting(`onboarded.${k}`))),
      ...AUTONOMY_CLASSES.map((c) => api.getSetting(`autonomy.${c.id}`)),
    ]);
    let conversations = rows.map(toConversation);
    if (conversations.length === 0) {
      const created = await api.createConversation("New chat");
      conversations = [toConversation(created)];
    }
    const readingScale = readingScaleRaw ? Number(readingScaleRaw) || 1 : 1;
    applyReadingScale(readingScale);
    // Poiesis's birthday is set once, the first time it runs after this ships —
    // the growth narrative counts from there, not from an install timestamp we
    // never recorded.
    let selfBorn = bornRaw ? Number(bornRaw) : NaN;
    if (!Number.isFinite(selfBorn)) {
      selfBorn = Date.now();
      api.setSetting(SELF_BORN_KEY, String(selfBorn)).catch(() => {});
    }
    const autonomy: Record<string, string> = {};
    AUTONOMY_CLASSES.forEach((c, i) => {
      autonomy[c.id] = autonomyRaw[i] || c.fallback;
    });
    const firstTimeFlags: Record<string, boolean> = {};
    FIRST_TIME_KEYS.forEach((k, i) => {
      firstTimeFlags[k] = firstTimeRaw[i] === "true";
    });
    set({
      selfBorn,
      selfIntroduced: introducedRaw === "true",
      // Learning from finished work is on unless the user turned it off.
      autoReflect: autoReflectRaw !== "false",
      autonomy,
      conversations,
      activeConversationId: conversations[0].id,
      systemPrompt: prompt ?? DEFAULT_SYSTEM_PROMPT,
      readingScale,
      telemetryEnabled: telemetryRaw === "true",
      // Homeostasis is on unless the user turned it off.
      autoCompact: autoCompactRaw !== "false",
      expert: expertRaw === "true",
      memoryOnboarded: onboardedRaw === "true",
      firstTimeFlags,
      firstTimeFlagsLoaded: true,
      recallDeclined: recallDeclinedRaw === "true",
      indexExplained: indexExplainedRaw === "true",
      // The Workbench is open unless the user closed it last time.
      dockOpen: dockOpenRaw !== "0",
      dockWidth: Math.min(720, Math.max(260, Number(dockWidthRaw) || DEFAULT_DOCK_WIDTH)),
      bootstrapped: true,
    });
    // Swallowed deliberately: a failed library read must not abort the rest of
    // bootstrap. It used to throw straight out of here, which skipped every
    // refresh below — including the one that flips `modelsLoaded`, so the
    // first-run guide could never appear on exactly the broken installs that
    // needed it most.
    await get().refreshLibrary().catch(() => {});
    // Cloud models load in the background (network) — don't block startup.
    // `modelsLoaded` flips once they land, so anything that keys off "no
    // models and no keys" (the first-run guide) judges a list that has
    // actually arrived rather than one that is merely still empty.
    Promise.all([get().refreshCloud(), get().refreshMediaModels(), get().refreshEndpoints()]).finally(() =>
      set({ modelsLoaded: true })
    );
    get().refreshPersonas();
    get().refreshContextBudget();
    get().refreshMemoryContext();
    get().refreshChangeProposals();
    get().refreshMemoryToolset();
    get().refreshSelf();
    get().refreshToolHealth();
    get().refreshScheduler();
    listenForSelfEvents(set, get);
    maybeDailyProfileTick(get);
    await get().setActiveConversation(conversations[0].id);
  },

  setActiveConversation: async (id) => {
    // Rail rows call this unconditionally on every click, including a click on
    // the chat that's already active (e.g. returning to it from Settings) —
    // so "did the conversation actually change" has to be judged here, before
    // any of the below overwrites it with itself.
    const switchingConversation = id !== get().activeConversationId;

    // Leaving a conversation is when it becomes reviewable: it's finished
    // enough to learn from, and the user isn't waiting on anything (REF-3).
    // Fire-and-forget — reflection must never sit in the navigation path.
    const leaving = get().conversations.find((c) => c.id === get().activeConversationId);
    if (
      leaving &&
      leaving.id !== id &&
      !leaving.reflectedAt &&
      leaving.messages.length >= REFLECT_MIN_MESSAGES &&
      get().autoReflect &&
      api.inTauri()
    ) {
      get().reflectConversation(leaving.id).catch(() => {});
    }

    // A conversation carries its own workspace flag — switching sessions adopts
    // that session's layout (composed surface vs. classic message stream).
    const conv = get().conversations.find((c) => c.id === id);
    // The Workbench belongs to the conversation, not the window: its folder,
    // tree, selection and change history all reset and reload with the session.
    // A media model is sticky across messages but not across conversations
    // (Path E step 6) — "make me a picture" is a session, not a personality.
    // A new session opens on the chat model the user was last talking to.
    // Gated on an actual switch: re-selecting the already-active conversation
    // (e.g. clicking its Rail row to return from another view) must not snap
    // a just-picked media model back to the last chat model.
    const current = get().models.find((m) => m.id === get().selectedModelId);
    const restoreChatModel = switchingConversation && current?.modality && current.modality !== "chat";

    set({
      activeConversationId: id,
      view: "chat",
      ...(restoreChatModel ? { selectedModelId: get().lastChatModelId } : {}),
      workspaceMode: !!conv?.workspace,
      selected: null,
      viewerExpanded: false,
      folderTree: {},
      expandedDirs: [],
      touchedFiles: {},
      trash: [],
      folderError: null,
      indexState: null,
      indexProgress: null,
      indexError: null,
      duplicateGroups: null,
      duplicateScanPath: null,
      duplicatesError: null,
    });
    if (!api.inTauri()) return;
    get().refreshTree().catch(() => {});
    get().refreshTrash().catch(() => {});
    get().refreshIndexStatus().catch(() => {});
    const rows = await api.listMessages(id);
    // Load any saved workspace blocks and attach them to their anchor message
    // (Generative UI). A block with no message_id trails the last assistant turn.
    let blocksByMessage: Record<string, BlockView[]> = {};
    let orphanBlocks: BlockView[] = [];
    let surface: BlockView | undefined;
    try {
      const dbBlocks = await api.listBlocks(id);
      for (const b of dbBlocks) {
        const view = toBlockView(b);
        // The live composed interface lives in its own slice, not the transcript.
        if (b.kind === "surface") {
          surface = view;
          continue;
        }
        if (b.message_id) {
          (blocksByMessage[b.message_id] ??= []).push(view);
        } else {
          orphanBlocks.push(view);
        }
      }
    } catch {
      /* ignore */
    }
    // Load any saved artifacts for the Workbench (CHT-6) and re-attach each one
    // to the turn that made it, or its inline chip (`ArtifactChips`) vanishes
    // from the message stream on every reload even though the artifact itself
    // is still right there in the Workbench's own list.
    let arts: api.Artifact[] = [];
    let artifactIdsByMessage: Record<string, string[]> = {};
    try {
      arts = await api.listArtifacts(id);
      for (const a of arts) {
        if (a.message_id) (artifactIdsByMessage[a.message_id] ??= []).push(a.id);
      }
    } catch {
      /* ignore */
    }
    const messages = rows.map(toMessage);
    for (const m of messages) {
      if (blocksByMessage[m.id]) m.blocks = blocksByMessage[m.id];
      if (artifactIdsByMessage[m.id]) m.artifactIds = artifactIdsByMessage[m.id];
    }
    // `JOB-1`: a generation can outlive the view that started it. Re-attach to
    // anything still running so the turn shows its tile and its Cancel again,
    // rather than a finished-looking turn with nothing in it.
    try {
      const running = await api.listRunningMediaJobs(id);
      if (running.length) {
        const tracked: AppState["mediaJobs"] = {};
        for (const job of running) {
          const target = job.message_id && messages.find((m) => m.id === job.message_id);
          if (!target) continue;
          target.streaming = true;
          target.pendingMedia = {
            modality: job.modality,
            aspectRatio: job.aspect_ratio ?? undefined,
            startedAt: job.started_at,
            jobId: job.id,
          };
          tracked[job.id] = {
            conversationId: id,
            messageId: target.id,
            stepId: target.steps?.[0]?.id ?? "",
          };
        }
        set((s) => ({ mediaJobs: { ...s.mediaJobs, ...tracked } }));
      }
    } catch {
      /* ignore */
    }
    if (orphanBlocks.length) {
      const lastAssistant = [...messages].reverse().find((m) => m.role === "assistant");
      if (lastAssistant) lastAssistant.blocks = [...(lastAssistant.blocks ?? []), ...orphanBlocks];
    }
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === id ? { ...c, messages } : c
      ),
      surfaces: { ...s.surfaces, [id]: surface },
      // Nothing is selected on arrival — opening a chat shouldn't yank the
      // viewer onto an old artifact.
      artifacts: { ...s.artifacts, [id]: arts },
    }));
    // Load durable session state for context injection + the header strip (Phase C).
    try {
      const raw = await api.getSessionState(id);
      const parsed = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
      set((s) => ({ sessionState: { ...s.sessionState, [id]: parsed } }));
    } catch {
      /* ignore */
    }
  },

  newConversation: async () => {
    // Starting a chat while workspace mode is on pins the new session to it.
    const workspace = get().workspaceMode;
    if (!api.inTauri()) {
      const id = `c-${Date.now()}`;
      set((s) => ({
        conversations: [
          { id, title: "New chat", updatedAt: Date.now(), messages: [], workspace },
          ...s.conversations,
        ],
        activeConversationId: id,
        view: "chat",
      }));
      return;
    }
    const created = await api.createConversation("New chat", undefined, workspace);
    set((s) => ({
      conversations: [toConversation(created), ...s.conversations],
      activeConversationId: created.id,
      view: "chat",
    }));
  },

  renameConversation: async (id, title) => {
    set((s) => ({
      conversations: s.conversations.map((c) => (c.id === id ? { ...c, title } : c)),
    }));
    if (api.inTauri()) await api.renameConversation(id, title);
  },

  deleteConversation: async (id) => {
    if (api.inTauri()) await api.deleteConversation(id);
    set((s) => {
      const remaining = s.conversations.filter((c) => c.id !== id);
      const active = s.activeConversationId === id ? remaining[0]?.id ?? null : s.activeConversationId;
      return { conversations: remaining, activeConversationId: active };
    });
  },

  sendMessage: async (text, attachments = []) => {
    const state = get();
    const convId = state.activeConversationId;
    if (!convId || state.busy) return;
    const model = state.models.find((m) => m.id === state.selectedModelId) ?? state.models[0];

    // Belt and braces (`PIK-2`): `run_agent` must never be handed an image or
    // video model. The composer already routes these to `createMedia`, but a
    // media id reaching the agent loop would fail deep in the engine with an
    // unrecognisable error, so reroute here rather than trusting one caller.
    if (model?.modality && model.modality !== "chat") {
      await get().createMedia({ prompt: text, modelId: model.id });
      return;
    }

    const conv = state.conversations.find((c) => c.id === convId);
    const isFirstMessage = !conv || conv.messages.length === 0;

    // Optimistic user turn.
    const userMsg: Message = {
      id: `u-${Date.now()}`,
      role: "user",
      text,
      attachments: attachments.length ? attachments : undefined,
      createdAt: Date.now(),
    };
    const assistantId = `a-${Date.now()}`;
    const assistantMsg: Message = {
      id: assistantId,
      role: "assistant",
      model: { name: model.name, provenance: model.provenance },
      steps: [],
      text: "",
      streaming: true,
      createdAt: Date.now() + 1,
    };
    set((s) => ({
      busy: true,
      presence: "active",
      conversations: s.conversations.map((c) =>
        c.id === convId
          ? { ...c, updatedAt: Date.now(), messages: [...c.messages, userMsg, assistantMsg] }
          : c
      ),
      // `EDT-2`: the implicit reference only offers a *recent* image — three
      // turns out, "make it warmer" is more likely about something else the
      // conversation has moved on to than about a picture from a while ago.
      lastMediaArtifact:
        s.lastMediaArtifact && s.lastMediaArtifact.conversationId === convId
          ? s.lastMediaArtifact.turnsAgo >= 3
            ? null
            : { ...s.lastMediaArtifact, turnsAgo: s.lastMediaArtifact.turnsAgo + 1 }
          : s.lastMediaArtifact,
    }));

    if (isFirstMessage) {
      get().renameConversation(convId, deriveTitle(text));
    }

    // Browser preview can't reach a model.
    if (!api.inTauri()) {
      patchAssistant(set, convId, assistantId, {
        text: "_Run the desktop app with a model loaded to get a real response._",
        streaming: false,
      });
      set({ busy: false });
      return;
    }

    // Persist the user message and an empty assistant row.
    let persistedAssistantId = assistantId;
    try {
      await api.appendMessage({
        conversationId: convId,
        role: "user",
        content: text,
        attachments: attachments.length
          ? attachments.map((a) => ({ kind: a.kind, name: a.name, path: a.path || "" }))
          : undefined,
      });
      const row = await api.appendMessage({
        conversationId: convId,
        role: "assistant",
        content: "",
        modelName: model.name,
        modelProvenance: model.provenance,
      });
      persistedAssistantId = row.id;
    } catch {
      // Non-fatal; we still stream into the optimistic message.
    }

    // Readiness gating (§7.4): a local turn needs llama-server actually running
    // *this* model. Start it on demand (visible via the engine-status indicator)
    // rather than letting the request fail with "No model is loaded yet".
    const failTurn = async (msg: string) => {
      patchAssistant(set, convId, assistantId, { text: msg, streaming: false });
      set({ busy: false });
      try {
        await api.finalizeMessage(persistedAssistantId, msg, undefined);
      } catch {
        /* ignore */
      }
    };
    if (model.provenance === "local") {
      const st = get();
      if (!st.engineReady || st.loadedModelId !== model.id) {
        try {
          await get().loadModelById(model.id);
        } catch (e) {
          await failTurn(
            `I couldn't start the engine for “${model.name}”. ${String(e)}`
          );
          return;
        }
      }
      if (!get().engineReady) {
        await failTurn(
          "No model is loaded yet. Open Models and choose a model to start the engine."
        );
        return;
      }
    }

    // Resolve attachments into model-ready content (CHT-5, CHT-8).
    const images = attachments.filter((a) => a.kind === "image");
    const pdfs = attachments.filter((a) => a.kind === "pdf");
    let textForModel = text;

    // W2: carry queued workspace interactions (checked steps, pins) as context
    // on this message instead of having spent a model turn each.
    const pending = get().pendingActions[convId] ?? [];
    if (pending.length) {
      textForModel = `(Workspace updates since your last reply: ${pending.join("; ")}.)\n\n${textForModel}`;
      set((s) => ({ pendingActions: { ...s.pendingActions, [convId]: [] } }));
    }
    for (const pdf of pdfs) {
      try {
        const extracted = await api.extractPdfText(pdf.path);
        const body = extracted.trim()
          ? extracted.slice(0, 20000)
          : "(No selectable text — this looks like a scanned PDF.)";
        textForModel += `\n\n[Attached PDF: ${pdf.name}]\n${body}`;
      } catch {
        textForModel += `\n\n[Attached PDF: ${pdf.name} — couldn't be read]`;
      }
    }
    const visionOk = !!model.vision;
    if (images.length && !visionOk) {
      textForModel += `\n\n(I attached ${images.length} image(s), but “${model.name}” can't see images. Pick a vision-capable model to use them.)`;
    }

    let userContent: api.ChatTurnMessage["content"] = textForModel;
    if (images.length && visionOk) {
      const parts: api.ContentPart[] = [{ type: "text", text: textForModel }];
      for (const img of images) {
        try {
          // Pasted / browser-dropped images carry their bytes inline; file-picker
          // and native-drop images are read from disk by path.
          const url = img.dataUri ?? (await api.readImageDataUri(img.path));
          parts.push({ type: "image_url", image_url: { url } });
        } catch {
          /* skip unreadable image */
        }
      }
      userContent = parts;
    }

    // Resolve the effective persona/overrides for this conversation
    // (CHT-4/CHT-7): conversation override → persona → global default.
    const persona = conv?.personaId
      ? state.personas.find((p) => p.id === conv.personaId)
      : undefined;
    const baseSystemPrompt = persona?.system_prompt ?? get().systemPrompt;
    const { memory, matches, injectedFacts } = await recallForPrompt(get, text);
    const effectiveSystemPrompt = composeSystemPrompt(baseSystemPrompt, {
      conv: get().conversations.find((c) => c.id === convId),
      sessionState: get().sessionState[convId],
      toolsEnabled: get().toolsEnabled,
      surface: get().surfaces[convId],
      memory,
      toolHealth: get().toolHealth,
      skills: skillsForPersona(get().skills, persona?.skills_json),
    });
    const effectiveTemperature =
      conv?.overrides?.temperature ?? personaTemperature(persona);

    // System prompt + as much history as the context window holds + this turn.
    const turns = await assembleTurns(set, get, {
      convId,
      system: effectiveSystemPrompt,
      current: { role: "user", content: userContent },
      model,
    });

    const recalled = recallStep(matches);
    await streamAssistantTurn(set, get, {
      convId,
      assistantId,
      persistedAssistantId,
      turns,
      model,
      temperature: effectiveTemperature,
      initialSteps: recalled ? [recalled] : undefined,
      contextRefs: buildContextRefs({ personaId: persona?.id ?? null, memory, injectedFacts, matches }),
    });
  },

  sendBlockAction: async (blockId, humanText, payload) => {
    const state = get();
    const convId = state.activeConversationId;
    if (!convId || state.busy) return;
    const conv = state.conversations.find((c) => c.id === convId);
    const model = state.models.find((m) => m.id === state.selectedModelId) ?? state.models[0];
    if (!model) return;

    // Deterministic session-state auto-patch from the interaction (Phase C):
    // no model round-trip needed to remember pins / form submissions.
    const autoPatch = autoPatchForAction(conv, blockId, payload);
    if (autoPatch) applySessionPatch(set, get, convId, autoPatch);

    // W2 (workspace mode): pure state mutations don't spend a model turn — the
    // UI already applied them. Queue a note the next real message will carry.
    const action = String(payload.action ?? "");
    if (state.workspaceMode && (action === "pin" || action === "unpin" || action === "set_step")) {
      set((s) => ({
        pendingActions: {
          ...s.pendingActions,
          [convId]: [...(s.pendingActions[convId] ?? []), humanText],
        },
      }));
      return;
    }

    // The model sees the sentence plus a compact action payload; the transcript
    // renders it as a chip (see UserTurn).
    const modelContent = `${humanText}\n\n\`\`\`poiesis-action\n${JSON.stringify(payload)}\n\`\`\``;

    const userMsg: Message = {
      id: `u-${Date.now()}`,
      role: "user",
      text: modelContent,
      createdAt: Date.now(),
    };
    const assistantId = `a-${Date.now()}`;
    const assistantMsg: Message = {
      id: assistantId,
      role: "assistant",
      model: { name: model.name, provenance: model.provenance },
      steps: [],
      text: "",
      streaming: true,
      createdAt: Date.now() + 1,
    };
    set((s) => ({
      busy: true,
      presence: "active",
      conversations: s.conversations.map((c) =>
        c.id === convId
          ? { ...c, updatedAt: Date.now(), messages: [...c.messages, userMsg, assistantMsg] }
          : c
      ),
    }));

    if (!api.inTauri()) {
      patchAssistant(set, convId, assistantId, { streaming: false });
      // Back to resting unless a self-process is still working (PRES-1).
    set((st) => ({ busy: false, presence: st.reflectingIds.length ? "reflecting" : "idle" }));
      return;
    }

    let persistedAssistantId = assistantId;
    try {
      await api.appendMessage({ conversationId: convId, role: "user", content: modelContent });
      const row = await api.appendMessage({
        conversationId: convId,
        role: "assistant",
        content: "",
        modelName: model.name,
        modelProvenance: model.provenance,
      });
      persistedAssistantId = row.id;
    } catch {
      /* non-fatal */
    }

    // Reuse the same engine-readiness gate as a normal turn.
    const engineError = await ensureEngineForModel(get, model);
    if (engineError) {
      patchAssistant(set, convId, assistantId, { text: engineError, streaming: false });
      // Back to resting unless a self-process is still working (PRES-1).
    set((st) => ({ busy: false, presence: st.reflectingIds.length ? "reflecting" : "idle" }));
      try {
        await api.finalizeMessage(persistedAssistantId, engineError, undefined);
      } catch {
        /* ignore */
      }
      return;
    }

    const persona = conv?.personaId
      ? state.personas.find((p) => p.id === conv.personaId)
      : undefined;
    const baseSystemPrompt = persona?.system_prompt ?? get().systemPrompt;
    const { memory, matches, injectedFacts } = await recallForPrompt(get, humanText);
    const effectiveSystemPrompt = composeSystemPrompt(baseSystemPrompt, {
      conv: get().conversations.find((c) => c.id === convId),
      sessionState: get().sessionState[convId],
      toolsEnabled: get().toolsEnabled,
      surface: get().surfaces[convId],
      memory,
      toolHealth: get().toolHealth,
      skills: skillsForPersona(get().skills, persona?.skills_json),
    });
    const effectiveTemperature =
      conv?.overrides?.temperature ?? personaTemperature(persona);

    const turns = await assembleTurns(set, get, {
      convId,
      system: effectiveSystemPrompt,
      current: { role: "user", content: modelContent },
      model,
    });

    const recalled = recallStep(matches);
    await streamAssistantTurn(set, get, {
      convId,
      assistantId,
      persistedAssistantId,
      turns,
      model,
      temperature: effectiveTemperature,
      initialSteps: recalled ? [recalled] : undefined,
      contextRefs: buildContextRefs({ personaId: persona?.id ?? null, memory, injectedFacts, matches }),
    });
  },

  setBlockState: (blockId, blockState) => {
    const convId = get().activeConversationId;
    if (!convId) return;
    patchBlock(set, convId, blockId, { state: blockState });
    if (api.inTauri()) {
      api.updateBlockState(blockId, JSON.stringify(blockState)).catch(() => {});
    }
  },

  browserSessions: {},
  stopBrowsing: async (conversationId) => {
    set((s) => {
      const open = s.browserSessions[conversationId];
      if (!open) return {};
      return {
        browserSessions: { ...s.browserSessions, [conversationId]: { ...open, closed: true } },
      };
    });
    if (api.inTauri()) {
      await api.stopBrowser(conversationId).catch(() => {});
    }
  },
  dismissBrowserPanel: (conversationId) => {
    set((s) => {
      const next = { ...s.browserSessions };
      delete next[conversationId];
      return { browserSessions: next };
    });
    // The record outlives the live session, so dismissing has to clear it too
    // — otherwise the panel reappears the next time this chat is opened.
    if (api.inTauri()) api.forgetBrowserSession(conversationId).catch(() => {});
  },
  refreshBrowserSession: async (conversationId) => {
    if (!api.inTauri()) return;
    // A reload wipes the store but not the live Chrome process — without this,
    // the panel would stay blank while a session is still open.
    const state = await api.browserState(conversationId).catch(() => null);
    set((s) => {
      if (!state) {
        if (!s.browserSessions[conversationId]) return {};
        const next = { ...s.browserSessions };
        delete next[conversationId];
        return { browserSessions: next };
      }
      return { browserSessions: { ...s.browserSessions, [conversationId]: state } };
    });
  },

  surfaces: {},
  setSurfaceState: (stateObj) => {
    const convId = get().activeConversationId;
    if (!convId) return;
    const surface = get().surfaces[convId];
    if (!surface) return;
    set((s) => ({ surfaces: { ...s.surfaces, [convId]: { ...surface, state: stateObj } } }));
    if (api.inTauri()) {
      api.updateBlockState(surface.id, JSON.stringify(stateObj)).catch(() => {});
    }
  },
  sendSurfaceAction: async (humanText, payload) => {
    const convId = get().activeConversationId;
    if (!convId) return;
    const surface = get().surfaces[convId];
    if (!surface) return;
    await get().sendBlockAction(surface.id, humanText, {
      a: "ui_action",
      ...payload,
      state: (surface.state as Record<string, unknown>) ?? {},
    });
  },

  sessionState: {},
  pendingActions: {},

  clearSessionStateKey: (path) => {
    const convId = get().activeConversationId;
    if (!convId) return;
    // Support dotted paths ("constraints.budget") via a nested null patch.
    const parts = path.split(".");
    let patch: Record<string, unknown> = { [parts[parts.length - 1]]: null };
    for (let i = parts.length - 2; i >= 0; i--) patch = { [parts[i]]: patch };
    applySessionPatch(set, get, convId, patch);
  },

  stopGenerating: () => {
    if (api.inTauri()) api.stopChat().catch(() => {});
  },

  setSystemPrompt: async (prompt) => {
    set({ systemPrompt: prompt });
    if (api.inTauri()) await api.setSetting(SYSTEM_PROMPT_KEY, prompt);
  },

  resolvePermission: async (id, decision) => {
    const request = get().pendingPermissions.find((p) => p.id === id);
    set((s) => ({ pendingPermissions: s.pendingPermissions.filter((p) => p.id !== id) }));
    // "Don't ask again in this folder" raises the trust level backend-side —
    // mirror it so the header's segmented control matches what just happened.
    if (request?.in_folder && decision === "forever") {
      const convId = get().activeConversationId;
      if (convId) {
        set((s) => ({
          conversations: s.conversations.map((c) =>
            c.id === convId ? { ...c, folderTrust: "auto" as FolderTrust } : c
          ),
        }));
      }
    }
    if (api.inTauri()) await api.resolvePermission(id, decision);
  },

  artifacts: {},
  dockOpen: true,
  toggleDock: () => {
    const dockOpen = !get().dockOpen;
    set({ dockOpen });
    if (api.inTauri()) api.setSetting(DOCK_OPEN_KEY, dockOpen ? "1" : "0").catch(() => {});
  },
  setDockOpen: (dockOpen) => {
    if (get().dockOpen === dockOpen) return;
    set({ dockOpen });
    if (api.inTauri()) api.setSetting(DOCK_OPEN_KEY, dockOpen ? "1" : "0").catch(() => {});
  },
  selected: null,
  selectNode: (selection) => set({ selected: selection, viewerExpanded: false }),
  openArtifact: (artifactId) => {
    const convId = get().activeConversationId;
    const artifact = convId
      ? (get().artifacts[convId] ?? []).find((a) => a.id === artifactId)
      : undefined;
    // A saved artifact is a file now — show it where it actually lives.
    set({
      dockOpen: true,
      viewerExpanded: false,
      selected: artifact?.saved_path
        ? { kind: "file", id: artifact.saved_path }
        : { kind: "artifact", id: artifactId },
    });
  },
  viewerExpanded: false,
  setViewerExpanded: (viewerExpanded) => set({ viewerExpanded }),
  dockWidth: DEFAULT_DOCK_WIDTH,
  setDockWidth: (px) => {
    // Floor keeps the tree usable; ceiling keeps the conversation readable.
    const dockWidth = Math.round(Math.min(720, Math.max(260, px)));
    if (get().dockWidth === dockWidth) return;
    set({ dockWidth });
    // Only worth persisting once the drag settles; `setDockDragging(false)`
    // does that, so the write here is skipped mid-drag.
    if (!get().dockDragging && api.inTauri()) {
      api.setSetting(DOCK_WIDTH_KEY, String(dockWidth)).catch(() => {});
    }
  },
  dockDragging: false,
  setDockDragging: (dockDragging) => {
    set({ dockDragging });
    if (!dockDragging && api.inTauri()) {
      api.setSetting(DOCK_WIDTH_KEY, String(get().dockWidth)).catch(() => {});
    }
  },
  showHidden: false,
  toggleShowHidden: () => {
    set((s) => ({ showHidden: !s.showHidden, folderTree: {} }));
    get().refreshTree().catch(() => {});
  },

  folderTree: {},
  expandedDirs: [],
  touchedFiles: {},
  trash: [],
  folderError: null,
  indexState: null,
  indexProgress: null,
  indexError: null,
  indexExplained: false,

  duplicateGroups: null,
  duplicateScanPath: null,
  duplicatesLoading: false,
  duplicatesError: null,

  toggleDir: async (path) => {
    const open = get().expandedDirs.includes(path);
    if (open) {
      set((s) => ({ expandedDirs: s.expandedDirs.filter((p) => p !== path) }));
      return;
    }
    set((s) => ({ expandedDirs: [...s.expandedDirs, path] }));
    // Fetch children the first time a branch opens; reopening reuses what we
    // already have, and the agent's file events refresh it when it goes stale.
    if (!get().folderTree[path]) await get().refreshTree(path);
  },

  attachFolder: async () => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    if (!convId) return;
    set({ folderError: null });
    try {
      const picked = await api.pickFolder();
      if (!picked) return;
      await api.setConversationFolder(convId, picked);
      set((s) => ({
        conversations: s.conversations.map((c) =>
          c.id === convId ? { ...c, folderPath: picked } : c
        ),
        folderTree: {},
        expandedDirs: [],
        selected: null,
        dockOpen: true,
        indexState: null,
        indexProgress: null,
        indexError: null,
      }));
      await get().refreshTree();
      get().refreshIndexStatus().catch(() => {});
      // SMP-4a: handing over a folder already means "work here" — reading it
      // is not a second decision, so it starts now and can be stopped.
      get().maybeAutoIndex().catch(() => {});
      // SMP-2b: attaching a folder is one of the two genuine first-need
      // moments for the recall helper.
      get().maybeOfferRecall();
    } catch (e) {
      set({ folderError: String(e) });
    }
  },

  detachFolder: async () => {
    const convId = get().activeConversationId;
    if (!convId) return;
    // Nothing on disk is touched — this only forgets the path.
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === convId ? { ...c, folderPath: null } : c
      ),
      folderTree: {},
      expandedDirs: [],
      selected: s.selected?.kind === "file" ? null : s.selected,
      folderError: null,
      indexState: null,
      indexProgress: null,
      indexError: null,
    }));
    if (api.inTauri()) await api.setConversationFolder(convId, null);
  },

  setFolderTrust: async (trust) => {
    const convId = get().activeConversationId;
    if (!convId) return;
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === convId ? { ...c, folderTrust: trust } : c
      ),
    }));
    if (api.inTauri()) await api.setConversationTrust(convId, trust);
  },

  refreshIndexStatus: async () => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    if (!convId) return;
    try {
      const indexState = await api.indexStatus(convId);
      // A conversation switch (or detach) may have landed in between — don't
      // let a slow response overwrite a newer one.
      if (get().activeConversationId === convId) set({ indexState });
    } catch {
      // Silent: this is a background refresh, not a user action.
    }
  },

  maybeAutoIndex: async () => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    if (!convId) return;
    try {
      if (!(await api.shouldAutoIndex(convId))) return;
      // The conversation may have moved on while we asked.
      if (get().activeConversationId !== convId) return;
      await get().buildFolderIndex();
    } catch {
      // Nothing was promised — a folder that can't be read yet just shows
      // "I haven't read this folder yet" and its `Read it` button.
    }
  },

  buildFolderIndex: async () => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    if (!convId) return;
    // SMP-4c: the first read explains itself, then never again. Set the flag
    // as the read starts, so the sentence shows for exactly one build.
    if (!get().indexExplained) {
      api.setSetting(INDEX_EXPLAINED_KEY, "true").catch(() => {});
    }
    set({ indexError: null, indexProgress: { files_done: 0, files_total: 0 } });
    try {
      const indexState = await api.buildIndex(convId, (p) => {
        if (get().activeConversationId === convId) set({ indexProgress: p });
      });
      if (get().activeConversationId === convId) set({ indexState, indexProgress: null });
    } catch (e) {
      if (get().activeConversationId === convId) {
        set({ indexError: String(e), indexProgress: null });
      }
      // Either way, the row on disk may have changed (reverted to idle, or
      // dropped back to "never built") — pick up the real state rather than
      // leave the stale pre-build one showing.
      get().refreshIndexStatus().catch(() => {});
    } finally {
      // The explanation has now been shown for the length of one read; a
      // second folder attached in the same session doesn't repeat it.
      set({ indexExplained: true });
    }
  },

  cancelFolderIndex: async () => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    if (!convId) return;
    await api.cancelIndex(convId).catch(() => {});
  },

  forgetFolderIndex: async (path) => {
    if (!api.inTauri()) return;
    await api.forgetIndex(path);
    if (get().indexState?.path === path) set({ indexState: null });
  },

  findDuplicatesIn: async (path) => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    if (!convId) return;
    set({ duplicatesLoading: true, duplicatesError: null, duplicateScanPath: path });
    try {
      const groups = await api.findDuplicates(convId, path);
      if (get().activeConversationId === convId) set({ duplicateGroups: groups });
    } catch (e) {
      if (get().activeConversationId === convId) set({ duplicatesError: String(e) });
    } finally {
      if (get().activeConversationId === convId) set({ duplicatesLoading: false });
    }
  },

  keepDuplicate: async (group, keep) => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    if (!convId) return;
    const others = group.files.filter((f) => f !== keep);
    // A file that couldn't be trashed is still on disk, and the group is about
    // to vanish from the panel — so say so rather than let the UI imply a
    // tidy-up that didn't happen.
    const failed: string[] = [];
    for (const path of others) {
      try {
        await api.trashFile(convId, path);
      } catch {
        failed.push(path.split(/[\\/]/).filter(Boolean).pop() ?? path);
      }
    }
    set((s) => ({
      duplicateGroups: (s.duplicateGroups ?? []).filter((g) => g !== group),
      duplicatesError: failed.length ? `I couldn't remove ${failed.join(", ")}.` : s.duplicatesError,
    }));
    get().refreshTrash().catch(() => {});
    get().refreshTree().catch(() => {});
  },

  dismissDuplicates: () => {
    set({ duplicateGroups: null, duplicateScanPath: null, duplicatesError: null });
  },

  refreshTree: async (path) => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    const conv = get().conversations.find((c) => c.id === convId);
    const root = conv?.folderPath;
    if (!root) return;
    // Refreshing the whole tree means the root plus every branch already open,
    // so an agent edit deep in the tree shows up without collapsing anything.
    const targets = path ? [path] : [root, ...get().expandedDirs];
    const showHidden = get().showHidden;
    const loaded = await Promise.all(
      targets.map(async (t) => {
        try {
          return [t, await api.readDirTree(t, convId ?? undefined, showHidden)] as const;
        } catch {
          return [t, [] as api.FileNode[]] as const;
        }
      })
    );
    set((s) => {
      const folderTree = { ...s.folderTree };
      for (const [t, nodes] of loaded) folderTree[t] = nodes;
      return { folderTree };
    });
  },

  refreshTrash: async () => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    if (!convId) return;
    try {
      set({ trash: await api.listTrash(convId, 20) });
    } catch {
      /* ignore */
    }
  },

  undoFileOp: async (id) => {
    if (!api.inTauri()) return;
    const entry = get().trash.find((t) => t.id === id);
    await api.undoFileOp(id);
    set((s) => ({
      trash: s.trash.map((t) => (t.id === id ? { ...t, undone: true } : t)),
      // The file is back to its prior state, so it's no longer "changed".
      touchedFiles: entry
        ? Object.fromEntries(Object.entries(s.touchedFiles).filter(([p]) => p !== entry.path))
        : s.touchedFiles,
    }));
    await get().refreshTree();
  },

  saveArtifactToFolder: async (artifactId, dest) => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    if (!convId) return;
    const written = await api.saveArtifactToFolder(convId, artifactId, dest);
    // The artifact promotes: it stops being "made in this chat" and becomes a
    // file in the tree, selected so the user sees where it landed.
    set((s) => ({
      artifacts: {
        ...s.artifacts,
        [convId]: (s.artifacts[convId] ?? []).map((a) =>
          a.id === artifactId ? { ...a, saved_path: written } : a
        ),
      },
      touchedFiles: { ...s.touchedFiles, [written]: Date.now() },
      selected: { kind: "file", id: written },
    }));
    await get().refreshTree();
    await get().refreshTrash();
  },

  openInSystem: async (path) => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    await api.openPath(path, convId ?? undefined).catch(() => {});
  },
  revealInSystem: async (path) => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    await api.revealPath(path, convId ?? undefined).catch(() => {});
  },

  allArtifacts: [],
  refreshAllArtifacts: async () => {
    if (!api.inTauri()) return;
    try {
      const artifacts = await api.listAllArtifacts();
      set({ allArtifacts: artifacts });
    } catch {
      /* ignore */
    }
  },
  viewArtifact: async (artifact) => {
    if (artifact.conversation_id) {
      await get().setActiveConversation(artifact.conversation_id);
    }
    set((s) => ({
      selected: artifact.saved_path
        ? { kind: "file" as const, id: artifact.saved_path }
        : { kind: "artifact" as const, id: artifact.id },
      dockOpen: true,
      view: artifact.conversation_id ? "chat" : s.view,
    }));
  },

  imageLightbox: null,
  viewArtifactByPath: (path, dataUri, alt) => set({ imageLightbox: { path, dataUri, alt } }),
  closeImageLightbox: () => set({ imageLightbox: null }),
}));

type StoreSet = (fn: (s: AppState) => Partial<AppState>) => void;
type StoreGet = () => AppState;

/** The shared spine behind both generation entry points (`STR-1`): one
 * presentation, two submit calls. Posts a normal-looking agent turn, persists
 * both messages so the background job has a real message to attach its result
 * to, submits, and then gets out of the way.
 *
 * Crucially it releases `busy` as soon as the job is *accepted* (`JOB-1`) —
 * not when the picture is ready. Holding it for the whole generation is what
 * used to stop the user saying anything else for the next three minutes. The
 * turn stays visibly in flight through `pendingMedia`; the composer does not.
 */
async function startMediaTurn(
  set: StoreSet,
  get: StoreGet,
  args: {
    text: string;
    modality: "image" | "video";
    aspectRatio?: string;
    modelLabel?: string;
    provenance?: Provenance;
    /** Path B: the reference has been consumed, so stop offering it. */
    clearImplicitReference?: boolean;
    submit: (ctx: {
      conversationId: string;
      messageId: string | null;
    }) => Promise<api.MediaJob>;
  }
): Promise<void> {
  const state = get();
  const convId = state.activeConversationId;
  const text = args.text.trim();
  if (!convId || state.busy || !text) return;
  const conv = state.conversations.find((c) => c.id === convId);
  const isFirstMessage = !conv || conv.messages.length === 0;

  const stepId = `step-${Date.now()}`;
  const runningStep: AgentStep = {
    id: stepId,
    verb: "generated",
    target: ellipsizeClient(text, 40),
    status: "running",
  };

  if (!api.inTauri()) {
    const assistantId = `a-${Date.now()}`;
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === convId
          ? {
              ...c,
              updatedAt: Date.now(),
              messages: [
                ...c.messages,
                { id: `u-${Date.now()}`, role: "user", text, createdAt: Date.now() },
                {
                  id: assistantId,
                  role: "assistant",
                  text: "_Run the desktop app to generate media._",
                  steps: [{ ...runningStep, status: "error" }],
                  createdAt: Date.now() + 1,
                },
              ],
            }
          : c
      ),
    }));
    return;
  }

  set(() => ({ busy: true, presence: "active" }));

  // Persist both turns before submitting: the worker attaches the finished
  // media to `messageId`, which has to be a row that already exists. These are
  // local SQLite inserts, so the placeholder still appears immediately.
  let userId = `u-${Date.now()}`;
  let assistantId = `a-${Date.now()}`;
  try {
    const userRow = await api.appendMessage({ conversationId: convId, role: "user", content: text });
    userId = userRow.id;
    const assistantRow = await api.appendMessage({
      conversationId: convId,
      role: "assistant",
      content: "",
      modelName: args.modelLabel,
      modelProvenance: args.provenance,
    });
    assistantId = assistantRow.id;
  } catch {
    /* non-fatal — the turn still runs, it just won't survive a reload */
  }

  set((s) => ({
    conversations: s.conversations.map((c) =>
      c.id === convId
        ? {
            ...c,
            updatedAt: Date.now(),
            messages: [
              ...c.messages,
              { id: userId, role: "user", text, createdAt: Date.now() },
              {
                id: assistantId,
                role: "assistant",
                text: "",
                model: args.modelLabel
                  ? { name: args.modelLabel, provenance: args.provenance ?? "cloud" }
                  : undefined,
                steps: [runningStep],
                pendingMedia: {
                  modality: args.modality,
                  aspectRatio: args.aspectRatio,
                  startedAt: Date.now(),
                },
                streaming: true,
                createdAt: Date.now() + 1,
              },
            ],
          }
        : c
    ),
    ...(args.clearImplicitReference ? { lastMediaArtifact: null } : {}),
  }));
  if (isFirstMessage) get().renameConversation(convId, deriveTitle(text));

  try {
    const job = await args.submit({ conversationId: convId, messageId: assistantId });
    set((s) => ({
      mediaJobs: { ...s.mediaJobs, [job.id]: { conversationId: convId, messageId: assistantId, stepId } },
    }));
    patchAssistant(set, convId, assistantId, {
      pendingMedia: {
        modality: args.modality,
        aspectRatio: args.aspectRatio,
        startedAt: job.started_at,
        jobId: job.id,
      },
    });
  } catch (e) {
    // The submit itself was refused — no backend, a bad model id, an empty
    // prompt. Nothing is running, so this turn ends here.
    patchAssistant(set, convId, assistantId, {
      text: `That didn't work: ${String(e)}`,
      streaming: false,
      pendingMedia: undefined,
      steps: [{ ...runningStep, status: "error", result: `— ${String(e)}` }],
    });
  } finally {
    set(() => ({ busy: false }));
  }
}

/** Patch fields on a specific assistant message inside a conversation. */
function patchAssistant(
  set: StoreSet,
  convId: string,
  msgId: string,
  patch: Partial<Message>
) {
  set((s) => ({
    conversations: s.conversations.map((c) =>
      c.id === convId
        ? { ...c, messages: c.messages.map((m) => (m.id === msgId ? { ...m, ...patch } : m)) }
        : c
    ),
  }));
}

/** Patch a block wherever it lives in a conversation's messages (Generative UI). */
function patchBlock(set: StoreSet, convId: string, blockId: string, patch: Partial<BlockView>) {
  set((s) => ({
    conversations: s.conversations.map((c) =>
      c.id === convId
        ? {
            ...c,
            messages: c.messages.map((m) =>
              m.blocks?.some((b) => b.id === blockId)
                ? { ...m, blocks: m.blocks.map((b) => (b.id === blockId ? { ...b, ...patch } : b)) }
                : m
            ),
          }
        : c
    ),
  }));
}

/** Ensure the local engine is running the given model; returns an error message
 * (to show as the assistant turn) or null when the turn may proceed. */
async function ensureEngineForModel(get: () => AppState, model: Model): Promise<string | null> {
  if (model.provenance !== "local") return null;
  const st = get();
  if (!st.engineReady || st.loadedModelId !== model.id) {
    try {
      await get().loadModelById(model.id);
    } catch (e) {
      return `I couldn't start the engine for “${model.name}”. ${String(e)}`;
    }
  }
  if (!get().engineReady) {
    return "No model is loaded yet. Open Models and choose a model to start the engine.";
  }
  return null;
}

/**
 * SEM-3: the live, per-turn replacement for the old wholesale-index inject.
 * Facts stay wholesale (`SCP` narrows that later); lessons and
 * lessons are retrieved by relevance to `query` when an embedder is ready,
 * and never shown twice — an entry that surfaces here is *removed* from
 * `index`, not merely repeated in both places.
 *
 * Same toolset-gating as `memoryForPrompt`: off ⇒ soul only, no retrieval
 * call at all. A failed `recall_for` (engine hiccup) falls back to the last
 * cached wholesale index rather than dropping memory from the turn.
 */
async function recallForPrompt(
  get: () => AppState,
  query: string
): Promise<{
  memory: api.MemoryContext | undefined;
  matches: api.SearchHit[];
  /** Fact names that actually reached the prompt this turn (WHY-2) — empty
   * whenever recall didn't run through the backend (toolset off, no Tauri, or
   * a failed call falling back to the cached wholesale index). */
  injectedFacts: string[];
}> {
  const state = get();
  const mc = state.memoryContext;
  if (!mc) return { memory: undefined, matches: [], injectedFacts: [] };
  if (!state.memoryToolEnabled) {
    return {
      memory: mc.soul.trim() || mc.about_you.trim() ? { ...mc, index: "" } : undefined,
      matches: [],
      injectedFacts: [],
    };
  }
  if (!api.inTauri()) return { memory: mc, matches: [], injectedFacts: [] };
  try {
    const { index, matches, injected_facts } = await api.recallFor(query);
    return { memory: { ...mc, index }, matches, injectedFacts: injected_facts };
  } catch {
    return { memory: mc, matches: [], injectedFacts: [] };
  }
}

/** WHY-2: the compact per-message record built from what this turn actually
 * used — persona id, soul presence, and the exact fact/lesson names
 * that reached the prompt (never the prompt text itself). */
function buildContextRefs(opts: {
  personaId: string | null;
  memory: api.MemoryContext | undefined;
  injectedFacts: string[];
  matches: api.SearchHit[];
}): api.ContextRefs {
  return {
    persona_id: opts.personaId,
    soul_present: !!opts.memory?.soul?.trim(),
    about_you_present: !!opts.memory?.about_you?.trim(),
    facts: opts.injectedFacts,
    lessons: opts.matches.filter((m) => m.kind === "lesson").map((m) => m.title),
    files: [],
  };
}

/** SEM-5: reuses the `recall` event's shape — a step the timeline renders
 * exactly like a `search_history` call — for memory that surfaced on its
 * own, before the turn even started. `null` when nothing was retrieved:
 * always-injected entries (soul, facts) never announce themselves. */
function recallStep(matches: api.SearchHit[]): AgentStep | null {
  if (!matches.length) return null;
  const target = matches.length === 1 ? matches[0].snippet : `${matches.length} things`;
  return {
    id: `recall-${Date.now()}`,
    verb: "remembered",
    target,
    status: "done",
    matches,
  };
}

// ---- context homeostasis (CTX-4) ----

async function resolveBudget(model: Model | undefined): Promise<number> {
  if (model?.provenance === "cloud") return CLOUD_CTX[model.provider ?? ""] ?? 32_000;
  // The endpoint's own context window (set by the user when they added it) —
  // `/v1/models` doesn't reliably report one, and `getContextBudget()` below
  // reports the *integrated* engine's window, which is a different server.
  if (model?.provenance === "endpoint") return model.ctxSize ?? 8192;
  if (!api.inTauri()) return DEFAULT_LOCAL_CTX;
  try {
    return (await api.getContextBudget()) ?? DEFAULT_LOCAL_CTX;
  } catch {
    return DEFAULT_LOCAL_CTX;
  }
}

/** True for a model that's routed like a cloud model — a hosted BYOK provider,
 * or a user's own connected server — as opposed to the integrated engine. */
function isRemoteModel(model: Model | undefined): model is Model {
  return model?.provenance === "cloud" || model?.provenance === "endpoint";
}

function targetFor(model: Model): api.ChatTarget {
  if (model.provenance === "cloud") {
    return { provenance: "cloud", provider: model.provider, model: model.cloudModel };
  }
  if (model.provenance === "endpoint") {
    return { provenance: "endpoint", provider: model.endpointId, model: model.cloudModel };
  }
  return { provenance: "local" };
}

/** Optimistic ids are minted client-side and mean nothing to the backend. */
export function isPersistedId(id: string): boolean {
  return !id.startsWith("u-") && !id.startsWith("a-");
}

/**
 * Build the turns for one request: system prompt + as much history as the
 * model's context window can hold + the current turn (CTX-4).
 *
 * When history overflows, the older part is summarized into the conversation
 * (once), and the summary rides along in the system prompt. Nothing is deleted
 * or hidden — this only decides what gets *sent*. Shared by `sendMessage` and
 * `sendBlockAction` so both paths budget identically.
 */
async function assembleTurns(
  set: StoreSet,
  get: () => AppState,
  opts: {
    convId: string;
    system: string;
    current: api.ChatTurnMessage;
    model: Model;
  }
): Promise<api.ChatTurnMessage[]> {
  const { convId, current, model } = opts;
  const conv = get().conversations.find((c) => c.id === convId);
  const keepRecent = conv?.workspace ? KEEP_RECENT_WORKSPACE : KEEP_RECENT;

  /** History after the summary boundary — the turns still sent verbatim. */
  const priorFrom = (boundaryId: string | null | undefined) => {
    const all = (conv?.messages ?? []).filter((m) => m.text.trim().length > 0);
    const cut = boundaryId ? all.findIndex((m) => m.id === boundaryId) : -1;
    return all.slice(cut + 1).map((m) => ({
      id: m.id,
      turn: { role: m.role as "user" | "assistant", content: m.text },
    }));
  };

  const budget = await resolveBudget(model);
  set(() => ({ contextBudget: budget }));

  let system = conv?.summary ? withSummary(opts.system, conv.summary) : opts.system;
  let prior = priorFrom(conv?.summaryUptoMessageId);
  let bt = budgetTurns(system, prior.map((p) => p.turn), current, budget, keepRecent);

  if (bt.needsCompaction && api.inTauri() && get().autoCompact) {
    // Overflow is the oldest prefix, so the boundary is its last message.
    const boundary = prior[bt.overflow.length - 1];
    if (boundary && isPersistedId(boundary.id)) {
      try {
        const summary = await api.compactConversation(convId, boundary.id, targetFor(model));
        set((s) => ({
          conversations: s.conversations.map((c) =>
            c.id === convId ? { ...c, summary, summaryUptoMessageId: boundary.id } : c
          ),
        }));
        system = withSummary(opts.system, summary);
        prior = priorFrom(boundary.id);
        bt = budgetTurns(system, prior.map((p) => p.turn), current, budget, keepRecent);
      } catch {
        // Summarizing failed (no engine, model error). Sending must never block:
        // fall through and let the oldest turns simply be dropped.
      }
    }
  }

  return bt.turns;
}

/** Run one assistant turn: stream events into the optimistic message, then
 * finalize. Shared by `sendMessage` and `sendBlockAction`. */
async function streamAssistantTurn(
  set: StoreSet,
  get: () => AppState,
  opts: {
    convId: string;
    assistantId: string;
    persistedAssistantId: string;
    turns: api.ChatTurnMessage[];
    model: Model;
    temperature?: number;
    /** A step already resolved before the turn started (SEM-5's ambient
     * recall) — shown immediately, not waiting for the first stream event. */
    initialSteps?: AgentStep[];
    /** WHY-2: what this turn's prompt was actually built from, stored on the
     * finalized message so it can be explained later. */
    contextRefs?: api.ContextRefs;
  }
): Promise<void> {
  const { convId, assistantId, persistedAssistantId, turns, model, temperature } = opts;
  let acc = "";
  const steps: AgentStep[] = [...(opts.initialSteps ?? [])];
  if (steps.length) patchAssistant(set, convId, assistantId, { steps: [...steps] });
  const blocks: BlockView[] = [];
  const proposalIds: string[] = [];
  const artifactIds: string[] = [];
  /** Media artifacts this turn produced, rendered inline (`STR-1`). */
  const mediaAttachments: Attachment[] = [];
  const fileChangeIds: string[] = [];
  try {
    await api.agentChat(
      convId,
      turns,
      (e) => {
        switch (e.type) {
          case "token":
            acc += e.text;
            patchAssistant(set, convId, assistantId, { text: acc, streaming: true });
            break;
          case "step_start":
            steps.push({ id: e.id, verb: e.verb, target: e.target, status: "running" });
            patchAssistant(set, convId, assistantId, { steps: [...steps] });
            break;
          case "step_done": {
            const s = steps.find((x) => x.id === e.id);
            if (s) {
              s.status = "done";
              s.result = e.result ?? undefined;
            }
            patchAssistant(set, convId, assistantId, { steps: [...steps] });
            break;
          }
          case "step_error": {
            const s = steps.find((x) => x.id === e.id);
            if (s) {
              s.status = "error";
              s.result = `— ${e.error}`;
            }
            patchAssistant(set, convId, assistantId, { steps: [...steps] });
            break;
          }
          case "artifact": {
            const artifact: api.Artifact = {
              id: e.id,
              conversation_id: convId,
              title: e.title,
              kind: e.kind,
              content: e.content,
              created_at: Date.now(),
              saved_path: null,
              meta_json: e.meta_json,
            };
            artifactIds.push(e.id);
            // Media is the deliberate exception (`ART-2`): it's already visible
            // inline in the stream, so auto-opening the viewer for it would be
            // redundant motion rather than the strong "look, it's ready" signal
            // it is for every other artifact kind.
            const isMedia = e.kind === "image" || e.kind === "video";
            // …and it is only already visible because the tool path attaches it
            // to the turn here. Without this the agent's own image showed as a
            // chip while the composer's showed as a picture — the two paths the
            // user must never be able to tell apart (`STR-1`).
            if (isMedia) mediaAttachments.push(mediaAttachmentFor(artifact));
            patchAssistant(set, convId, assistantId, {
              artifactIds: [...artifactIds],
              ...(isMedia ? { attachments: [...mediaAttachments] } : {}),
            });
            set((st) => {
              const existing = st.artifacts[convId] ?? [];
              return {
                artifacts: { ...st.artifacts, [convId]: [...existing, artifact] },
                dockOpen: isMedia ? st.dockOpen : true,
                selected: isMedia ? st.selected : { kind: "artifact" as const, id: e.id },
              };
            });
            break;
          }
          case "file_changed": {
            // The agent changed a real file. Mark it, refresh the branch it
            // lives in, and pull the undo row so "Recent changes" stays honest
            // about what just happened on disk.
            set((st) => ({
              touchedFiles: { ...st.touchedFiles, [e.path]: Date.now() },
            }));
            if (e.undo_token) {
              fileChangeIds.push(e.undo_token);
              patchAssistant(set, convId, assistantId, { fileChangeIds: [...fileChangeIds] });
            }
            get().refreshTree().catch(() => {});
            get().refreshTrash().catch(() => {});
            break;
          }
          case "browser": {
            // `BRW-UI-1`: the panel replaces its state wholesale — every
            // field can change on any one action.
            set((st) => ({
              browserSessions: { ...st.browserSessions, [convId]: e.state },
            }));
            break;
          }
          case "mail_sent":
            // `MAIL-3`: only the `auto` rung reaches here — accepting an
            // `email` proposal is announced by the card disappearing instead.
            set(() => ({
              mailSentToast: `✉ I sent it to ${e.to}. There's no unsending — tell me if that was wrong.`,
            }));
            break;
          case "block": {
            // The composed workspace surface streams through the same event but
            // lives in its own slice — never inside a chat message.
            if (e.kind === "surface") {
              set((st) => ({
                surfaces: {
                  ...st.surfaces,
                  [convId]: {
                    id: e.id,
                    kind: "surface",
                    title: e.title,
                    data: e.data,
                    state: st.surfaces[convId]?.state,
                    messageId: e.message_id ?? null,
                  },
                },
              }));
              break;
            }
            blocks.push({
              id: e.id,
              kind: e.kind as BlockView["kind"],
              title: e.title,
              data: e.data,
              messageId: e.message_id ?? assistantId,
            });
            patchAssistant(set, convId, assistantId, { blocks: [...blocks] });
            break;
          }
          case "block_update": {
            const surf = get().surfaces[convId];
            if (surf && surf.id === e.id) {
              set((st) => ({
                surfaces: { ...st.surfaces, [convId]: { ...surf, title: e.title, data: e.data } },
              }));
              break;
            }
            const b = blocks.find((x) => x.id === e.id);
            if (b) {
              b.title = e.title;
              b.data = e.data;
              patchAssistant(set, convId, assistantId, { blocks: [...blocks] });
            } else {
              // Updating a block from an earlier turn.
              patchBlock(set, convId, e.id, { title: e.title, data: e.data });
            }
            break;
          }
          case "state_update":
            set((st) => ({
              sessionState: {
                ...st.sessionState,
                [convId]: (e.state as Record<string, unknown>) ?? {},
              },
            }));
            break;
          case "permission":
            set((st) => ({ pendingPermissions: [...st.pendingPermissions, e.request] }));
            break;
          case "memory_write": {
            // The self changed mid-turn: refresh what gets injected, and show
            // the user what was written with a way to take it back.
            get().refreshMemoryContext();
            if (e.op !== "read") {
              set(() => ({
                memoryToast: {
                  op: e.op,
                  name: e.name,
                  description: e.description,
                  collection: e.collection,
                  undoToken: e.undo_token,
                },
              }));
              // SMP-2b: a genuine memory write is the other first-need
              // moment for the recall helper.
              get().maybeOfferRecall();
              // PRO-4: a fact just changed — worth reconsidering the synthesis.
              if (e.collection === "facts") get().noteGlobalFactChange();
            }
            break;
          }
          case "proposal":
            // Hang it off this turn so the user meets the suggestion where it
            // was made, not only later in Settings (SOUL-UI-2).
            proposalIds.push(e.id);
            patchAssistant(set, convId, assistantId, { proposalIds: [...proposalIds] });
            get().refreshChangeProposals();
            break;
          case "recall": {
            // Hang the provenance off the step the search is running under, so
            // the timeline row can expand into clickable sources (RCL-UI).
            const s = steps.find((x) => x.id === e.id);
            if (s) s.matches = e.matches;
            patchAssistant(set, convId, assistantId, { steps: [...steps] });
            // SMP-7b: explain the ability the first time it actually surfaces
            // something — `search_folder` and cross-conversation recall share
            // this event, distinguished by the hits' own `source`.
            if (e.matches.length > 0) {
              if (e.matches.every((m) => m.source === "file")) {
                get().maybeFirstTime(
                  "retrieval",
                  "That came from your files — the names under my answer show which."
                );
              } else {
                get().maybeFirstTime(
                  "recall",
                  "I brought that up because I remembered it from an earlier chat."
                );
              }
            }
            break;
          }
          case "code": {
            // `DAT-UI-1`: same idea as "recall" — the snippet hangs off its
            // step so the `⌄` disclosure can reveal it on demand.
            const s = steps.find((x) => x.id === e.id);
            if (s) s.code = { language: e.language, code: e.code };
            patchAssistant(set, convId, assistantId, { steps: [...steps] });
            break;
          }
          case "untrusted": {
            // `TRU-UI-1`: a step can wrap more than one source (several
            // retrieved file excerpts in one `search_folder` call), so this
            // accumulates rather than replaces.
            const s = steps.find((x) => x.id === e.id);
            if (s) {
              s.untrusted = [
                ...(s.untrusted ?? []),
                { label: e.label, risk: e.risk, flags: e.flags, text: e.text },
              ];
            }
            patchAssistant(set, convId, assistantId, { steps: [...steps] });
            break;
          }
          case "done":
          case "cancelled":
            patchAssistant(set, convId, assistantId, { streaming: false });
            break;
          case "error":
            acc = acc || `That didn't work: ${e.message}`;
            patchAssistant(set, convId, assistantId, { text: acc, streaming: false });
            break;
        }
      },
      {
        toolsEnabled: get().toolsEnabled,
        temperature,
        assistantMessageId: persistedAssistantId,
        target: targetFor(model),
      }
    );
  } catch (err) {
    acc = acc || `That didn't work: ${String(err)}`;
    patchAssistant(set, convId, assistantId, { text: acc, streaming: false });
  } finally {
    // Back to resting unless a self-process is still working (PRES-1).
    set((st) => ({ busy: false, presence: st.reflectingIds.length ? "reflecting" : "idle" }));
    try {
      const stepsJson = steps.length ? JSON.stringify(steps) : undefined;
      const contextJson = opts.contextRefs ? JSON.stringify(opts.contextRefs) : undefined;
      await api.finalizeMessage(persistedAssistantId, acc, stepsJson, contextJson);
      // The message still carries its optimistic client id until this turn
      // finalizes — swap in the real one so "why this answer?" (WHY-4) can
      // address it this session, not only after the next reload.
      if (persistedAssistantId !== assistantId) {
        patchAssistant(set, convId, assistantId, { id: persistedAssistantId });
      }
    } catch {
      /* ignore */
    }
    // This turn just wrote its tool outcomes (GRM-4). Re-reading them here is
    // what lets a tool that starts failing earn its caution during the session
    // it is failing in, rather than after the next restart (HEAL-2).
    if (steps.length) useAppStore.getState().refreshToolHealth();
  }
}

/** PRO-4's daily tick: attempt an automatic rebuild at most once per calendar
 * date. Fire-and-forget from `bootstrap`; `maybeAutoRebuildProfile` already
 * covers every "decided not to" case silently, so this is safe to call on
 * every launch once that date has turned over. */
async function maybeDailyProfileTick(get: () => AppState) {
  if (!api.inTauri()) return;
  const today = new Date().toISOString().slice(0, 10);
  try {
    const last = await api.getSetting(PROFILE_CHECKED_KEY);
    if (last === today) return;
    await api.setSetting(PROFILE_CHECKED_KEY, today);
    await get().maybeAutoRebuildProfile();
  } catch {
    /* try again next launch */
  }
}

/** The selected remote model (cloud, or a user's own connected server) shaped
 * as a routing target — `undefined` means the local engine. Reflection,
 * consolidation and `GLD-2`'s before/after checks all route this way: a
 * cloud-only or endpoint-only setup would otherwise go unchecked, since
 * there is no local engine for the guard to fall back to. */
export function cloudTarget(): api.ChatTarget | undefined {
  const s = useAppStore.getState();
  const model = s.models.find((m) => m.id === s.selectedModelId);
  return isRemoteModel(model) ? targetFor(model) : undefined;
}

/** Subscribe to the self-maintenance processes that run outside a chat stream
 * (REF-3, HEAL-1). Registered once, at bootstrap. */
function listenForSelfEvents(set: StoreSet, get: () => AppState) {
  // `JOB-1`: a generation finishing has no chat stream to arrive on — the run
  // that asked for it has usually ended by then. This is how the picture gets
  // back into its turn.
  api.onAppEvent<api.MediaJobEvent>("poiesis-media-job", (e) => {
    get().applyMediaJobEvent(e);
  });
  // `STR-4`: successive partials fill the placeholder in. Kept in their own
  // slice so a frame arriving every few hundred ms doesn't re-render the
  // whole transcript.
  api.onAppEvent<api.MediaPartialEvent>("poiesis-media-partial", (e) => {
    set((s) => (s.mediaJobs[e.job_id] ? { mediaPartials: { ...s.mediaPartials, [e.job_id]: e.data_uri } } : {}));
  });
  api.onAppEvent<api.MemoryWriteEvent>("poiesis-memory-write", (e) => {
    get().refreshMemoryContext();
    get().refreshSelf();
    set(() => ({
      memoryToast: {
        op: e.op,
        name: e.name,
        description: e.description,
        collection: e.collection,
        undoToken: e.undo_token,
      },
    }));
    // PRO-4: reflection can write facts outside a live turn too.
    if (e.collection === "facts") get().noteGlobalFactChange();
  });
  api.onAppEvent<api.HealedEvent>("poiesis-healed", (e) => {
    set(() => ({
      presence: "healing" as const,
      healToast: e.ok
        ? "↻ My engine stalled — I restarted it."
        : "↻ I couldn't keep my engine alive — I've stopped trying. Check the Engine page.",
    }));
    // The healing state is a moment, not a mode.
    setTimeout(() => {
      set((s) => (s.presence === "healing" ? { presence: "idle" } : {}));
    }, 3000);
  });
  // SCH-UI-4: the Rail's running-job row and Stop button have no other way
  // to learn a job started or ended — a scheduled run isn't invoked from the
  // UI, so there's no channel open to stream it.
  api.onAppEvent<api.JobStartedEvent>("poiesis-job-started", () => {
    get().refreshScheduler();
  });
  api.onAppEvent<api.JobFinishedEvent>("poiesis-job-finished", () => {
    get().refreshScheduler();
  });
  // TTL-2: short-lived facts let go, at startup or overnight.
  api.onAppEvent<api.ExpirySweptEvent>("poiesis-expiry-swept", (e) => {
    set(() => ({
      expirySweptToast: `I let ${e.count} short-lived note${e.count === 1 ? "" : "s"} go.`,
    }));
    get().refreshMemoryContext();
  });
  // BRW-1: a browsing session timed out on its own. Nothing is streaming by
  // then, so the panel can only learn it from an app event.
  api.onAppEvent<api.BrowserClosedEvent>("poiesis-browser-closed", (e) => {
    set((s) => {
      const open = s.browserSessions[e.conversationId];
      if (!open || open.closed) return {};
      return {
        browserSessions: { ...s.browserSessions, [e.conversationId]: { ...open, closed: true } },
      };
    });
  });
  // GLD-2: a self-change was checked and found to make things worse.
  api.onAppEvent<api.GoldenRevertedEvent>("poiesis-golden-reverted", (e) => {
    set(() => ({
      goldenRevertedToast: `That change made me worse at ${e.count} thing${e.count === 1 ? "" : "s"} — I put it back.`,
    }));
    get().refreshSelf();
  });
}

// ---- session state helpers (Generative UI, Phase C) ----

/** PRO-6: the agent's own synthesis of how this user likes to be talked to —
 * placed first among the memory blocks since it changes slowest of all of
 * them (a rebuild is debounced and gated), which is what keeps the prefix
 * cache warm turn to turn. PRO-7: unlike SOUL.md, this is a background
 * inference, not something the user just decided — a persona always wins. */
function aboutYouBlock(text: string | undefined): string {
  const t = text?.trim();
  if (!t) return "";
  return `## About you, as I understand it (apply it; don't mention it unless asked; the persona/system prompt above always wins if they conflict)\n${t}`;
}

/** Standing instructions, framed so the model knows they outrank the persona
 * prompt above them when the two pull in different directions (SOUL constrains,
 * persona styles — persona still governs voice/format/depth). */
function soulBlock(soul: string | undefined): string {
  const s = soul?.trim();
  if (!s) return "";
  return `## Standing instructions (SOUL.md — the user approved these; they take precedence over the persona/system prompt above when the two conflict)\n${s}`;
}

/** The durable memory index, with a caveat when tools (and so `memory` reads) are off. */
function memoryIndexBlock(index: string | undefined, toolsEnabled: boolean): string {
  const i = index?.trim();
  if (!i) return "";
  const detail = toolsEnabled
    ? ""
    : " Tools are off — treat descriptions as the only available detail.";
  return `## Your notes about the user (durable facts)\n${i}\n(Read a note's full text with memory(op:"read", name:…) before relying on its details.${detail})`;
}

/** A compact rendering of durable session state. */
function sessionStateBlock(state: Record<string, unknown> | undefined): string {
  if (!state || Object.keys(state).length === 0) return "";
  return `## Session state (durable; update with the remember tool)\n${JSON.stringify(state)}`;
}

/** The standing guidance only sent when the model can actually call tools. */
function toolGuidanceBlock(): string {
  return `${SURFACE_GUIDANCE}\n\n${BLOCK_GUIDANCE}\n\n${PLAN_FIRST_GUIDANCE}`;
}

export interface ComposePromptOpts {
  conv: Conversation | undefined;
  sessionState: Record<string, unknown> | undefined;
  toolsEnabled: boolean;
  surface?: BlockView;
  /** The durable self (MEM-3). Omitted when the Memory toolset is off. */
  memory?: api.MemoryContext;
  /** 7-day tool reliability for this model (HEAL-2). */
  toolHealth?: api.ToolHealth[];
  /** Discovered Agent Skills (SKL-2), for the "Skills available" block. */
  skills?: api.SkillView[];
}

/** Assemble the full system prompt for a turn: base persona/prompt, then the
 * live workspace-block registry (W3), durable session state, and the
 * block-usage guidance (W4/W5). Kept in one place so `sendMessage` and
 * `sendBlockAction` build identical context. */
/** Exported for `store.compose-system-prompt.test.ts` — this is otherwise an
 * internal helper used only by `sendMessage`/`sendBlockAction`. */
export function composeSystemPrompt(base: string, opts: ComposePromptOpts): string {
  let out = base;
  // The durable self comes first, right after the base prompt. The synthesis
  // leads (PRO-6) — it's the slowest-changing of these blocks — then standing
  // instructions the user approved, then the index of what's remembered.
  const aboutYouText = aboutYouBlock(opts.memory?.about_you);
  if (aboutYouText) out += `\n\n${aboutYouText}`;
  const soulText = soulBlock(opts.memory?.soul);
  if (soulText) out += `\n\n${soulText}`;
  const indexText = memoryIndexBlock(opts.memory?.index, opts.toolsEnabled);
  if (indexText) out += `\n\n${indexText}`;
  // Only mention blocks/surface machinery when the model can actually call the
  // tools — otherwise it imitates tool-call JSON as prose and it leaks raw.
  if (opts.toolsEnabled) {
    const skillsText = skillsBlock(opts.skills);
    if (skillsText) out += `\n\n${skillsText}`;
    const registry = blockRegistry(opts.conv);
    if (registry) out += `\n\n${registry}`;
    const surface = surfaceContext(opts.surface);
    if (surface) out += `\n\n${surface}`;
  }
  const sessionText = sessionStateBlock(opts.sessionState);
  if (sessionText) out += `\n\n${sessionText}`;
  if (opts.toolsEnabled) {
    out += `\n\n${toolGuidanceBlock()}`;
    const cautions = toolCautions(opts.toolHealth);
    if (cautions) out += `\n\n${cautions}`;
  }
  return out;
}

/** Per-entry cap (description + when_to_use combined) and whole-block cap for
 * the `SKL-2` stage-1 disclosure — matches the Agent Skills standard's own
 * numbers, so a skill written for another agent isn't truncated differently
 * here than it would be there. */
export const SKILL_ENTRY_CAP = 1536;
export const SKILLS_BLOCK_CAP = 4000;

/** SKL-2 stage 1: name + description of every *enabled* skill, so the model
 * knows what exists before spending a turn on `skill` to read one. Lowest
 * priority (last to fit) is whichever skill sorts last — a full priority
 * ranking by source isn't worth the complexity at the skill counts this is
 * ever exercised at. */
/** `SKL-6`: narrow the advertised list to a persona's allowlist, mirroring the
 * backend's `skillpack::enabled_names_for_persona`. Without this the model is
 * told about skills the `skill` tool will then refuse — it would burn a turn
 * to learn what the prompt could have said. A persona can narrow, never widen:
 * the global `enabled` flag is still what `skillsBlock` filters on. */
function skillsForPersona(
  skills: api.SkillView[],
  skillsJson: string | null | undefined
): api.SkillView[] {
  if (!skillsJson) return skills;
  try {
    const allow = JSON.parse(skillsJson) as string[];
    if (!Array.isArray(allow)) return skills;
    return skills.filter((s) => allow.includes(s.name));
  } catch {
    return skills;
  }
}

function skillsBlock(skills: api.SkillView[] | undefined): string {
  const enabled = (skills ?? []).filter((s) => s.enabled);
  if (!enabled.length) return "";
  const header = "Skills available (read one with the `skill` tool before doing the work it covers):";
  const lines: string[] = [header];
  let used = header.length;
  let shown = 0;
  for (const s of enabled) {
    const desc = [s.description, s.when_to_use].filter(Boolean).join(" — ");
    const clipped = desc.length > SKILL_ENTRY_CAP ? `${desc.slice(0, SKILL_ENTRY_CAP)}…` : desc;
    const line = `- ${s.name}: ${clipped}`;
    if (used + line.length + 1 > SKILLS_BLOCK_CAP) break;
    lines.push(line);
    used += line.length + 1;
    shown += 1;
  }
  const remaining = enabled.length - shown;
  if (remaining > 0) lines.push(`(+${remaining} more)`);
  return lines.join("\n");
}

/** HEAL-2: tell the agent which of its own tools have been failing lately, so
 * it can route around the damage. Informational self-repair — it changes only
 * this prompt, stores nothing, and needs no setting. Worst two tools only:
 * a wall of cautions would just teach the model to distrust every tool. */
export function toolCautions(health: api.ToolHealth[] | undefined): string {
  if (!health?.length) return "";
  const failing = health
    .filter((t) => t.total >= 8 && t.ok / t.total < 0.4)
    .sort((a, b) => a.ok / a.total - b.ok / b.total)
    .slice(0, 2);
  return failing
    .map(
      (t) =>
        `Note: your "${t.tool_name}" tool has failed often recently — double-check its arguments, and prefer an alternative when one exists.`
    )
    .join("\n");
}

/** The current composed surface, injected so the model can revise it by
 * node_id instead of re-rendering blind. Capped so a huge tree can't flood
 * the context — past the cap the model should just re-render whole regions. */
function surfaceContext(surface: BlockView | undefined): string {
  if (!surface) return "";
  let tree = JSON.stringify(surface.data);
  if (tree.length > 4000) tree = `${tree.slice(0, 4000)}…(truncated — re-render regions you need to change)`;
  const bound =
    surface.state && Object.keys(surface.state as Record<string, unknown>).length
      ? `\nUser's bound state (from inputs/choices/toggles): ${JSON.stringify(surface.state)}`
      : "";
  return `## Workspace surface (the live interface you composed with render_ui)\nCurrent tree: ${tree}${bound}`;
}

/** Teach the model that the Workspace is a composable surface it owns. */
const SURFACE_GUIDANCE = [
  "## Composing the workspace",
  "The Workspace view renders whatever interface tree you pass to `render_ui` — compose a real interface for the task (a dashboard, a board, a picker, a wizard, a tracker) instead of describing things in prose or emitting fixed chat blocks.",
  "Keep the surface CURRENT: as the task evolves, revise it (render_ui with node_id for one region, or re-render the whole tree) rather than accumulating chat.",
  "When a `ui_action` message arrives, it carries the user's bound state — revise the surface to reflect the interaction and reply in at most one sentence.",
  // ORG-UI-2: the data is already in this prompt (notes, lessons)
  // and render_ui already renders — so "show me what you've learned" becomes
  // the organism examining itself in its own body, with no new machinery.
  "If the user asks how you are, what you remember, or what you've learned, you may render your notes and lessons as a workspace surface.",
].join("\n");

/** W4/W5: teach the model to treat blocks as the surface, not to narrate them,
 * and to acknowledge bare interactions briefly. Only added when tools are on. */
const BLOCK_GUIDANCE = [
  "## Presenting blocks",
  "When you present a block (comparison, plan, collection, form, progress, document), the user sees it rendered in full in their workspace. Do NOT restate the block's contents in prose — after presenting, conclude in at most two sentences.",
  "To change a block that already exists, call `present` with that block's existing `block_id` (see the workspace-block list above) rather than creating a new one.",
  "If the user's message is only a block interaction (a workspace update, or a `poiesis-action`), acknowledge it in one short sentence and do not present a menu of follow-up options.",
].join("\n");

/** LOOP-3: a multi-step run reads as deliberate rather than flailing when the
 * model says what it intends before the first tool call. One line, tools only. */
const PLAN_FIRST_GUIDANCE = [
  "## Working through a task",
  "For multi-step tasks, state a one-line plan before your first tool call.",
].join("\n");

/** W3: a compact registry of the blocks already on the user's workspace, so the
 * model can update them by id instead of recreating (the duplicate-block bug). */
function blockRegistry(conv: Conversation | undefined): string {
  if (!conv) return "";
  const blocks = conv.messages.flatMap((m) => m.blocks ?? []);
  if (!blocks.length) return "";
  const lines = blocks.map((b) => {
    const summary = blockSummary(b);
    return `[${b.id}] "${b.title}" (${b.kind}${summary ? `, ${summary}` : ""})`;
  });
  return `## Workspace blocks (already visible to the user — update these by passing their block_id to present, do not recreate)\n${lines.join("\n")}`;
}

function blockSummary(b: BlockView): string {
  const data = b.data && typeof b.data === "object" ? (b.data as Record<string, unknown>) : {};
  const arr = (x: unknown) => (Array.isArray(x) ? (x as Record<string, unknown>[]) : []);
  if (b.kind === "plan") {
    const steps = arr(data.steps);
    const state = b.state && typeof b.state === "object" ? (b.state as Record<string, unknown>) : {};
    const checked =
      state.checked && typeof state.checked === "object"
        ? (state.checked as Record<string, unknown>)
        : {};
    const done = steps.filter((s) => (checked[String(s.id)] ?? s.status) === "done").length;
    return `${done}/${steps.length} done`;
  }
  if (b.kind === "comparison") return `${arr(data.options).length} options`;
  if (b.kind === "collection") return `${arr(data.items).length} items`;
  if (b.kind === "form") return `${arr(data.fields).length} fields`;
  return "";
}

/** Apply a JSON merge patch to a conversation's session state and persist it. */
function applySessionPatch(
  set: StoreSet,
  get: () => AppState,
  convId: string,
  patch: Record<string, unknown>
) {
  const current = get().sessionState[convId] ?? {};
  const merged = mergePatch(current, patch);
  set((s) => ({ sessionState: { ...s.sessionState, [convId]: merged } }));
  if (api.inTauri()) {
    api.setSessionState(convId, JSON.stringify(merged)).catch(() => {});
  }
}

/** RFC 7386-style merge patch, mirroring the backend `merge_patch`. */
function mergePatch(
  target: Record<string, unknown>,
  patch: Record<string, unknown>
): Record<string, unknown> {
  const out: Record<string, unknown> = { ...target };
  for (const [k, v] of Object.entries(patch)) {
    if (v === null) {
      delete out[k];
    } else if (
      typeof v === "object" &&
      !Array.isArray(v) &&
      typeof out[k] === "object" &&
      out[k] !== null &&
      !Array.isArray(out[k])
    ) {
      out[k] = mergePatch(out[k] as Record<string, unknown>, v as Record<string, unknown>);
    } else {
      out[k] = v;
    }
  }
  return out;
}

/** Derive a deterministic session-state patch from a block interaction, so pins
 * and form submissions are remembered without a model round-trip. */
function autoPatchForAction(
  conv: Conversation | undefined,
  blockId: string,
  payload: Record<string, unknown>
): Record<string, unknown> | null {
  const action = payload.action as string | undefined;
  const title = findBlockTitle(conv, blockId) ?? "selection";
  if (action === "pin") {
    const label = (payload.label as string) ?? (payload.option as string) ?? "";
    return { decisions: { [title]: label } };
  }
  if (action === "select") {
    const label = (payload.title as string) ?? (payload.item as string) ?? "";
    return { decisions: { [title]: label } };
  }
  if (action === "submit" && payload.data && typeof payload.data === "object") {
    return { constraints: payload.data as Record<string, unknown> };
  }
  return null;
}

function findBlockTitle(conv: Conversation | undefined, blockId: string): string | undefined {
  if (!conv) return undefined;
  for (const m of conv.messages) {
    const b = m.blocks?.find((x) => x.id === blockId);
    if (b) return b.title;
  }
  return undefined;
}

const NO_MODEL: Model = { id: "__none__", name: "No model yet", provenance: "local", available: false };

export function useSelectedModel(): Model {
  return useAppStore(
    (s) => s.models.find((m) => m.id === s.selectedModelId) ?? s.models[0] ?? NO_MODEL
  );
}

export function useActiveConversation(): Conversation | null {
  return useAppStore((s) => s.conversations.find((c) => c.id === s.activeConversationId) ?? null);
}

/** SMP-1b: expert-only surfaces render `null` when this is false — no
 * greying out, no "upgrade to see" affordance. */
export function useExpert(): boolean {
  return useAppStore((s) => s.expert);
}
