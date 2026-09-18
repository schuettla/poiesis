import { useMemo } from "react";
import { create } from "zustand";
import type {
  AgentStep,
  Attachment,
  BlockView,
  Conversation,
  DockView,
  FolderTrust,
  ItemRef,
  Message,
  Mode,
  Model,
  ModelFilter,
  Project,
  RuntimeTab,
  Provenance,
  SubRun,
  View,
  WorkbenchSelection,
} from "./types";
import { NEW_PROJECT_NAME } from "./types";
import { mockConversations, mockModels } from "./mockData";
import * as api from "./api";
import { budgetTurns, withSummary, KEEP_RECENT, KEEP_RECENT_WORKSPACE } from "./context";
import {
  EMPTY_PREFS,
  PREF_KEYS,
  moveFavorite,
  normalizePrefs,
  parseIdList,
  parseLabels,
  prefsChanged,
  reorderFavorites,
  resolveChatModel,
  setDefault,
  skippedNotice,
  toggleFavorite,
  type FavoriteTab,
  type ModelPrefs,
} from "./modelPrefs";

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
/** `SHL-17`: the header strip's open tabs, one set for the whole app. */
const TAB_SET_KEY = "shell.tabs";
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
  /** The Ctrl+K palette: chats (titles and message text), projects, library
   * and commands in one place. The Rail's Search row opens it too. */
  paletteOpen: boolean;
  setPaletteOpen: (open: boolean) => void;

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

  /** `MOD-3`/`MOD-4`: defaults across sources and the ordered favorites. */
  modelPrefs: ModelPrefs;
  modelPrefsLoaded: boolean;
  /** False until every model source has answered once at startup. Until
   * then the selection keeps re-resolving to the default, so a cloud default
   * isn't lost to a local model just because the library answered first. */
  selectionSettled: boolean;
  /** The composer's one-time line when the default couldn't be used. */
  modelNotice: string | null;
  modelNoticeFor: string | null;
  dismissModelNotice: () => void;
  loadModelPrefs: () => Promise<void>;
  settleSelection: () => void;
  /** `fallback` describes a model that isn't in `models` (a local image file
   * that isn't the active checkpoint yet). */
  setDefaultModelPref: (id: string, fallback?: Model) => Promise<void>;
  /** `false` when refused (the default can't be unstarred). */
  toggleFavoriteModel: (id: string, fallback?: Model) => boolean;
  moveFavoriteModel: (tab: FavoriteTab, id: string, delta: -1 | 1) => void;
  reorderFavoriteModels: (tab: FavoriteTab, ids: string[]) => void;

  /** Deep links between Models, Providers and Runtime. `modelsSource` is a
   * source chip on Models (`MOD-6`): "local", "endpoint:<id>" or
   * "cloud:<provider>". */
  modelsSource: { source: string; label: string } | null;
  setModelsSource: (f: { source: string; label: string } | null) => void;
  openModelsFiltered: (f: { source: string; label: string }) => void;
  runtimeTab: RuntimeTab | null;
  openRuntime: (tab?: RuntimeTab) => void;
  providerFocus: string | null;
  openProviders: (id?: string) => void;

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
  /** `HRN-1`: the run working right now, if any. Its id is what Stop and
   * mid-run steering address. Cleared when the run ends, however it ends. */
  activeRun: {
    runId: string;
    convId: string;
    /** 1-based iteration, and the budget it is counted against (`HRN-UI-3`). */
    step: number;
    maxSteps: number;
    startedAt: number;
    /** `OBS-3`: an estimate of what the current turn is sending, and what the
     * model can hold. `contextWindow` is null when the provider does not say,
     * and the meter then shows no percentage rather than a made-up one. */
    contextTokens: number;
    contextWindow: number | null;
    /** What the model has been thinking since the last thing it said out loud.
     * Cleared when prose starts arriving, so the indicator describes now and
     * not the whole run. Shown as thinking, never as the answer. */
    thinking: string;
    /** `PLN-UI-2`: the plan this run is working to, once it has written one.
     * The running item is a better label for the meter than the running tool,
     * because it says what the work is *for*. */
    plan?: api.PlanView;
  } | null;
  /** `HRN-UI-1`: say something to the run that is already working. Returns
   * false when there was no live run to say it to. */
  steerActiveRun: (text: string) => Promise<boolean>;
  /** `SUB-UI-1`: every delegated child this session knows about, by run id.
   * Live children are folded in from the stream; finished ones are rehydrated
   * from `subagent_runs` when a conversation opens. */
  subRuns: Record<string, SubRun>;
  /** Load the children a conversation started, so they survive a reload. */
  loadSubRuns: (convId: string) => Promise<void>;
  /** `SUB-UI-1`: say something to a child that is still working. */
  steerSubRun: (runId: string, text: string) => Promise<boolean>;
  /** `SUB-7`: stop a child, keeping what it already has. */
  stopSubRun: (runId: string) => Promise<void>;
  /** `SUB-UI-4`: which agent asked for a pending permission, by request id.
   * Absent means the lead asked. */
  permissionAgents: Record<string, string>;
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
    /** `SUB-3`: when to use this agent, and whether it may be handed work. */
    description?: string | null;
    spawnable?: boolean;
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
  /** `PLN-3`/`PLN-UI-4`: whether a run is told to plan the work first. */
  planMode: PlanMode;
  setPlanMode: (mode: PlanMode) => Promise<void>;
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
  /** `SUB-12`: a background agent finished after the turn that started it had
   * already ended. Announcement only — the report is in the Fleet card. */
  agentDoneToast: string | null;
  dismissAgentDoneToast: () => void;
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
  /** `SHL-24`: shows a conversation — the Rail's "select a chat" path. A chat
   * is a destination, not a tab: one is live at a time, and picking another
   * goes there rather than adding to a list. Any item open over the sidebar
   * loses focus, since what you pressed was the conversation. */
  openSession: (id: string) => Promise<void>;
  /** `HRN-UI-5`: branch this chat just before one assistant turn and ask the
   * question again, leaving the original exactly as it was. */
  forkFromMessage: (messageId: string) => Promise<void>;
  /** `HRN-UI-5`: continue the last run instead of starting over, with its tool
   * results intact. */
  resumeLastRun: () => Promise<void>;
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
  /** What the viewer is showing, file or artifact. Mirrors whichever entry in
   * `itemTabs` is active (`SHL-10`/`SHL-22`) — every existing reader of
   * `selected` (`Viewer`, `Tree`, `Artifacts`) keeps working unchanged. */
  selected: WorkbenchSelection | null;
  /** Opens a file or artifact and focuses it. Kept as the one call every
   * existing site (`Tree`, `Artifacts`, timeline chips, "recent changes")
   * already uses; it opens an item tab. Passing `null` closes whichever item
   * tab is active. */
  selectNode: (selection: WorkbenchSelection | null) => void;
  /** Open a specific artifact as a tab — from a timeline chip or a document
   * block. */
  openArtifact: (artifactId: string) => void;
  /** `SHL-24`/`SHL-27`: single things picked out of the right sidebar — a
   * file, an artifact, one child agent, one patch. They open as tabs in the
   * header and fill the conversation's own column, which is the widest surface
   * the shell has. The conversation is the first tab in that strip, so it is
   * never more than a click away and never has to share the width. */
  itemTabs: ItemRef[];
  /** The key (`"<kind>:<id>"`) of the focused item tab, or `null` when the
   * conversation itself is what's showing. */
  activeItemId: string | null;
  /** Opens (or focuses) an item tab. Only ever called for something the user
   * did — the agent moves `dockView`, never a tab (`SHL-23`). */
  openItem: (ref: ItemRef) => void;
  /** Closes an item tab, by its key. Closing the active one focuses its
   * neighbour; with none left, the sidebar's overview shows again. */
  closeItem: (id: string) => void;
  /** `EDT-1`: absolute paths of file tabs whose editor holds edits that are
   * not on disk. Only the flag lives here — the text stays in the Monaco
   * model, which is the only copy that can be edited — because this is state
   * *other* components need: the strip draws a dot from it, and `closeItem`
   * refuses to throw a buffer away without asking. */
  unsavedFiles: Record<string, true>;
  setUnsaved: (path: string, unsaved: boolean) => void;
  /** Which overview the right sidebar shows (`SHL-21`). Persisted with the
   * tab set. A value the current chat cannot show (Files with no folder)
   * falls back in the dock itself rather than being rewritten here. */
  dockView: DockView;
  /** `SHL-23`: the agent may move this, and it is all the agent may move. It
   * names a section of the right sidebar, and nothing in the sidebar can
   * change what the main column is showing — the trust rule is a fact of the
   * layout rather than something this setter has to enforce. */
  setDockView: (view: DockView) => void;
  /** Dock width in px, set by dragging its edge. Persisted across restarts. */
  dockWidth: number;
  setDockWidth: (px: number) => void;
  /** True while the divider is being dragged, so the shell drops its easing. */
  dockDragging: boolean;
  setDockDragging: (dragging: boolean) => void;
  /** `SHL-27`: show the conversation again without closing anything — what the
   * session tab does. Distinct from `closeItem`: the tabs stay, and coming
   * back to one costs a click rather than reopening it. */
  showConversation: () => void;
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

  /** `PRJ-1`: every project that is not archived, newest activity first. */
  projects: Project[];
  refreshProjects: () => Promise<void>;
  /** `PRJ-UI-4`: which project the `project` route is showing. */
  activeProjectId: string | null;
  /** `PRJ-3` explicit create: a folderless project, opened in its own view so
   * the first thing you do is name it and say what it is (`PRJ-UI-1a`). */
  newProject: () => Promise<void>;
  /** Open a project: focus its most recent session, or start one. */
  openProject: (projectId: string) => Promise<void>;
  /** `PRJ-UI-4`: open the project's own view as a route tab. */
  openProjectView: (projectId: string) => void;
  renameProject: (projectId: string, name: string) => Promise<void>;
  /** `PRJ-7`: the instructions every session in this project carries. */
  setProjectInstructions: (projectId: string, instructions: string) => Promise<void>;
  /** `PRJ-3a`: give the project a working folder, or with `null` take it away.
   * Removing it leaves the project and every one of its sessions in place. */
  setProjectFolder: (projectId: string, pick: boolean) => Promise<void>;
  /** `PRJ-9`: put a session in a project, or with `null` take it out. */
  moveSessionToProject: (conversationId: string, projectId: string | null) => Promise<void>;
  /** Hide the project and its sessions. Nothing on disk is touched. */
  archiveProject: (projectId: string) => Promise<void>;
  /** `PRJ-UI-1`: which project rows are expanded to show their sessions. */
  expandedProjects: string[];
  toggleProjectExpanded: (projectId: string) => void;

  attachFolder: () => Promise<void>;
  detachFolder: () => Promise<void>;
  setFolderTrust: (trust: FolderTrust) => Promise<void>;
  /** Reload one directory's children, or the whole tree when omitted. */
  refreshTree: (path?: string) => Promise<void>;
  refreshTrash: () => Promise<void>;
  undoFileOp: (id: string) => Promise<void>;
  /** `PRJ-UI-3`: each conversation's change set — the files the agent changed
   * since the last "Keep all", as patches. Keyed by conversation, because the
   * strip is global and a file tab's dot reads its own chat's set. */
  changeSets: Record<string, api.ChangeSet>;
  refreshChanges: (conversationId?: string) => Promise<void>;
  undoChanges: (conversationId: string, path?: string) => Promise<void>;
  keepChanges: (conversationId: string) => Promise<void>;
  /** The file the Changes sub-view should scroll to, set by a tab's dirty dot. */
  changesFocus: string | null;
  focusChange: (conversationId: string, path: string) => void;
  /** `COD-UI-1`: each project's detected card and execution settings. */
  projectCards: Record<string, api.ProjectCardView | null>;
  refreshProjectCard: (projectId: string, redetect?: boolean) => Promise<void>;
  setProjectExecPolicy: (projectId: string, policy: api.ExecPolicy | "inherit") => Promise<void>;
  setProjectTaskAllowed: (projectId: string, task: string, allowed: boolean) => Promise<void>;
  setProjectCommands: (projectId: string, runCommand: boolean, forget?: string) => Promise<void>;
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
    promptPerMtok: m.prompt_per_mtok ?? undefined,
    outputPerMtok: m.output_per_mtok ?? undefined,
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
  s: Pick<AppState, "selectedModelId" | "lastChatModelId" | "modelPrefs" | "selectionSettled" | "modelNoticeFor">
): Partial<AppState> {
  // `MOD-3`: the default (any source), then the next available favorite,
  // then the library's marked model, then any chat model — never silently
  // land on a media model, which would change what pressing send does.
  const resolved = resolveChatModel(models, libraryModels, s.modelPrefs);
  const fallback = resolved.id;
  const patch: Partial<AppState> = {};
  // Before every source has answered, keep following the preference rather
  // than holding whatever happened to load first.
  const unsettled = !s.selectionSettled;
  if ((unsettled || !models.some((m) => m.id === s.selectedModelId)) && fallback) {
    patch.selectedModelId = fallback;
  }
  if ((unsettled || !models.some((m) => m.id === s.lastChatModelId)) && fallback) {
    patch.lastChatModelId = fallback;
  }
  // Say so once when the default had to be skipped — but only after startup
  // settles, since a cloud default is "missing" until its list arrives.
  if (!unsettled && patch.selectedModelId && resolved.skippedDefault && s.modelNoticeFor !== resolved.skippedDefault) {
    patch.modelNotice = skippedNotice(s.modelPrefs, resolved, models);
    patch.modelNoticeFor = resolved.skippedDefault;
  }
  return patch;
}

/** `MOD-3`: a new chat opens on the default chat model, or the next available
 * favorite when the default can't be used (saying so once). Existing chats
 * keep whatever they were using. */
function openOnPreferredModel(get: () => AppState, set: (p: Partial<AppState>) => void) {
  const s = get();
  const resolved = resolveChatModel(s.models, s.libraryModels, s.modelPrefs);
  if (!resolved.id) return;
  if (resolved.skippedDefault && s.modelNoticeFor !== resolved.skippedDefault) {
    set({
      modelNotice: skippedNotice(s.modelPrefs, resolved, s.models),
      modelNoticeFor: resolved.skippedDefault,
    });
  }
  if (resolved.id !== s.selectedModelId) s.selectModel(resolved.id);
}

/** Write every model-preference setting. Small, and it keeps the stored
 * record whole rather than half-updated. */
function persistPrefs(prefs: ModelPrefs) {
  if (!api.inTauri()) return;
  const writes: [string, string][] = [
    [PREF_KEYS.defaultChat, prefs.defaults.chat ?? ""],
    [PREF_KEYS.defaultImage, prefs.defaults.image ?? ""],
    [PREF_KEYS.favChat, JSON.stringify(prefs.favorites.chat)],
    [PREF_KEYS.favMedia, JSON.stringify(prefs.favorites.media)],
    [PREF_KEYS.labels, JSON.stringify(prefs.labels)],
  ];
  for (const [k, v] of writes) api.setSetting(k, v).catch(() => {});
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
    // `PLN-UI-5`: a plan that vanished on reload was never state, it was
    // decoration — so it comes back with the timeline it belongs to.
    plan: api.parsePlan(m.plan_json),
    // A turn that stopped short keeps saying so across a reload — the mark is
    // about the answer's completeness, not about this session.
    stopReason: m.stop_reason && m.stop_reason !== "completed" ? m.stop_reason : undefined,
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
    parentConversationId: c.parent_conversation_id,
    projectId: c.project_id ?? null,
  };
}

/** `PRJ-1`: the backend row, in the frontend's shape. */
export function toProject(p: api.DbProject): Project {
  return {
    id: p.id,
    name: p.name,
    rootPath: p.root_path,
    instructions: p.instructions,
    trust: (p.trust as FolderTrust) ?? "confirm",
    execPolicy: (p.exec_policy as Project["execPolicy"]) ?? "ask",
    cardJson: p.card_json,
    tabsJson: p.tabs_json,
    archived: p.archived,
    updatedAt: p.updated_at,
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
  // `SHL-24`: a route is a destination again. Settings, Library and a project
  // are places you go to and come back from, not things you accumulate in the
  // strip — a tab per surface read as clutter and as a second, competing list
  // of where you might be. `setView` is the whole router once more.
  setView: (view) => set({ view }),
  railCollapsed: false,
  toggleRail: () => set((s) => ({ railCollapsed: !s.railCollapsed })),
  paletteOpen: false,
  setPaletteOpen: (open) => set({ paletteOpen: open }),

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
    // An explicit pick ends startup's "follow the default" phase.
    set({
      selectedModelId: id,
      selectionSettled: true,
      ...(!m?.modality || m.modality === "chat" ? { lastChatModelId: id } : {}),
    });
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
        ...reconcileSelection(models, lib, s),
      };
    });
    // `MOD-4`: a first download on a new install becomes its first favorite.
    const s = get();
    if (s.selectionSettled && s.modelPrefsLoaded) {
      const prefs = normalizePrefs(s.modelPrefs, s.libraryModels, s.models);
      if (prefsChanged(prefs, s.modelPrefs)) {
        set({ modelPrefs: prefs });
        persistPrefs(prefs);
      }
    }
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
        ...reconcileSelection(models, s.libraryModels, s),
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
        ...reconcileSelection(models, s.libraryModels, s),
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
        ...reconcileSelection(models, s.libraryModels, s),
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
      set({ loadingModel: { id, label: "Starting the runtime…" }, engineReady: false });
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

  modelPrefs: EMPTY_PREFS,
  modelPrefsLoaded: false,
  selectionSettled: false,
  modelNotice: null,
  modelNoticeFor: null,
  dismissModelNotice: () => set({ modelNotice: null }),

  loadModelPrefs: async () => {
    if (!api.inTauri()) return;
    const [chat, image, favChat, favMedia, labels] = await Promise.all(
      [PREF_KEYS.defaultChat, PREF_KEYS.defaultImage, PREF_KEYS.favChat, PREF_KEYS.favMedia, PREF_KEYS.labels].map(
        (k) => api.getSetting(k).catch(() => null)
      )
    );
    set({
      modelPrefs: {
        defaults: { chat: chat || null, image: image || null },
        favorites: { chat: parseIdList(favChat), media: parseIdList(favMedia) },
        labels: parseLabels(labels),
      },
      modelPrefsLoaded: true,
    });
  },

  // Every source has answered once: apply the rules (seed the default, give
  // a new install its first favorite), then settle the selection for good.
  settleSelection: () => {
    const s = get();
    const prefs = s.modelPrefsLoaded ? normalizePrefs(s.modelPrefs, s.libraryModels, s.models) : s.modelPrefs;
    if (prefsChanged(prefs, s.modelPrefs)) persistPrefs(prefs);
    const withPrefs = { ...s, modelPrefs: prefs };
    // An explicit pick before startup finished stands; only fill the notice.
    const patch = s.selectionSettled ? {} : reconcileSelection(s.models, s.libraryModels, withPrefs);
    const resolved = resolveChatModel(s.models, s.libraryModels, prefs);
    const notice =
      !s.selectionSettled && resolved.skippedDefault && s.modelNoticeFor !== resolved.skippedDefault
        ? { modelNotice: skippedNotice(prefs, resolved, s.models), modelNoticeFor: resolved.skippedDefault }
        : {};
    set({ modelPrefs: prefs, ...patch, ...notice, selectionSettled: true });
  },

  setDefaultModelPref: async (id, fallback) => {
    const model = get().models.find((m) => m.id === id) ?? fallback;
    if (!model || model.modality === "video") return;
    const prefs = setDefault(get().modelPrefs, model);
    set({ modelPrefs: prefs, modelNoticeFor: null });
    persistPrefs(prefs);
    if (!api.inTauri()) return;
    // The runtime's own "start" still reads the library's marked model, and
    // the local image backend its own checkpoint, so keep both in step.
    if (model.provenance === "local" && !model.modality) {
      await api.setDefaultModel(id).catch(() => {});
      await get().refreshLibrary().catch(() => {});
    } else if (model.modality === "image" && model.backendId === "local") {
      const path = id.replace(/^media:local\//, "");
      await api.setDefaultImageModel(path).catch(() => {});
      await get().refreshMediaModels().catch(() => {});
    }
  },

  toggleFavoriteModel: (id, fallback) => {
    const model = get().models.find((m) => m.id === id) ?? fallback;
    if (!model) {
      // Unavailable right now, but still removable from the list.
      const prefs = get().modelPrefs;
      const tab: FavoriteTab = prefs.favorites.media.includes(id) ? "media" : "chat";
      if (prefs.defaults.chat === id || prefs.defaults.image === id) return false;
      const next = { ...prefs, favorites: { ...prefs.favorites, [tab]: prefs.favorites[tab].filter((x) => x !== id) } };
      set({ modelPrefs: next });
      persistPrefs(next);
      return true;
    }
    const next = toggleFavorite(get().modelPrefs, model);
    if (!next) return false;
    set({ modelPrefs: next });
    persistPrefs(next);
    return true;
  },

  moveFavoriteModel: (tab, id, delta) => {
    const next = moveFavorite(get().modelPrefs, tab, id, delta);
    set({ modelPrefs: next });
    persistPrefs(next);
  },

  reorderFavoriteModels: (tab, ids) => {
    const next = reorderFavorites(get().modelPrefs, tab, ids);
    set({ modelPrefs: next });
    persistPrefs(next);
  },

  modelsSource: null,
  setModelsSource: (modelsSource) => set({ modelsSource }),
  openModelsFiltered: (modelsSource) => set({ modelsSource, view: "models" }),
  runtimeTab: null,
  openRuntime: (tab) => set({ runtimeTab: tab ?? null, view: "runtime" }),
  providerFocus: null,
  openProviders: (id) => set({ providerFocus: id ?? null, view: "providers" }),

  conversations: [],
  activeConversationId: null,
  busy: false,
  activeRun: null,
  subRuns: {},
  permissionAgents: {},
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
  createPersona: async ({
    name,
    systemPrompt,
    modelId,
    temperature,
    toolsJson,
    skillsJson,
    description,
    spawnable,
  }) => {
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
      description: description ?? null,
      spawnable: spawnable ?? false,
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
      // The same trip settles the planning setting: both are read once, before
      // the first turn assembles a prompt that has to agree with the backend.
      const mode = await api.getSetting(PLAN_MODE_KEY);
      set({ planMode: mode === "always" || mode === "never" ? mode : "auto" });
    } catch {
      /* keep the default */
    }
  },

  planMode: "auto",
  setPlanMode: async (mode) => {
    set({ planMode: mode });
    if (api.inTauri()) await api.setSetting(PLAN_MODE_KEY, mode);
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
  agentDoneToast: null,
  dismissAgentDoneToast: () => set({ agentDoneToast: null }),
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
      tabSetRaw,
      projectRows,
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
      api.getSetting(TAB_SET_KEY),
      // `PRJ-UI-1`: loaded with the conversations rather than after them. The
      // Rail groups sessions under their project on its first paint, and a
      // list that arrives a beat later would reshuffle the whole Rail in front
      // of the user.
      api.listProjects(),
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
    // `SHL-17`: restore the header strip, dropping anything that no longer
    // resolves. A set that fails to parse is worth nothing and is discarded
    // rather than partially recovered.
    const projects = projectRows.map(toProject);
    // One global strip. A build that scoped the strip per project wrote the
    // live project's set onto its row; that is read once, only when there is
    // no global set yet, so the tabs someone had open do not vanish on upgrade.
    const liveProject = projects.find((p) => p.id === conversations[0].projectId);
    const { itemTabs, activeItemId, dockView } = validateTabSet(
      tabSetRaw ?? liveProject?.tabsJson,
      conversations
    );
    set({
      projects,
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
      itemTabs,
      activeItemId,
      selected: itemToSelection(itemTabs.find((t) => itemKey(t) === activeItemId)),
      dockView,
      bootstrapped: true,
    });
    // Swallowed deliberately: a failed library read must not abort the rest of
    // bootstrap. It used to throw straight out of here, which skipped every
    // refresh below — including the one that flips `modelsLoaded`, so the
    // first-run guide could never appear on exactly the broken installs that
    // needed it most.
    await get().loadModelPrefs().catch(() => {});
    await get().refreshLibrary().catch(() => {});
    // `PRJ-UI-2`: a restored file tab's dot reads its own chat's change set,
    // and that chat may not be the live one.
    for (const convId of new Set(itemTabs.map((t) => t.conversationId).filter((c): c is string => !!c))) {
      get().refreshChanges(convId).catch(() => {});
    }
    // Cloud models load in the background (network) — don't block startup.
    // `modelsLoaded` flips once they land, so anything that keys off "no
    // models and no keys" (the first-run guide) judges a list that has
    // actually arrived rather than one that is merely still empty.
    Promise.all([get().refreshCloud(), get().refreshMediaModels(), get().refreshEndpoints()]).finally(() => {
      get().settleSelection();
      set({ modelsLoaded: true });
    });
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
    scheduleCatchUpReflection(get);
    await get().setActiveConversation(conversations[0].id);
  },

  setActiveConversation: async (id) => {
    // Rail rows call this unconditionally on every click, including a click on
    // the chat that's already active (e.g. returning to it from Settings) —
    // so "did the conversation actually change" has to be judged here, before
    // any of the below overwrites it with itself.
    const switchingConversation = id !== get().activeConversationId;

    reflectOnLeaving(get, id);

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
      // Item tabs stay open across a switch; each one remembers its own chat.
      // Only focus moves: the sidebar goes back to its overview. An item being
      // opened in that chat sets its focus right after this.
      ...(switchingConversation ? { selected: null, activeItemId: null } : {}),
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
    persistTabSet(get());
    if (!api.inTauri()) return;
    get().refreshTree().catch(() => {});
    get().refreshTrash().catch(() => {});
    get().refreshChanges(id).catch(() => {});
    get().refreshIndexStatus().catch(() => {});
    const rows = await api.listMessages(id);
    // `SUB-UI-1`: after the messages land, so the children can be hung off the
    // turns that started them.
    setTimeout(() => get().loadSubRuns(id).catch(() => {}), 0);
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
    // REF-3: starting a new chat leaves the current one behind just as much as
    // switching to another does — reflect on it before it scrolls out of reach.
    reflectOnLeaving(get);
    // Starting a chat while workspace mode is on pins the new session to it.
    const workspace = get().workspaceMode;
    // `PRJ-UI-2`: the `+` in the session zone starts a new session *in the
    // project you are in*, not a loose chat beside it. One call site, and it
    // is why the strip never needed to learn what a project is.
    const from = get().conversations.find((c) => c.id === get().activeConversationId);
    const projectId = from?.projectId ?? null;
    const folderPath = projectId ? (from?.folderPath ?? null) : null;
    if (!api.inTauri()) {
      const id = `c-${Date.now()}`;
      set((s) => ({
        conversations: [
          {
            id,
            title: "New chat",
            updatedAt: Date.now(),
            messages: [],
            workspace,
            projectId,
            folderPath,
          },
          ...s.conversations,
        ],
        activeConversationId: id,
        view: "chat",
        selected: null,
        activeItemId: null,
      }));
      openOnPreferredModel(get, set);
      return;
    }
    const created = await api.createConversation("New chat", undefined, workspace);
    if (projectId) await api.setConversationProject(created.id, projectId);
    set((s) => ({
      conversations: [{ ...toConversation(created), projectId, folderPath }, ...s.conversations],
      activeConversationId: created.id,
      view: "chat",
      selected: null,
      activeItemId: null,
    }));
    openOnPreferredModel(get, set);
    persistTabSet(get());
  },

  forkFromMessage: async (messageId) => {
    const convId = get().activeConversationId;
    if (!convId || !api.inTauri() || !isPersistedId(messageId)) return;
    const { conversation, resend } = await api.forkConversation(convId, messageId);
    set((s) => ({ conversations: [toConversation(conversation), ...s.conversations] }));
    await get().setActiveConversation(conversation.id);
    // Sending is what makes this a rerun rather than a copy: it builds a fresh
    // prompt from the branch's own history, so the model meets the question
    // without the answer it is being asked to replace.
    if (resend) await get().sendMessage(resend);
  },

  resumeLastRun: async () => {
    const state = get();
    const convId = state.activeConversationId;
    const model = state.models.find((m) => m.id === state.selectedModelId) ?? state.models[0];
    if (!convId || !model || state.busy || !api.inTauri()) return;
    const conv = state.conversations.find((c) => c.id === convId);

    const assistantId = `a-${Date.now()}`;
    const assistantMsg: Message = {
      id: assistantId,
      role: "assistant",
      model: { name: model.name, provenance: model.provenance },
      steps: [],
      text: "",
      streaming: true,
      createdAt: Date.now(),
    };
    set((s) => ({
      busy: true,
      presence: "active",
      conversations: s.conversations.map((c) =>
        c.id === convId
          ? { ...c, updatedAt: Date.now(), messages: [...c.messages, assistantMsg] }
          : c
      ),
    }));

    let persistedAssistantId = assistantId;
    try {
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

    const engineError = await ensureEngineForModel(get, model);
    if (engineError) {
      patchAssistant(set, convId, assistantId, { text: engineError, streaming: false });
      set((st) => ({ busy: false, presence: st.reflectingIds.length ? "reflecting" : "idle" }));
      return;
    }

    const persona = conv?.personaId
      ? state.personas.find((p) => p.id === conv.personaId)
      : undefined;
    await streamAssistantTurn(set, get, {
      convId,
      assistantId,
      persistedAssistantId,
      // Deliberately empty: a resumed run's transcript is rebuilt in Rust from
      // the session log, because that is the only place the interrupted run's
      // tool results survive. Reassembling here would send the prose back and
      // silently throw the work away.
      turns: [],
      model,
      temperature: conv?.overrides?.temperature ?? personaTemperature(persona),
      resume: true,
    });
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
      // Its item tabs go with it: nothing they point at resolves any more.
      const itemTabs = s.itemTabs.filter((t) => t.conversationId !== id);
      const itemGone = !!s.activeItemId && !itemTabs.some((t) => itemKey(t) === s.activeItemId);
      return {
        conversations: remaining,
        activeConversationId: active,
        itemTabs,
        ...(itemGone ? { activeItemId: null, selected: null } : {}),
      };
    });
    persistTabSet(get());
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
      memoryEnabled: get().memoryToolEnabled,
      toolHealth: get().toolHealth,
      skills: skillsForPersona(get().skills, persona?.skills_json),
      planMode: get().planMode,
      ...projectPrompt(get(), convId),
    });
    const effectiveTemperature =
      conv?.overrides?.temperature ?? personaTemperature(persona);

    // System prompt + as much history as the context window holds + this turn.
    const turns = await assembleTurns(set, get, {
      convId,
      system: effectiveSystemPrompt,
      current: { role: "user", content: userContent },
      currentId: userMsg.id,
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
      memoryEnabled: get().memoryToolEnabled,
      toolHealth: get().toolHealth,
      skills: skillsForPersona(get().skills, persona?.skills_json),
      planMode: get().planMode,
      ...projectPrompt(get(), convId),
    });
    const effectiveTemperature =
      conv?.overrides?.temperature ?? personaTemperature(persona);

    const turns = await assembleTurns(set, get, {
      convId,
      system: effectiveSystemPrompt,
      current: { role: "user", content: modelContent },
      currentId: userMsg.id,
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

  steerActiveRun: async (text) => {
    const body = text.trim();
    const run = get().activeRun;
    const convId = get().activeConversationId;
    if (!body || !run || !convId || run.convId !== convId || !api.inTauri()) return false;

    // The message goes on screen before the backend has seen it: the point of
    // the feature is that typing lands immediately. `midRun: "pending"` is what
    // marks it as not yet picked up; the `steered` event settles it.
    const msg: Message = {
      id: `u-${Date.now()}`,
      role: "user",
      text: body,
      midRun: "pending",
      createdAt: Date.now(),
    };
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === convId ? { ...c, updatedAt: Date.now(), messages: [...c.messages, msg] } : c
      ),
    }));

    const delivered = await api.steerRun(run.runId, body).catch(() => false);
    if (!delivered) {
      // The run ended in the gap between the keystroke and the send. Take the
      // message back off screen rather than leaving a turn nothing will read;
      // the composer resends it as an ordinary message.
      set((s) => ({
        conversations: s.conversations.map((c) =>
          c.id === convId ? { ...c, messages: c.messages.filter((m) => m.id !== msg.id) } : c
        ),
      }));
      return false;
    }
    // A steer is a real user turn: it has to survive a reload, and the next
    // turn's context has to include it.
    await api
      .appendMessage({ conversationId: convId, role: "user", content: body })
      .catch(() => {});
    return true;
  },

  loadSubRuns: async (convId) => {
    if (!api.inTauri()) return;
    const rows = await api.listSubagentRuns(convId).catch(() => []);
    if (!rows.length) return;
    set((s) => {
      const subRuns = { ...s.subRuns };
      for (const row of rows) {
        // A live child's own accumulators are ahead of the row, which is only
        // written at the start and at the end — never overwrite one.
        if (api.stillWorking(subRuns[row.id]?.status ?? "done")) continue;
        subRuns[row.id] = {
          runId: row.id,
          conversationId: row.child_conversation_id,
          parentConversationId: row.parent_conversation_id,
          agent: row.agent,
          task: row.task,
          // Taken at face value: `SUB-12` settles every run a restart orphaned
          // at startup, so a row still unfinished here is a background child
          // genuinely still working — one this session simply has not met yet.
          status: row.status,
          stopReason: row.stop_reason ?? undefined,
          steps: [],
          text: row.result ?? "",
          startedAt: row.started_at,
          endedAt: row.ended_at ?? undefined,
        };
      }
      return { subRuns };
    });
    // Hang the children back off the turn that started them, so the Fleet card
    // is there after a reload and not only in the session that made it.
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id !== convId
          ? c
          : {
              ...c,
              messages: c.messages.map((m) => {
                const mine = rows.filter((r) => r.parent_message_id === m.id).map((r) => r.id);
                return mine.length ? { ...m, subRunIds: mine } : m;
              }),
            }
      ),
    }));
  },

  steerSubRun: async (runId, text) => {
    const body = text.trim();
    if (!body || !api.inTauri()) return false;
    const delivered = await api.steerSubagent(runId, body).catch(() => false);
    if (delivered) patchSubRun(set, runId, { steerPending: true });
    return delivered;
  },

  stopSubRun: async (runId) => {
    if (!api.inTauri()) return;
    await api.stopRun(runId).catch(() => {});
    // The child's own `sub_ended` settles the row properly; this is only so the
    // button stops inviting a second click while that is in flight.
    patchSubRun(set, runId, { status: "stopped", stopReason: "aborted" });
  },

  setSystemPrompt: async (prompt) => {
    set({ systemPrompt: prompt });
    if (api.inTauri()) await api.setSetting(SYSTEM_PROMPT_KEY, prompt);
  },

  resolvePermission: async (id, decision) => {
    const request = get().pendingPermissions.find((p) => p.id === id);
    set((s) => {
      const permissionAgents = { ...s.permissionAgents };
      delete permissionAgents[id];
      return { pendingPermissions: s.pendingPermissions.filter((p) => p.id !== id), permissionAgents };
    });
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
  selectNode: (selection) => {
    if (!selection) {
      // "Close whatever's showing" — that means closing the active item tab,
      // not just blanking a field.
      const activeItemId = get().activeItemId;
      if (activeItemId) get().closeItem(activeItemId);
      return;
    }
    focusItem(set, get, selection);
  },
  openArtifact: (artifactId) => {
    const convId = get().activeConversationId;
    const artifact = convId
      ? (get().artifacts[convId] ?? []).find((a) => a.id === artifactId)
      : undefined;
    // A saved artifact is a file now — show it where it actually lives.
    focusItem(
      set,
      get,
      artifact?.saved_path ? { kind: "file", id: artifact.saved_path } : { kind: "artifact", id: artifactId }
    );
  },
  itemTabs: [],
  activeItemId: null,
  openItem: (ref) => focusItem(set, get, ref),
  unsavedFiles: {},
  setUnsaved: (path, unsaved) =>
    set((s) => {
      if (unsaved === !!s.unsavedFiles[path]) return {};
      const next = { ...s.unsavedFiles };
      if (unsaved) next[path] = true;
      else delete next[path];
      return { unsavedFiles: next };
    }),
  closeItem: (id) => {
    // `EDT-1`: a tab is the only place an unsaved edit exists, so closing one
    // is the one moment it can be lost. Asked here rather than in the strip
    // so every route to closing — the ×, middle-click, the pane's own button,
    // a keyboard shortcut — goes through the same question.
    const closing = get().itemTabs.find((t) => itemKey(t) === id);
    if (closing?.kind === "file" && get().unsavedFiles[closing.id]) {
      const name = closing.id.split(/[\\/]/).pop();
      if (!confirm(`${name} has unsaved changes. Close it and lose them?`)) return;
      get().setUnsaved(closing.id, false);
    }
    set((s) => {
      const idx = s.itemTabs.findIndex((t) => itemKey(t) === id);
      if (idx === -1) return {};
      const itemTabs = s.itemTabs.filter((_, i) => i !== idx);
      if (s.activeItemId !== id) return { itemTabs };
      // The right-hand neighbour first, then the left.
      const closed = s.itemTabs[idx];
      // Only a neighbour from the chat already live, since closing a tab must
      // not switch which chat you are in. With none, the overview shows.
      const sameChat = (t: ItemRef) => t.conversationId === closed.conversationId;
      const neighbor =
        itemTabs.slice(idx).find(sameChat) ?? itemTabs.slice(0, idx).reverse().find(sameChat);
      return {
        itemTabs,
        activeItemId: neighbor ? itemKey(neighbor) : null,
        selected: itemToSelection(neighbor),
      };
    });
    persistTabSet(get());
  },
  dockView: "files",
  setDockView: (dockView) => {
    if (get().dockView === dockView) return;
    set({ dockView });
    persistTabSet(get());
  },
  openSession: async (id) => {
    // A chat is a destination (`SHL-24`). Showing one closes the item pane —
    // what was in it belonged to the chat you just left.
    if (get().activeItemId) set({ activeItemId: null, selected: null });
    await get().setActiveConversation(id);
  },
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
  showConversation: () => {
    if (get().activeItemId === null) return;
    set({ activeItemId: null, selected: null });
    persistTabSet(get());
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

  projects: [],

  refreshProjects: async () => {
    if (!api.inTauri()) return;
    try {
      set({ projects: (await api.listProjects()).map(toProject) });
    } catch {
      // A Rail group that fails to load is worth less than the Rail; the
      // conversation list below it is unaffected either way.
    }
  },

  activeProjectId: null,

  newProject: async () => {
    // `PRJ-UI-1a`: no folder picker. A project is a named group of sessions,
    // and most are not about a directory — opening a file dialog first would
    // say the opposite. The view it lands in is where a folder gets added, if
    // one ever does.
    set({ folderError: null });
    try {
      const project = api.inTauri()
        ? toProject(await api.createProject(null))
        : {
            id: `p-${Date.now()}`,
            name: NEW_PROJECT_NAME,
            rootPath: null,
            instructions: null,
            trust: "confirm" as FolderTrust,
            execPolicy: "ask" as const,
            archived: false,
            updatedAt: Date.now(),
          };
      set((s) => ({
        projects: [project, ...s.projects.filter((p) => p.id !== project.id)],
        expandedProjects: [...new Set([...s.expandedProjects, project.id])],
      }));
      get().openProjectView(project.id);
    } catch (e) {
      set({ folderError: String(e) });
    }
  },

  openProjectView: (projectId) => {
    // A route tab, exactly like Settings (`SHL-14`): it opens beside the chats
    // and closes without losing them. `activeProjectId` says *which* project,
    // the same way `activeConversationId` says which chat.
    set({ activeProjectId: projectId });
    get().setView("project");
  },

  setProjectInstructions: async (projectId, instructions) => {
    const trimmed = instructions.trim();
    set((s) => ({
      projects: s.projects.map((p) =>
        p.id === projectId ? { ...p, instructions: trimmed || null } : p
      ),
    }));
    if (api.inTauri()) await api.setProjectInstructions(projectId, trimmed || null);
  },

  setProjectFolder: async (projectId, pick) => {
    if (!api.inTauri()) return;
    set({ folderError: null });
    try {
      let root: string | null = null;
      if (pick) {
        root = await api.pickFolder();
        // Cancelling the dialog is not a request to remove the folder.
        if (!root) return;
      }
      const project = toProject(await api.setProjectRoot(projectId, root));
      set((s) => ({
        projects: s.projects.map((p) => (p.id === projectId ? project : p)),
        // The sessions' own fallback column followed in the backend; mirror it
        // here or a chat on screen still shows the folder that was removed.
        conversations: s.conversations.map((c) =>
          c.projectId === projectId ? { ...c, folderPath: project.rootPath } : c
        ),
        folderTree: {},
        expandedDirs: [],
        indexState: null,
        indexProgress: null,
        indexError: null,
      }));
      const active = get().conversations.find((c) => c.id === get().activeConversationId);
      if (active?.projectId === projectId) {
        await get().refreshTree().catch(() => {});
        get().refreshIndexStatus().catch(() => {});
      }
    } catch (e) {
      set({ folderError: String(e) });
    }
  },

  moveSessionToProject: async (conversationId, projectId) => {
    const project = projectId ? get().projects.find((p) => p.id === projectId) : undefined;
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === conversationId
          ? { ...c, projectId: projectId ?? null, folderPath: project?.rootPath ?? null }
          : c
      ),
    }));
    if (api.inTauri()) await api.setConversationProject(conversationId, projectId);
  },

  openProject: async (projectId) => {
    const state = get();
    const project = state.projects.find((p) => p.id === projectId);
    if (!project) return;
    // The most recent session in the project, or a fresh one. A project with
    // no session yet is not an empty state to design around — it is one call.
    const existing = state.conversations
      .filter((c) => c.projectId === projectId && !c.parentConversationId)
      .sort((a, b) => b.updatedAt - a.updatedAt)[0];
    if (existing) {
      await state.setActiveConversation(existing.id);
      return;
    }
    await state.newConversation();
    const convId = get().activeConversationId;
    if (!convId) return;
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === convId ? { ...c, projectId, folderPath: project.rootPath } : c
      ),
      folderTree: {},
      expandedDirs: [],
      dockOpen: true,
    }));
    if (api.inTauri()) {
      await api.setConversationProject(convId, projectId);
      await get().refreshTree();
      get().refreshIndexStatus().catch(() => {});
    }
  },

  renameProject: async (projectId, name) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    set((s) => ({
      projects: s.projects.map((p) => (p.id === projectId ? { ...p, name: trimmed } : p)),
    }));
    if (api.inTauri()) await api.renameProject(projectId, trimmed);
  },

  archiveProject: async (projectId) => {
    // Hides the project and its sessions. Nothing on disk is touched, ever,
    // which is why there is no delete beside this.
    set((s) => ({
      projects: s.projects.filter((p) => p.id !== projectId),
      expandedProjects: s.expandedProjects.filter((id) => id !== projectId),
    }));
    if (api.inTauri()) await api.setProjectArchived(projectId, true);
  },

  expandedProjects: [],

  toggleProjectExpanded: (projectId) =>
    set((s) => ({
      expandedProjects: s.expandedProjects.includes(projectId)
        ? s.expandedProjects.filter((id) => id !== projectId)
        : [...s.expandedProjects, projectId],
    })),

  attachFolder: async () => {
    if (!api.inTauri()) return;
    const convId = get().activeConversationId;
    if (!convId) return;
    set({ folderError: null });
    try {
      const picked = await api.pickFolder();
      if (!picked) return;
      // `PRJ-3`: this creates or joins the project for that folder, in the
      // backend, under one existing gesture. There is nothing new to learn and
      // no empty-project state to design around — and the trust granted last
      // time is already there.
      await api.setConversationFolder(convId, picked);
      const project = (await api.listProjects()).find((p) => p.root_path === picked);
      set((s) => ({
        conversations: s.conversations.map((c) =>
          c.id === convId
            ? {
                ...c,
                folderPath: picked,
                projectId: project?.id ?? null,
                folderTrust: (project?.trust as FolderTrust) ?? c.folderTrust,
              }
            : c
        ),
        projects: project
          ? [toProject(project), ...s.projects.filter((p) => p.id !== project.id)]
          : s.projects,
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
    // Nothing on disk is touched — this only forgets the path. `PRJ-3`: it
    // also leaves the project, and the project and its other sessions stand.
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === convId ? { ...c, folderPath: null, projectId: null } : c
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
    // `PRJ-4`: with a project attached this grants trust for the *folder*, so
    // every session in it — including ones opened later — sees it. The
    // backend writes both; the mirror here has to match or the sibling
    // sessions on screen would still show the old level.
    const projectId = get().conversations.find((c) => c.id === convId)?.projectId ?? null;
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === convId || (projectId && c.projectId === projectId)
          ? { ...c, folderTrust: trust }
          : c
      ),
      projects: s.projects.map((p) => (p.id === projectId ? { ...p, trust } : p)),
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

  changeSets: {},
  refreshChanges: async (conversationId) => {
    if (!api.inTauri()) return;
    const convId = conversationId ?? get().activeConversationId;
    if (!convId) return;
    try {
      const changes = await api.conversationChanges(convId);
      set((s) => ({ changeSets: { ...s.changeSets, [convId]: changes } }));
    } catch {
      /* a change set that fails to load leaves the last one standing */
    }
  },
  undoChanges: async (conversationId, path) => {
    if (!api.inTauri()) return;
    await api.undoChanges(conversationId, path);
    await get().refreshChanges(conversationId);
    if (conversationId === get().activeConversationId) {
      get().refreshTrash().catch(() => {});
      get().refreshTree().catch(() => {});
    }
  },
  keepChanges: async (conversationId) => {
    if (!api.inTauri()) return;
    await api.keepChanges(conversationId);
    await get().refreshChanges(conversationId);
  },
  changesFocus: null,
  focusChange: (conversationId, path) => {
    // A click on a tab's dot is the user's own action, so it may make that
    // tab's chat live first, the way pressing the tab does.
    if (conversationId !== get().activeConversationId) void get().setActiveConversation(conversationId);
    set({ changesFocus: path, dockOpen: true });
    get().setDockView("changes");
  },

  projectCards: {},
  refreshProjectCard: async (projectId, redetect) => {
    if (!api.inTauri()) return;
    try {
      const view = await api.projectCard(projectId, redetect);
      set((s) => ({ projectCards: { ...s.projectCards, [projectId]: view } }));
    } catch {
      /* the header shows no chips rather than an error about chips */
    }
  },
  setProjectExecPolicy: async (projectId, policy) => {
    set((s) => ({
      projects: s.projects.map((p) => (p.id === projectId ? { ...p, execPolicy: policy } : p)),
    }));
    if (!api.inTauri()) return;
    await api.setProjectExecPolicy(projectId, policy);
    await get().refreshProjectCard(projectId);
  },
  setProjectTaskAllowed: async (projectId, task, allowed) => {
    if (!api.inTauri()) return;
    await api.setProjectTaskAllowed(projectId, task, allowed);
    await get().refreshProjectCard(projectId);
  },
  setProjectCommands: async (projectId, runCommand, forget) => {
    if (!api.inTauri()) return;
    await api.setProjectCommands(projectId, runCommand, forget);
    await get().refreshProjectCard(projectId);
  },

  undoFileOp: async (id) => {
    if (!api.inTauri()) return;
    const entry = get().trash.find((t) => t.id === id);
    await api.undoFileOp(id);
    get().refreshChanges().catch(() => {});
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
    // file in the tree, opened as an item tab so the user sees where it
    // landed.
    set((s) => ({
      artifacts: {
        ...s.artifacts,
        [convId]: (s.artifacts[convId] ?? []).map((a) =>
          a.id === artifactId ? { ...a, saved_path: written } : a
        ),
      },
      touchedFiles: { ...s.touchedFiles, [written]: Date.now() },
    }));
    focusItem(set, get, { kind: "file", id: written });
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
    focusItem(
      set,
      get,
      artifact.saved_path ? { kind: "file", id: artifact.saved_path } : { kind: "artifact", id: artifact.id }
    );
  },

  imageLightbox: null,
  viewArtifactByPath: (path, dataUri, alt) => set({ imageLightbox: { path, dataUri, alt } }),
  closeImageLightbox: () => set({ imageLightbox: null }),
}));

type StoreSet = (fn: (s: AppState) => Partial<AppState>) => void;
type StoreGet = () => AppState;

/** An item tab's stable identity — what `activeItemId` and the strip's `key`
 * prop both use, and the only place the string is built (`SHL-22`). */
export function itemKey(ref: ItemRef): string {
  return `${ref.kind}:${ref.id}`;
}

/** The reverse of `itemKey` for the two kinds `selected` understands — `null`
 * for a run, which the Viewer does not render. */
function itemToSelection(ref: ItemRef | undefined): WorkbenchSelection | null {
  if (!ref || ref.kind === "run" || ref.kind === "diff") return null;
  return { kind: ref.kind, id: ref.id };
}

const DOCK_VIEWS: readonly DockView[] = ["files", "artifacts", "agents", "browser", "changes"];
function isDockView(x: unknown): x is DockView {
  return typeof x === "string" && (DOCK_VIEWS as readonly string[]).includes(x);
}

/** Is this path under that folder? Compared with both separators normalised,
 * since a path can reach the store from Windows APIs (`\`) and from the
 * agent's own tool calls (`/`) in the same session. */
/** `SHL-17`: turn whatever was on disk into a tab set this build can actually
 * render, dropping every entry that no longer resolves.
 *
 * Pure and exported so the dropping rules can be tested directly — they are
 * the half of persistence that is easy to get wrong and impossible to notice,
 * since a bad entry shows up as a tab that does nothing rather than as an
 * error. A set that fails to parse at all is discarded whole: a half-recovered
 * strip is worth less than an empty one.
 *
 * `conversations` must be non-empty, and `conversations[0]` is the chat that
 * is about to become live — every "does this still resolve" question is asked
 * against it.
 */
export function validateTabSet(
  raw: string | null | undefined,
  conversations: Conversation[]
): {
  itemTabs: ItemRef[];
  activeItemId: string | null;
  dockView: DockView;
} {
  let itemTabs: ItemRef[] = [];
  let activeItemId: string | null = null;
  let dockView: DockView = "files";
  try {
    const parsed = raw ? (JSON.parse(raw) as Record<string, unknown>) : null;
    if (parsed && typeof parsed === "object") {
      // `sessionTabs` and `routeTabs` may still be on disk from a build that
      // put chats and routes in the strip (`SHL-24` took them out). They are
      // read and dropped: a chat and a route are destinations now, and nothing
      // in the app has a list of "open" ones to restore them into.
      //
      // A set written before `SHL-22` names these `docTabs`/`activeDocId`/
      // `docConversationId`, and holds `panel` entries for the sidebar's
      // sections. The panels are not items: the last one seen becomes the
      // sidebar's sub-view, so the section the user had open comes back as
      // what it now is instead of vanishing.
      const storedItems = Array.isArray(parsed.itemTabs)
        ? parsed.itemTabs
        : Array.isArray(parsed.docTabs)
          ? parsed.docTabs
          : [];
      for (const entry of storedItems) {
        const panel = legacyPanel(entry);
        if (panel) dockView = panel;
      }
      if (isDockView(parsed.dockView)) dockView = parsed.dockView;
      // Every item carries the chat it belongs to. A set written before the
      // strip went global holds items for one chat only, named once in
      // `itemConversationId`/`docConversationId`; those are stamped with it.
      const setConversationId = parsed.itemConversationId ?? parsed.docConversationId;
      itemTabs = storedItems.filter(isItemRef).flatMap((t) => {
        const conversationId =
          t.conversationId ?? (typeof setConversationId === "string" ? setConversationId : undefined);
        return conversationId ? [{ ...t, conversationId }] : [];
      });
      const active = parsed.activeItemId ?? parsed.activeDocId;
      if (typeof active === "string") activeItemId = active;
    }
  } catch {
    /* a tab set that fails to parse is discarded, not quarantined */
  }

  // An item whose chat is gone is dropped. A file tab only resolves inside the
  // folder its own chat is attached to: detach the folder, or attach a
  // different one, and the path is no longer something that chat can open —
  // so it is dropped rather than restored as a tab that can only ever show a
  // read error. (A file deleted from disk while the folder stayed put is not
  // caught here; the viewer reports that in place, which is a more useful
  // answer than a tab vanishing for reasons the user cannot see.)
  const byId = new Map(conversations.map((c) => [c.id, c]));
  itemTabs = itemTabs.filter((t) => {
    const owner = t.conversationId ? byId.get(t.conversationId) : undefined;
    if (!owner) return false;
    const folder = owner.folderPath ?? null;
    // `PRJ-UI-3`: a patch only exists for a chat that works in a folder. Whether
    // the change set still holds it is asked once the set has loaded, in
    // `ItemView`, since that needs the backend.
    if (t.kind === "diff") return !!folder;
    return t.kind !== "file" || (!!folder && isInsideFolder(t.id, folder));
  });
  // Focus only comes back on an item of the chat that is about to be live;
  // anything else would show an item over the wrong conversation.
  const activeItem = itemTabs.find((t) => itemKey(t) === activeItemId);
  if (!activeItem || activeItem.conversationId !== conversations[0].id) activeItemId = null;

  return { itemTabs, activeItemId, dockView };
}

export function isInsideFolder(path: string, folder: string): boolean {
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  const f = norm(folder);
  const p = norm(path);
  return p === f || p.startsWith(`${f}/`);
}

function isItemRef(x: unknown): x is ItemRef {
  if (!x || typeof x !== "object") return false;
  const r = x as { kind?: unknown; id?: unknown; conversationId?: unknown; line?: unknown };
  if (r.conversationId !== undefined && typeof r.conversationId !== "string") return false;
  if (r.line !== undefined && typeof r.line !== "number") return false;
  return (
    typeof r.id === "string" &&
    (r.kind === "file" || r.kind === "artifact" || r.kind === "run" || r.kind === "diff")
  );
}

/** A pre-`SHL-22` `{ kind: "panel" }` entry, read as the sub-view it became. */
function legacyPanel(x: unknown): DockView | null {
  if (!x || typeof x !== "object") return null;
  const r = x as { kind?: unknown; id?: unknown };
  return r.kind === "panel" && isDockView(r.id) ? r.id : null;
}

/** Opens (or focuses) an item tab and keeps `selected` mirroring it
 * (`SHL-10`/`SHL-22`) — every call site that used to hand-roll `selected`
 * routes through this one function instead. An item is shown where the
 * conversation is shown, so opening one from a route (Library) comes back to
 * the chat area to show it.
 *
 * The strip is global, so the tab is stamped with the chat it belongs to, and
 * an item from another chat makes that chat live first. The switch is
 * synchronous up to its first await, so the item is focused on the chat that
 * is now live, and the file reads and artifact lookups it does all resolve
 * against the right conversation. */
function focusItem(set: StoreSet, get: StoreGet, ref: ItemRef) {
  const conversationId = ref.conversationId ?? get().activeConversationId;
  if (!conversationId) return;
  if (conversationId !== get().activeConversationId) {
    void get().setActiveConversation(conversationId);
  }
  const stamped = { ...ref, conversationId } as ItemRef;
  const key = itemKey(stamped);
  set((s) => {
    const exists = s.itemTabs.some((t) => itemKey(t) === key);
    return {
      // A file open from two chats in one folder is one tab; it follows the
      // chat that opened it last.
      itemTabs: exists ? s.itemTabs.map((t) => (itemKey(t) === key ? stamped : t)) : [...s.itemTabs, stamped],
      activeItemId: key,
      selected: itemToSelection(stamped),
      view: "chat",
    };
  });
  persistTabSet(get());
}

/** `SHL-17`: the whole strip is one persisted set.
 *
 * Written on tab open/close rather than per render, but that is still every
 * `Ctrl+Tab` and every streamed artifact, so the write is coalesced onto a
 * trailing timer: a burst of focus changes costs one settings write instead of
 * one each. The last state always wins, which is the only ordering that
 * matters for a snapshot.
 *
 * One global set under one key, whichever project the live chat is in. Each
 * item tab carries its own `conversationId`, because an artifact id, a run id
 * or a path only means anything inside the chat it came from. */
let persistTimer: ReturnType<typeof setTimeout> | null = null;

function persistTabSet(state: AppState) {
  if (!api.inTauri()) return;
  const payload = JSON.stringify({
    itemTabs: state.itemTabs,
    activeItemId: state.activeItemId,
    dockView: state.dockView,
  });
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    persistTimer = null;
    api.setSetting(TAB_SET_KEY, payload).catch(() => {});
  }, 250);
}

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

/** Patch one delegated child's live record (`SUB-UI-1`). Silently does nothing
 * for a run this session never saw — a stale event is not worth inventing a row
 * the user cannot open. */
function patchSubRun(set: StoreSet, runId: string, patch: Partial<SubRun>) {
  set((s) =>
    s.subRuns[runId]
      ? { subRuns: { ...s.subRuns, [runId]: { ...s.subRuns[runId], ...patch } } }
      : {}
  );
}

/**
 * Fold one event from a delegated child into that child's own record
 * (`SUB-UI-1`).
 *
 * Deliberately narrow: a child's steps, prose and permission prompts are what
 * the Fleet card and the Agents tab show. Its artifacts, files and browsing
 * belong to *its* conversation, and the Agents tab reads them from there — if
 * they were folded in here they would land on the lead's message and appear to
 * be the lead's own work.
 */
export function applySubEvent(set: StoreSet, runId: string, event: api.AgentEvent) {
  set((s) => {
    const run = s.subRuns[runId];
    if (!run) return {};
    const steps = run.steps;
    let next: SubRun | null = null;
    switch (event.type) {
      case "run_started":
        // `SUB-12`: a background child announces itself when the pool finally
        // gives it a slot. This is the only signal that it stopped queueing.
        next = { ...run, status: "running", startedAt: run.startedAt || Date.now() };
        break;
      case "token":
        next = { ...run, text: run.text + event.text };
        break;
      case "steps_parallel":
        next = {
          ...run,
          parallelPending: {
            ...(run.parallelPending ?? {}),
            ...Object.fromEntries(event.ids.map((id) => [id, event.ids[0]])),
          },
        };
        break;
      case "step_start": {
        const group = run.parallelPending?.[event.id];
        next = {
          ...run,
          steps: [
            ...steps,
            {
              id: event.id,
              verb: event.verb,
              target: event.target,
              status: "running" as const,
              ...(group ? { parallelGroup: group } : {}),
              ...(event.parent ? { nestedUnder: event.parent } : {}),
            },
          ],
        };
        break;
      }
      case "task_started":
      case "task_output":
      case "task_ended":
        next = { ...run, steps: steps.map((st) => (st.id === event.id ? applyTaskEvent(st, event) : st)) };
        break;
      case "kept_result":
        next = {
          ...run,
          steps: steps.map((st) =>
            st.id === event.id
              ? { ...st, kept: { reference: event.reference, bytes: event.bytes, text: event.text } }
              : st
          ),
        };
        break;
      case "step_done":
        next = {
          ...run,
          steps: steps.map((st) =>
            st.id === event.id
              ? { ...st, status: "done" as const, result: event.result ?? undefined }
              : st
          ),
        };
        break;
      case "step_error":
        next = {
          ...run,
          steps: steps.map((st) =>
            st.id === event.id
              ? { ...st, status: "error" as const, result: `— ${event.error}` }
              : st
          ),
        };
        break;
      case "steered":
        next = { ...run, steerPending: false };
        break;
      case "permission":
        // `SUB-UI-4`: the panel must say which agent is asking. Two children can
        // ask at once, so these queue rather than replace.
        return {
          pendingPermissions: [...s.pendingPermissions, event.request],
          permissionAgents: { ...s.permissionAgents, [event.request.id]: run.agent },
        };
      default:
        return {};
    }
    return { subRuns: { ...s.subRuns, [runId]: next } };
  });
}

/** `COD-UI-2`: fold one task event into the step that ran the task. Shared by a
 * lead's own timeline and a child's, so a build reads the same in both. */
export function applyTaskEvent(
  step: AgentStep,
  event: Extract<api.AgentEvent, { type: "task_started" | "task_output" | "task_ended" }>
): AgentStep {
  switch (event.type) {
    case "task_started":
      return {
        ...step,
        task: { name: event.task, argv: event.argv, cwd: event.cwd, kind: event.kind },
      };
    case "task_output":
      return step.task ? { ...step, task: { ...step.task, lastLine: event.line } } : step;
    case "task_ended":
      return {
        ...step,
        task: {
          ...(step.task ?? { name: step.target, argv: [], cwd: "", kind: "other" }),
          outcome: event.outcome,
          exitCode: event.exit_code,
          durationMs: event.duration_ms,
          diagnostics: event.diagnostics,
          tail: event.tail,
        },
      };
  }
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
 * REF-3: leaving a conversation is when it becomes reviewable — it's finished
 * enough to learn from, and the user isn't waiting on anything.
 *
 * Every path that leaves the active conversation has to go through here, not
 * just `setActiveConversation`: starting a new chat is a far commoner way to
 * walk away from one, and for a long time it didn't reflect at all, which is
 * most of why so few conversations were ever digested.
 *
 * `nextId` is the conversation being moved to, so re-selecting the active one
 * (a Rail click on the current chat, returning from Settings) isn't mistaken
 * for leaving it. Omit it when nothing is being opened in its place.
 *
 * Fire-and-forget: reflection must never sit in the navigation path.
 */
function reflectOnLeaving(get: () => AppState, nextId?: string) {
  const state = get();
  const leaving = state.conversations.find((c) => c.id === state.activeConversationId);
  if (
    leaving &&
    leaving.id !== nextId &&
    !leaving.reflectedAt &&
    leaving.messages.length >= REFLECT_MIN_MESSAGES &&
    state.autoReflect &&
    api.inTauri()
  ) {
    state.reflectConversation(leaving.id).catch(() => {});
  }
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
    /** The optimistic message `current` was made from. It is already in the
     * transcript by now, and sending it as history too would put the user's
     * words in front of the model twice (the loop merges the two turns). */
    currentId: string;
    model: Model;
  }
): Promise<api.ChatTurnMessage[]> {
  const { convId, current, model } = opts;
  const conv = get().conversations.find((c) => c.id === convId);
  const keepRecent = conv?.workspace ? KEEP_RECENT_WORKSPACE : KEEP_RECENT;

  /** History after the summary boundary — the turns still sent verbatim. */
  const priorFrom = (boundaryId: string | null | undefined) => {
    const all = (conv?.messages ?? []).filter((m) => m.id !== opts.currentId && m.text.trim().length > 0);
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
    /** `HRN-UI-5`: continue the conversation's last run instead of starting a
     * new one. `turns` is then unused — the transcript comes from the session
     * log, which is the only place the interrupted run's tool results survive. */
    resume?: boolean;
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
  /** `PRJ-UI-3`: whether this run has changed a file yet. The first change
   * moves the sidebar to Changes; later ones leave it where the user put it. */
  let editedYet = false;
  /** `SUB-UI-1`: children this turn started, in the order it asked for them. */
  const subRunIds: string[] = [];
  /** `HRN-UI-2`: step id -> the parallel batch it belongs to. Announced before
   * the steps themselves arrive, so it is held here until they do. */
  const parallelOf: Record<string, string> = {};
  /** `HRN-3`: set by `run_ended`, written to the row when the turn finalizes. */
  let stopReason: api.StopReason | undefined;
  /** `PLN-UI-1`: the plan this run is working to, as it last told us. Held here
   * as well as on the turn so it can be written to the row when the turn
   * finalizes (`PLN-UI-5`). */
  let plan: api.PlanView | undefined;
  /** `HRN-UI-5`: the backend found no log to pick up. Not an error — the run
   * predates the session log, or finished cleanly and has nothing left to do. */
  let nothingToResume = false;
  /** Where this turn's events come from. Resuming is the same stream with the
   * same handling; the only difference is that the transcript is rebuilt in
   * Rust from the session log instead of being assembled here. */
  const startRun = opts.resume
    ? async (onEvent: (e: api.AgentEvent) => void, runOpts: api.RunOptions) => {
        nothingToResume = !(await api.resumeRun(convId, onEvent, runOpts));
      }
    : (onEvent: (e: api.AgentEvent) => void, runOpts: api.RunOptions) =>
        api.agentChat(convId, turns, onEvent, runOpts);
  try {
    await startRun(
      (e) => {
        switch (e.type) {
          case "token":
            acc += e.text;
            patchAssistant(set, convId, assistantId, { text: acc, streaming: true });
            // The model has started speaking, so whatever it was thinking is
            // over. Clearing here keeps the indicator honest without needing a
            // second event to say "done thinking".
            set((s) =>
              s.activeRun && s.activeRun.thinking ? { activeRun: { ...s.activeRun, thinking: "" } } : {}
            );
            break;
          case "thinking":
            set((s) =>
              s.activeRun?.runId === e.run_id
                ? { activeRun: { ...s.activeRun, thinking: s.activeRun.thinking + e.text } }
                : {}
            );
            break;
          case "run_started":
            set(() => ({
              activeRun: {
                runId: e.run_id,
                convId,
                step: 0,
                maxSteps: e.max_steps,
                startedAt: Date.now(),
                contextTokens: 0,
                contextWindow: e.context_window,
                thinking: "",
                // A resume announces the plan it is continuing right after
                // this, so the card is never blank while the run is live.
                plan,
              },
            }));
            break;
          case "run_progress":
            set((s) =>
              s.activeRun?.runId === e.run_id
                ? {
                    activeRun: {
                      ...s.activeRun,
                      step: e.step,
                      maxSteps: e.max_steps,
                      contextTokens: e.context_tokens,
                      // Each step thinks afresh. Without this the indicator
                      // would read as one think growing across the whole run.
                      thinking: "",
                    },
                  }
                : {}
            );
            break;
          case "run_ended":
            // `HRN-3`: keep the reason on the turn. A run that hit its step
            // limit still has an answer worth reading; it just isn't the one
            // it meant to give, and the stream says so rather than passing a
            // stump off as finished.
            if (e.stop_reason !== "completed") {
              stopReason = e.stop_reason;
              patchAssistant(set, convId, assistantId, { stopReason: e.stop_reason });
            }
            // `PLN-5`: the plan as the run left it. This is what lets a turn
            // that stopped at its step limit show which items it never reached
            // rather than only that it stopped.
            if (e.plan) {
              plan = e.plan;
              patchAssistant(set, convId, assistantId, { plan: e.plan });
            }
            set((s) => (s.activeRun?.runId === e.run_id ? { activeRun: null } : {}));
            break;
          case "plan":
            // `PLN-UI-1`: the plan the model just wrote or revised. It goes on
            // the turn, so the card renders it above the timeline, and on the
            // run, so the meter can name the item being worked on.
            plan = e.plan;
            patchAssistant(set, convId, assistantId, { plan: e.plan });
            set((s) =>
              s.activeRun?.runId === e.run_id
                ? { activeRun: { ...s.activeRun, plan: e.plan } }
                : {}
            );
            break;
          case "steered":
            // The run picked up something typed at it mid-flight. Settle the
            // optimistic mark on the newest pending user turn.
            set((s) => ({
              conversations: s.conversations.map((c) =>
                c.id !== convId
                  ? c
                  : {
                      ...c,
                      messages: c.messages.map((m) =>
                        m.role === "user" && m.midRun === "pending" && m.text === e.text
                          ? { ...m, midRun: "delivered" as const }
                          : m
                      ),
                    }
              ),
            }));
            break;
          case "sub_spawned": {
            // A child started. The card exists from this moment, not from its
            // first token — an agent you cannot see is the failure this whole
            // feature exists to avoid.
            subRunIds.push(e.run_id);
            set((s) => ({
              subRuns: {
                ...s.subRuns,
                [e.run_id]: {
                  runId: e.run_id,
                  conversationId: e.conversation_id,
                  parentConversationId: convId,
                  agent: e.agent,
                  task: e.task,
                  status: "running",
                  steps: [],
                  text: "",
                  startedAt: Date.now(),
                },
              },
            }));
            patchAssistant(set, convId, assistantId, { subRunIds: [...subRunIds] });
            break;
          }
          case "sub":
            applySubEvent(set, e.run_id, e.event);
            break;
          case "sub_ended":
            patchSubRun(set, e.run_id, {
              status: e.status,
              stopReason: e.stop_reason,
              // The report the lead was handed is the whole result — better
              // than whatever prose happened to stream before it.
              ...(e.summary ? { text: e.summary } : {}),
              endedAt: Date.now(),
              ms: e.ms,
              steerPending: false,
            });
            break;
          case "steps_parallel":
            // `HRN-4`: these are about to start together. The batch is named
            // after its first step, which is stable and needs no counter.
            for (const id of e.ids) parallelOf[id] = e.ids[0];
            break;
          case "step_start":
            steps.push({
              id: e.id,
              verb: e.verb,
              target: e.target,
              status: "running",
              ...(parallelOf[e.id] ? { parallelGroup: parallelOf[e.id] } : {}),
              ...(e.parent ? { nestedUnder: e.parent } : {}),
            });
            patchAssistant(set, convId, assistantId, { steps: [...steps] });
            break;
          case "task_started":
          case "task_output":
          case "task_ended": {
            // `COD-UI-2`: a task's progress hangs off its own step.
            const at = steps.findIndex((x) => x.id === e.id);
            if (at !== -1) {
              steps[at] = applyTaskEvent(steps[at], e);
              patchAssistant(set, convId, assistantId, { steps: [...steps] });
            }
            break;
          }
          case "kept_result": {
            // `HRN-8`: the model got a preview. The user gets all of it, behind
            // the same disclosure `Code` and `Recall` already use.
            const s = steps.find((x) => x.id === e.id);
            if (s) s.kept = { reference: e.reference, bytes: e.bytes, text: e.text };
            patchAssistant(set, convId, assistantId, { steps: [...steps] });
            break;
          }
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
            // Same turn, same artifact, twice (made then fixed) is one chip.
            if (!artifactIds.includes(e.id)) artifactIds.push(e.id);
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
              // `ART-4`: the agent can now revise an artifact in place, which
              // arrives on this same event with an id already in the list.
              // Appending blindly would leave the panel showing two Pac-Mans,
              // the broken one first. Replace in place instead, keeping its
              // position so the thing the user is looking at doesn't jump.
              const at = existing.findIndex((a) => a.id === e.id);
              const next =
                at === -1
                  ? [...existing, artifact]
                  : existing.map((a, i) =>
                      // Keep what belongs to the row rather than to this
                      // revision: an edited artifact was still made when it
                      // was made, and is still saved where it was saved.
                      i === at
                        ? { ...artifact, created_at: a.created_at, saved_path: a.saved_path }
                        : a
                    );
              return {
                artifacts: { ...st.artifacts, [convId]: next },
                dockOpen: isMedia ? st.dockOpen : true,
              };
            });
            // `SHL-23`: the agent points the sidebar at what it made; it does
            // not open the artifact as a tab. A tab would take the chat you
            // are typing in off the screen, and the agent may never do that.
            if (!isMedia) get().setDockView("artifacts");
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
            get().refreshChanges(convId).catch(() => {});
            // `SHL-23`/`PRJ-UI-3`: the first edit of a run points the sidebar at
            // the patch. It never opens a diff tab — only the user does that,
            // because a tab would take the chat off the screen.
            if (!editedYet) {
              editedYet = true;
              const conv = get().conversations.find((c) => c.id === convId);
              if (conv?.folderPath && convId === get().activeConversationId) get().setDockView("changes");
            }
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
    if (nothingToResume) {
      acc =
        acc ||
        "I have no record of that run to pick up — it finished before I started keeping one. Ask me again and I will start it fresh.";
      patchAssistant(set, convId, assistantId, { text: acc, streaming: false });
    }
  } catch (err) {
    acc = acc || `That didn't work: ${String(err)}`;
    patchAssistant(set, convId, assistantId, { text: acc, streaming: false });
  } finally {
    // Back to resting unless a self-process is still working (PRES-1). The run
    // is dropped here as well as on `run_ended`, so a stream that dies without
    // a closing event can't leave the composer thinking it can still steer.
    set((st) => ({
      busy: false,
      activeRun: null,
      presence: st.reflectingIds.length ? "reflecting" : "idle",
      // A steer the run never got round to reading stops claiming to be in
      // flight — it stays in the transcript as an ordinary unanswered turn.
      conversations: st.conversations.map((c) =>
        c.id !== convId
          ? c
          : {
              ...c,
              messages: c.messages.map((m) =>
                m.midRun === "pending" ? { ...m, midRun: undefined } : m
              ),
            }
      ),
    }));
    try {
      const stepsJson = steps.length ? JSON.stringify(steps) : undefined;
      const contextJson = opts.contextRefs ? JSON.stringify(opts.contextRefs) : undefined;
      const planJson = plan?.items.length ? JSON.stringify(plan) : undefined;
      await api.finalizeMessage(
        persistedAssistantId,
        acc,
        stepsJson,
        contextJson,
        stopReason,
        planJson
      );
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

/** How long after launch the catch-up pass runs. Long enough that the models
 * list, the engine and the first paint are all settled — a backlog that has sat
 * there for weeks can wait another half-minute, and must not compete with the
 * user's first message. */
const CATCH_UP_DELAY_MS = 30_000;

/**
 * REF-3b: digest the conversations nobody ever left *through the door* — closed
 * with the app, or abandoned back when only a chat switch triggered reflection.
 *
 * Deliberately not run on window close: reflection is two model calls per
 * lesson, and an app being quit is the worst possible place to start one. The
 * backlog is drained a couple at a time at launch instead, where it can take as
 * long as it needs.
 */
function scheduleCatchUpReflection(get: () => AppState) {
  if (!api.inTauri()) return;
  setTimeout(async () => {
    if (!get().autoReflect || get().busy) return;
    try {
      const digested = await api.catchUpReflection(cloudTarget());
      if (digested > 0) {
        get().refreshMemoryContext();
        get().refreshSelf();
        get().refreshChangeProposals();
      }
    } catch {
      /* the backlog keeps; try again next launch */
    }
  }, CATCH_UP_DELAY_MS);
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
  // `SUB-10`/`SUB-12`: a background child outlives the turn that started it, so
  // its events cannot arrive on that turn's channel — by the time it takes its
  // second step, there is no channel. They come over the app bus instead, in
  // exactly the shape a live child's events have, so the Fleet card, the Agents
  // tab and the permission panel all keep working with no second code path.
  api.onAppEvent<api.AgentEvent>("poiesis-agent-sub", (e) => {
    if (e.type === "sub") {
      applySubEvent(set, e.run_id, e.event);
      return;
    }
    if (e.type !== "sub_ended") return;
    patchSubRun(set, e.run_id, {
      status: e.status,
      stopReason: e.stop_reason,
      ...(e.summary ? { text: e.summary } : {}),
      endedAt: Date.now(),
      ms: e.ms,
      steerPending: false,
    });
    // Say so. An agent finishing quietly, minutes after the reply that started
    // it, is work the person never learns happened.
    const run = get().subRuns[e.run_id];
    if (!run) return;
    const verb = e.stop_reason === "completed" ? "finished" : "stopped";
    set(() => ({ agentDoneToast: `The ${run.agent} agent ${verb}. Its report is in the turn that started it.` }));
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
        : "↻ I couldn't keep my runtime alive — I've stopped trying. Check the Runtime page.",
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

/** `PRJ-7`: what this project is, carried by every session in it.
 *
 * Sits after SOUL.md because standing instructions the user approved apply
 * everywhere and this applies only here. Mirrors `project_block` in
 * `context.rs` — the two are compared byte for byte by `CTX-4`. */
function projectBlock(name: string | undefined, instructions: string | undefined): string {
  const n = name?.trim();
  const t = instructions?.trim();
  if (!n || !t) return "";
  const text = t.length > PROJECT_INSTRUCTIONS_CAP ? `${t.slice(0, PROJECT_INSTRUCTIONS_CAP)}…` : t;
  return `## Project: ${n} (instructions for this project; the persona/system prompt above still governs voice, format and depth)\n${text}`;
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

/** The standing guidance only sent when the model can actually call tools.
 *
 * `PLN-3`: the planning sentence rides on the end of the same block — it is the
 * same kind of thing (how to go about the work), and `never` must be able to
 * remove it without leaving a gap. Mirrors `tool_guidance_block` in
 * `agent/context.rs`; the golden gate (`CTX-4`) holds the two to the byte. */
function toolGuidanceBlock(planMode: PlanMode | undefined): string {
  const out = `${SURFACE_GUIDANCE}\n\n${BLOCK_GUIDANCE}\n\n${PLAN_FIRST_GUIDANCE}`;
  const planning = planGuidance(planMode);
  return planning ? `${out}\n${planning}` : out;
}

/** `PLN-3`: the user's override of the model's judgement about planning.
 * Mirrors `agent::plan::PlanMode`. */
export type PlanMode = "always" | "auto" | "never";
export const PLAN_MODE_KEY = "agent.plan_mode";

/** One sentence, and one sentence only. A model that writes junk plans will not
 * be argued out of it by a paragraph — `never` is the answer for that model.
 * Word for word `PlanMode::guidance` in `agent/plan.rs`. */
export function planGuidance(mode: PlanMode | undefined): string {
  if (mode === "never") return "";
  if (mode === "always") {
    return "Before your first tool call, write the plan with the `plan` tool, then keep it up to date as you work.";
  }
  return "When a request has several distinct parts, or will take more than a few steps, write a short plan with the `plan` tool before you start and keep it up to date as you work; otherwise just do the work.";
}

/**
 * MEM-COLD: the instruction that makes durable memory actually happen.
 *
 * Without this the only thing telling the model to save is one tool
 * description buried among forty others, and `memoryIndexBlock` goes silent at
 * zero facts — so an empty memory never mentions memory, the model never saves,
 * and it stays empty forever. This block is therefore sent whenever the Memory
 * toolset is on, *especially* when there is nothing remembered yet.
 */
function memoryGuidanceBlock(hasFacts: boolean): string {
  const lines = [
    "## Remembering",
    'You keep durable notes about the user across conversations with the `memory` tool. When the user says something that will still be true next week and would change how you answer later, call memory(op:"save") in the same turn — do not wait to be asked, and do not announce it at length; the save shows up in their timeline on its own.',
    "Worth saving: how they want you to work (tone, length, format, language), tools/stacks/services they use, what they're building and why, stable personal or professional facts, standing decisions they've made.",
    "Never save: task state, one-off requests, anything you inferred rather than heard, or anything they haven't actually confirmed. When in doubt, don't.",
    "One fact per save, in their own terms, with a slug you would search for later.",
  ];
  if (!hasFacts) {
    lines.push(
      "You have not saved anything about this user yet, so the bar for the first few notes is simply: would knowing this next month make you better here? If so, save it.",
    );
  }
  return lines.join("\n");
}

export interface ComposePromptOpts {
  conv: Conversation | undefined;
  sessionState: Record<string, unknown> | undefined;
  toolsEnabled: boolean;
  surface?: BlockView;
  /** The durable self (MEM-3). Omitted when the Memory toolset is off. */
  memory?: api.MemoryContext;
  /** Is the Memory toolset on — i.e. can the model actually call `memory`?
   * Distinct from `memory` being present: with the toolset off, soul and the
   * synthesis are still injected (`recallForPrompt`), just without an index. */
  memoryEnabled?: boolean;
  /** 7-day tool reliability for this model (HEAL-2). */
  toolHealth?: api.ToolHealth[];
  /** Discovered Agent Skills (SKL-2), for the "Skills available" block. */
  skills?: api.SkillView[];
  /** `PLN-3`: whether this turn is told to plan first. Absent means *when it
   * helps*, which is what an unset setting means. */
  planMode?: PlanMode;
  /** `PRJ-7`: the project this session is in, and what it says to do. Both or
   * neither — a name with no instructions has nothing to inject, and
   * instructions with no name have nothing to attribute them to. */
  projectName?: string;
  projectInstructions?: string;
}

/** `PRJ-7`: the project half of a turn's standing context, read beside the
 * persona because both answer the same question — what this turn starts from.
 * Mirrors `from_db` in `context.rs`. */
function projectPrompt(
  state: AppState,
  convId: string
): { projectName?: string; projectInstructions?: string } {
  const projectId = state.conversations.find((c) => c.id === convId)?.projectId;
  const project = projectId ? state.projects.find((p) => p.id === projectId) : undefined;
  if (!project?.instructions) return {};
  return { projectName: project.name, projectInstructions: project.instructions };
}

/** `PRJ-7`: same order as the skills block, and well under any model's
 * patience. Instructions past this are clipped rather than dropped — a project
 * whose instructions vanished for being long would be worse. */
export const PROJECT_INSTRUCTIONS_CAP = 4000;

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
  // `PRJ-7`: after the standing instructions, which apply everywhere, and
  // before the memory index, which this narrows the meaning of.
  const projectText = projectBlock(opts.projectName, opts.projectInstructions);
  if (projectText) out += `\n\n${projectText}`;
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
    out += `\n\n${toolGuidanceBlock(opts.planMode)}`;
    // MEM-COLD: last of the guidance, and sent even at zero facts — that is
    // precisely the state it exists to break out of.
    if (opts.memoryEnabled) {
      // `fact_count`, not the index text: scoped recall (SCP) can leave the
      // index empty on a turn where facts do exist, and that is not a cold start.
      out += `\n\n${memoryGuidanceBlock((opts.memory?.fact_count ?? 0) > 0)}`;
    }
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

/** `SHL-24`: what the item pane holds right now, derived in one place so the
 * strip, the pane and the shell's column width cannot disagree about it.
 *
 * Only the live chat's items, because the pane sits beside that chat's
 * conversation and a tab that silently switched which chat was live is the one
 * move the shell must never make. Items belonging to other chats stay in the
 * store and come back when you do.
 *
 * `activeKey` is `null` when the conversation itself is what's showing, which
 * is a real state and not a missing one (`SHL-27`): the session tab is
 * selected, and the item tabs are still open behind it. It also covers an
 * `activeItemId` naming something that is gone or belongs to another chat,
 * which lands on the same place — the conversation, which always exists. */
export function useLiveItems(): { items: ItemRef[]; activeKey: string | null } {
  const convId = useAppStore((s) => s.activeConversationId);
  const itemTabs = useAppStore((s) => s.itemTabs);
  const activeItemId = useAppStore((s) => s.activeItemId);
  return useMemo(() => {
    const items = itemTabs.filter((t) => t.conversationId === convId);
    const activeKey = items.some((t) => itemKey(t) === activeItemId) ? activeItemId : null;
    return { items, activeKey };
  }, [itemTabs, convId, activeItemId]);
}

/** SMP-1b: expert-only surfaces render `null` when this is false — no
 * greying out, no "upgrade to see" affordance. */
export function useExpert(): boolean {
  return useAppStore((s) => s.expert);
}
