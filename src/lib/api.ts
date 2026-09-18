// Typed wrappers over Tauri's `invoke` + streaming channels. The frontend can
// also run in a plain browser (vite dev without Tauri) for fast UI iteration;
// `inTauri()` lets callers fall back to mock data in that case.

import { Channel, invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AgentStep, Provenance } from "./types";

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri()) {
    throw new Error(`Tauri command "${cmd}" called outside the desktop app`);
  }
  return tauriInvoke<T>(cmd, args);
}

// ---- backend row shapes (snake_case, as serialized by serde) ----

export interface DbConversation {
  id: string;
  title: string;
  model_id: string | null;
  persona_id: string | null;
  overrides_json: string | null;
  workspace: boolean;
  created_at: number;
  updated_at: number;
  /** Rolling summary of the older turns (CTX-3), or null before compaction. */
  summary: string | null;
  /** Newest message covered by `summary`; later turns are sent verbatim. */
  summary_upto_message_id: string | null;
  /** When the agent last reflected on this conversation (REF-2), or null. */
  reflected_at: number | null;
  /** The real folder on disk this conversation works in, or null. */
  folder_path: string | null;
  /** How much the agent may change inside it: read-only | confirm | auto. */
  folder_trust: string;
  /**
   * `SUB-3`: set when this conversation is a delegated child's workspace. The
   * Rail keeps these out of the top-level list — they belong to the turn that
   * started them, not beside it.
   */
  parent_conversation_id: string | null;
  /**
   * `PRJ-1`: the project this conversation belongs to, or null for a loose
   * chat. When it is set the project owns the folder and the trust level, and
   * the two fields above are the fallback rather than the answer.
   */
  project_id?: string | null;
}

/** `PRJ-1`: a working directory and the sessions that happened in it. */
export interface DbProject {
  id: string;
  name: string;
  /** Canonical, and unique across projects. Null when the project is not
   * about a directory (`PRJ-1a`). */
  root_path: string | null;
  /** `PRJ-7`: free text carried into every session in this project. */
  instructions: string | null;
  /** read-only | confirm | auto, granted once for the folder. */
  trust: string;
  /** `COD-7`: off | ask | allow, or inherit for the Settings default. */
  exec_policy: string;
  /** `COD-1` detection result, as stored JSON. */
  card_json: string | null;
  card_built_at: number | null;
  /** `COD-7`/`COD-8`: what "always allow" was said to in this project. */
  allow_json?: string | null;
  /** `SHL-17`'s scope seam: the open tab set for this project. */
  tabs_json: string | null;
  archived: boolean;
  created_at: number;
  updated_at: number;
}

export interface DbAttachment {
  id: string;
  kind: string;
  name: string;
  path: string;
  /** The artifact this attachment renders, when one backs it (`ART-2`). */
  artifact_id?: string | null;
}
export interface DbMessage {
  id: string;
  conversation_id: string;
  role: "user" | "assistant";
  content: string;
  model_name: string | null;
  model_provenance: string | null;
  steps_json: string | null;
  /** `HRN-3`: why the run behind this turn stopped. Null on user turns and on
   * anything written before the column existed, both of which read as done. */
  stop_reason: StopReason | null;
  /** `PLN-UI-5`: the plan the run behind this turn worked to, as stored JSON.
   * Null for a turn that never wrote one, which is most of them. */
  plan_json: string | null;
  created_at: number;
  attachments?: DbAttachment[];
}

// ---- meta ----

export const getAppVersion = () => invoke<string>("app_version");

// ---- conversations & messages (Phase 2) ----

export const listConversations = () => invoke<DbConversation[]>("list_conversations_cmd");

export const createConversation = (title?: string, modelId?: string, workspace?: boolean) =>
  invoke<DbConversation>("create_conversation_cmd", { title, modelId, workspace });

export const renameConversation = (id: string, title: string) =>
  invoke<void>("rename_conversation_cmd", { id, title });

/** Context window of the loaded local engine, or null when none is loaded (CTX-1). */
export const getContextBudget = () => invoke<number | null>("get_context_budget_cmd");

export const setConversationWorkspace = (id: string, workspace: boolean) =>
  invoke<void>("set_conversation_workspace_cmd", { id, workspace });

export const deleteConversation = (id: string) =>
  invoke<void>("delete_conversation_cmd", { id });

export const listMessages = (conversationId: string) =>
  invoke<DbMessage[]>("list_messages_cmd", { conversationId });

export const appendMessage = (args: {
  conversationId: string;
  role: "user" | "assistant";
  content: string;
  modelName?: string;
  modelProvenance?: Provenance;
  stepsJson?: string;
  attachments?: { kind: string; name: string; path: string; artifact_id?: string }[];
}) =>
  invoke<DbMessage>("append_message_cmd", {
    conversationId: args.conversationId,
    role: args.role,
    content: args.content,
    modelName: args.modelName,
    modelProvenance: args.modelProvenance,
    stepsJson: args.stepsJson,
    attachments: args.attachments,
  });

export const finalizeMessage = (
  id: string,
  content: string,
  stepsJson?: string,
  contextJson?: string,
  stopReason?: StopReason,
  /** `PLN-UI-5`: the plan this turn worked to, so it survives a reload. */
  planJson?: string
) =>
  invoke<void>("finalize_message_cmd", {
    id,
    content,
    stepsJson,
    contextJson,
    stopReason,
    planJson,
  });

/** One conversation's best-matching message. The matched words in `snippet`
 * are fenced by ``…``; see `splitSnippet`. */
export interface MessageHit {
  conversation_id: string;
  snippet: string;
}

export const searchMessages = (query: string) =>
  invoke<MessageHit[]>("search_messages_cmd", { query });

export const listArtifacts = (conversationId: string) =>
  invoke<Artifact[]>("list_artifacts_cmd", { conversationId });

export const listAllArtifacts = () => invoke<Artifact[]>("list_all_artifacts_cmd");

export const getSetting = (key: string) => invoke<string | null>("get_setting_cmd", { key });
export const setSetting = (key: string, value: string) =>
  invoke<void>("set_setting_cmd", { key, value });

// ---- runtime (Phase 1) ----

export interface GpuInfo {
  vendor: string;
  name: string;
  vram_mb: number | null;
  driver_version: string | null;
}
export interface HardwareProfile {
  cpu: { brand: string; physical_cores: number; avx2: boolean; avx512: boolean };
  ram_mb: number;
  gpus: GpuInfo[];
}
export interface RuntimeSelection {
  backend: string;
  alternates: string[];
  rationale: string;
  build_tag: string;
}
export interface EngineStatus {
  running: boolean;
  port: number | null;
  model_path: string | null;
  /** GRM-1/2: engine enforces structured tool calls natively (`--jinja`). */
  structured_tool_output?: boolean;
  /** HEAL-1: times the watchdog restarted this engine since launch. */
  restarts_session?: number;
  /** HEAL-1: self-repair hit its rolling-hour limit and stopped trying. */
  self_heal_gave_up?: boolean;
}
export interface DownloadProgress {
  received: number;
  total: number | null;
  label: string;
}

export const detectHardware = () => invoke<HardwareProfile>("detect_hardware_cmd");
export const recommendRuntime = () => invoke<RuntimeSelection>("recommend_runtime_cmd");
export const runtimeStatus = () => invoke<EngineStatus>("runtime_status_cmd");
export const stopEngine = () => invoke<void>("stop_engine_cmd");

// ---- engine view (runtime management) ----

export interface BackendOption {
  backend: string;
  label: string;
  recommended: boolean;
  installed: boolean;
}
export interface RuntimeOverview {
  hardware: HardwareProfile;
  recommended: RuntimeSelection;
  active_backend: string;
  override_backend: string | null;
  installed: boolean;
  install_path: string | null;
  engine: EngineStatus;
  options: BackendOption[];
}
export interface UpdateInfo {
  current: string;
  latest: string;
  update_available: boolean;
}

export const runtimeOverview = () => invoke<RuntimeOverview>("runtime_overview_cmd");
export const setBackendOverride = (backend: string | null) =>
  invoke<void>("set_backend_override_cmd", { backend });
export const checkRuntimeUpdate = () => invoke<UpdateInfo>("check_runtime_update_cmd");

export function startEngine(onProgress: (p: DownloadProgress) => void): Promise<EngineStatus> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<EngineStatus>("start_engine_cmd", { onProgress: ch });
}

export function ensureRuntime(onProgress: (p: DownloadProgress) => void): Promise<string> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<string>("ensure_runtime_cmd", { onProgress: ch });
}

export function loadModel(
  args: { modelPath: string; ctxSize?: number; nGpuLayers?: number },
  onProgress: (p: DownloadProgress) => void
): Promise<EngineStatus> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<EngineStatus>("load_model_cmd", {
    modelPath: args.modelPath,
    ctxSize: args.ctxSize,
    nGpuLayers: args.nGpuLayers,
    onProgress: ch,
  });
}

// ---- marketplace + library (Phase 3) ----

export type Fit = "great" | "slow" | "wont-fit";

/** Shared by both catalogs — the same verdict, worded the same way, whether
 * the download is a GGUF or a diffusion checkpoint. */
export const FIT_LABEL: Record<Fit, string> = {
  great: "Runs great",
  slow: "Runs slowly",
  "wont-fit": "Too big for this PC",
};

export interface CatalogEntry {
  id: string;
  name: string;
  description: string;
  quant: string;
  size_mb: number;
  vision: boolean;
  url: string;
  source: string;
  license: string | null;
  fit: Fit;
  speed: string;
}

export interface HfModelSummary {
  id: string;
  downloads: number | null;
  likes: number | null;
}

export interface ModelEntry {
  id: string;
  name: string;
  path: string;
  quant: string | null;
  size_bytes: number | null;
  vision: boolean;
  /** "chat" | "embed" | "rerank" (schema v7) — one library, three engines. */
  role: string;
  is_default: boolean;
  added_at: number;
}

export const recommendedCatalog = () => invoke<CatalogEntry[]>("recommended_catalog_cmd");
export const searchHuggingface = (query: string) =>
  invoke<HfModelSummary[]>("search_huggingface_cmd", { query });
export const listRepoFiles = (repo: string) =>
  invoke<CatalogEntry[]>("list_repo_files_cmd", { repo });
export const listGithubModels = (ownerRepo: string) =>
  invoke<CatalogEntry[]>("list_github_models_cmd", { ownerRepo });

export const listModels = () => invoke<ModelEntry[]>("list_models_cmd");
export const deleteModelEntry = (id: string) => invoke<void>("delete_model_cmd", { id });
export const setDefaultModel = (id: string) => invoke<void>("set_default_model_cmd", { id });
export const addLocalModel = (path: string, name: string, quant?: string, vision?: boolean) =>
  invoke<ModelEntry>("add_local_model_cmd", { path, name, quant, vision });

export function downloadModel(
  args: { url: string; name: string; quant?: string; vision?: boolean },
  onProgress: (p: DownloadProgress) => void
): Promise<ModelEntry> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<ModelEntry>("download_model_cmd", {
    url: args.url,
    name: args.name,
    quant: args.quant,
    vision: args.vision,
    onProgress: ch,
  });
}

// ---- streaming chat (Phase 1 proxy ↔ Phase 2 persistence) ----

export type StreamEvent =
  | { type: "token"; text: string }
  | { type: "toolcall"; raw: string }
  | { type: "done" }
  | { type: "error"; message: string }
  | { type: "cancelled" };

export type ContentPart =
  | { type: "text"; text: string }
  | { type: "image_url"; image_url: { url: string } };

export interface ChatTurnMessage {
  role: "system" | "user" | "assistant";
  /** Plain string, or content-part array for multimodal/vision input. */
  content: string | ContentPart[];
}

/** Stream a completion from the loaded local engine, invoking `onEvent` per chunk. */
export function chat(
  messages: ChatTurnMessage[],
  onEvent: (e: StreamEvent) => void,
  temperature?: number
): Promise<void> {
  const ch = new Channel<StreamEvent>();
  ch.onmessage = onEvent;
  return invoke<void>("chat_cmd", { messages, temperature, onEvent: ch });
}

export const stopChat = () => invoke<void>("stop_chat_cmd");

// ---- agent loop + permissions (Phase 4) ----

/** Why a run stopped (`HRN-3`). Anything but `completed` means the answer is
 * what the run had in hand, not what it set out to say. */
export type StopReason = "completed" | "aborted" | "timeout" | "max_steps" | "error";

/** What a run cost, when the provider reported it (`OBS-1`). */
export interface TurnUsage {
  prompt_tokens: number;
  output_tokens: number;
}

export type AgentEvent =
  /** `parent` is `RPC-1`: the step this one happened *inside*. A script running
   * in the sandbox can make tool calls of its own, and they belong under the
   * step that ran the script rather than beside it. */
  | { type: "step_start"; id: string; verb: string; target: string; parent?: string }
  | { type: "step_done"; id: string; result: string | null }
  | { type: "step_error"; id: string; error: string }
  | { type: "token"; text: string }
  | { type: "artifact"; id: string; title: string; kind: string; content: string; meta_json: string | null }
  | { type: "block"; id: string; message_id: string | null; kind: string; title: string; data: unknown }
  | { type: "block_update"; id: string; title: string; data: unknown }
  | { type: "state_update"; state: unknown }
  | { type: "permission"; request: PermissionRequest }
  | { type: "memory_write"; op: string; name: string; description: string; collection: string; undo_token: string }
  | { type: "recall"; id: string; matches: SearchHit[] }
  | { type: "code"; id: string; language: string; code: string }
  /** `COD-UI-2`: a project task started, finished, or printed a line. */
  | {
      type: "task_started";
      id: string;
      task: string;
      argv: string[];
      cwd: string;
      kind: TaskKind;
      timeout_secs: number;
    }
  | { type: "task_output"; id: string; line: string }
  | {
      type: "task_ended";
      id: string;
      outcome: string;
      exit_code: number | null;
      timed_out: boolean;
      cancelled: boolean;
      duration_ms: number;
      diagnostics: Diagnostic[];
      tail: string;
    }
  | { type: "untrusted"; id: string; label: string; risk: number; flags: string[]; text: string }
  | { type: "proposal"; id: string; target: string; rationale: string }
  | { type: "file_changed"; op: string; path: string; undo_token: string }
  | { type: "browser"; state: BrowserPanelState }
  | { type: "mail_sent"; to: string }
  /** `HRN-4`/`HRN-UI-2`: these steps are running at the same time, in this
   * order. Arrives before their `step_start`s. */
  | { type: "steps_parallel"; ids: string[] }
  /** `HRN-8`: this step's output was kept on disk instead of pasted into the
   * transcript. The model got a preview; the user gets all of it. */
  | { type: "kept_result"; id: string; reference: string; bytes: number; text: string }
  /** A chunk of the model's thinking. Never the answer, and never appended to
   * the message — it exists so a reasoning model's long silence reads as work
   * rather than as a hang. */
  | { type: "thinking"; run_id: string; text: string }
  | { type: "run_started"; run_id: string; max_steps: number; context_window: number | null }
  | {
      type: "run_progress";
      run_id: string;
      step: number;
      max_steps: number;
      ms: number;
      /** `OBS-3`: an estimate of what this turn is about to send. */
      context_tokens: number;
    }
  | {
      type: "run_ended";
      run_id: string;
      stop_reason: StopReason;
      steps: number;
      ms: number;
      usage: TurnUsage | null;
      /** `PLN-5`: the plan as it stood when the run ended, so a run that
       * stopped at its budget can say which items it never reached. */
      plan: PlanView | null;
    }
  /** `PLN-1`: the run wrote or revised its plan. Carries the whole plan, not a
   * patch — it is a handful of short strings, and a card rebuilt from the
   * current state cannot drift out of step with the model's copy. */
  | { type: "plan"; run_id: string; plan: PlanView }
  | { type: "steered"; run_id: string; text: string }
  | {
      type: "sub_spawned";
      run_id: string;
      conversation_id: string;
      agent: string;
      task: string;
      index: number;
    }
  /** One event from a delegated child, tagged with whose it is (`SUB-2`). */
  | { type: "sub"; run_id: string; event: AgentEvent }
  | {
      type: "sub_ended";
      run_id: string;
      status: SubRunStatus;
      stop_reason: StopReason;
      summary: string;
      steps: number;
      ms: number;
    }
  | { type: "done" }
  | { type: "cancelled" }
  | { type: "error"; message: string };

/** A workspace block row as persisted by the backend (Generative UI). */
export interface DbBlock {
  id: string;
  conversation_id: string;
  message_id: string | null;
  kind: string;
  title: string;
  data_json: string;
  state_json: string | null;
  created_at: number;
  updated_at: number;
}

export interface Artifact {
  id: string;
  conversation_id: string | null;
  title: string;
  kind: string;
  content: string;
  created_at: number;
  /** Set once the user saved this artifact into the working folder. From then on
   * it lives in the tree as a real file rather than under "Made in this chat". */
  saved_path: string | null;
  /** Provider/cost/dimensions for a generated image or video (Phase 13), as a
   * JSON string — parse with `JSON.parse` where needed. Absent on artifacts
   * built from a streamed event rather than fetched from the backend. */
  meta_json?: string | null;
  /** The artifact this one was refined from, if any. */
  parent_id?: string | null;
  /** The assistant turn that produced this artifact, so a reloaded
   * conversation can still show its inline chip in the message stream. */
  message_id?: string | null;
}

/** One hit from the agent's search over its own past (RCL-1), or — once
 * retrieved by meaning (SEM, RET) — over durable memory or an attached
 * folder. */
export interface SearchHit {
  source: "chat" | "memory" | "file";
  conversation_id: string | null;
  /** Conversation title, memory entry name, or filename. */
  title: string;
  created_at: number;
  snippet: string;
  /** Set only for `source: "memory"` — labels a lesson differently from a
   * fact (SEM-UI-1/2). `"lesson"` is exact; a
   * fact carries its own on-disk kind instead (`preference`, `decision`,
   * `project`, …), which all read as "remembered". */
  kind?: "lesson" | (string & {});
  /** Absolute file path, set only for `source: "file"` (RET-UI-1) — lets a
   * match open in the Workbench viewer instead of switching conversation. */
  path?: string | null;
}

export interface PermissionRequest {
  id: string;
  summary: string;
  path: string;
  mode: "read" | "read-write";
  /** A diff or content excerpt to review, for in-folder operation confirms. */
  diff?: string;
  /** True when this confirms an operation inside the attached working folder
   * rather than asking to widen scope — the panel shows Allow / Deny / Don't
   * ask again instead of the four-way Once / Chat / Forever choice. */
  in_folder: boolean;
  /** `BRW-3`/`SYS-1`: set for a non-filesystem capability request —
   * `"domain"` | `"screen"` | `"open-app"` — instead of a folder request.
   * `path` carries the domain/app name. */
  capability?: "domain" | "screen" | "open-app" | "task" | "command";
  /** `COD-UI-4`: for a task or command, the exact program and arguments. */
  argv?: string[];
  /** The project it runs in. */
  project?: string;
  /** How long it may run before it is stopped. */
  timeout_secs?: number;
  /** What "always allow" remembers: a task's name, or a `command argv[0]` pair. */
  remember?: string;
}

export type Decision = "deny" | "once" | "chat" | "forever";

export interface Grant {
  id: string;
  path: string;
  mode: string;
  created_at: number;
}

/** `BRW-3`/`SYS-1`: a persisted "Always allow" answer to a domain or app
 * consent prompt. */
export interface CapabilityGrant {
  id: string;
  kind: "domain" | "open-app";
  value: string;
  created_at: number;
}

/** `BRW-UI-1`: the Workbench Browser panel's live state. */
export interface BrowserPanelState {
  title: string;
  domain: string;
  screenshot: string | null;
  trail: string[];
  /** The session has ended — the panel keeps the trail and says so, rather
   *  than disappearing as if the browsing never happened. */
  closed: boolean;
}

/** `BRW-1`: a session closed itself after ten idle minutes. Nothing is
 *  streaming at that point, so it arrives as an app event, not a run event. */
export interface BrowserClosedEvent {
  conversationId: string;
}

export interface ActivityEntry {
  id: string;
  conversation_id: string | null;
  kind: string;
  detail: string;
  created_at: number;
}

/** Run an agentic turn, streaming step + token events to `onEvent`. */
export interface ChatTarget {
  provenance: "local" | "cloud" | "endpoint";
  provider?: string;
  model?: string;
}

/** What a run needs beyond its messages. Shared by `agentChat` and `resumeRun`
 * so the two cannot drift into running the same conversation differently. */
export interface RunOptions {
  temperature?: number;
  toolsEnabled?: boolean;
  target?: ChatTarget;
  assistantMessageId?: string;
}

export function agentChat(
  conversationId: string,
  messages: ChatTurnMessage[],
  onEvent: (e: AgentEvent) => void,
  opts?: RunOptions
): Promise<void> {
  const ch = new Channel<AgentEvent>();
  ch.onmessage = onEvent;
  return invoke<void>("agent_chat_cmd", {
    conversationId,
    assistantMessageId: opts?.assistantMessageId,
    messages,
    temperature: opts?.temperature,
    toolsEnabled: opts?.toolsEnabled ?? false,
    target: opts?.target,
    onEvent: ch,
  });
}

/**
 * `HRN-UI-5`: pick an interrupted run back up with its tool results intact.
 *
 * The transcript is rebuilt in Rust from the session log (`CTX-2`), not from
 * what is on screen — the screen has the prose, the log has the work. Resolves
 * `false` when there is nothing to resume, which is a fact about the run and
 * not a failure.
 */
export function resumeRun(
  conversationId: string,
  onEvent: (e: AgentEvent) => void,
  opts?: RunOptions
): Promise<boolean> {
  const ch = new Channel<AgentEvent>();
  ch.onmessage = onEvent;
  return invoke<boolean>("resume_run_cmd", {
    conversationId,
    assistantMessageId: opts?.assistantMessageId,
    temperature: opts?.temperature,
    toolsEnabled: opts?.toolsEnabled ?? false,
    target: opts?.target,
    onEvent: ch,
  });
}

/** `HRN-UI-5`: the new branch, and the question to ask it again. */
export interface ForkedConversation {
  conversation: DbConversation;
  /** `null` when the fork point had no user turn before it. */
  resend: string | null;
}

/**
 * `HRN-UI-5`: branch a conversation just before one assistant turn. The branch
 * keeps the persona, model, working folder and trust of the original — a fork
 * the user has to set up again is not the same question asked again.
 */
export const forkConversation = (
  conversationId: string,
  messageId: string
): Promise<ForkedConversation> =>
  invoke<ForkedConversation>("fork_conversation_cmd", { conversationId, messageId });

/**
 * Say something to a run that is already working (`HRN-2`). The text is picked
 * up at the top of the run's next iteration, so it lands between tool calls
 * rather than in the middle of one.
 *
 * Resolves `false` when the run finished between the keystroke and the send —
 * the caller then sends it as an ordinary next message instead.
 */
export const steerRun = (runId: string, text: string): Promise<boolean> =>
  invoke<boolean>("steer_run_cmd", { runId, text });

// ---- delegation (`SUB-9`) ----

/** How a child run ended, as the row records it. `queued` is `SUB-12`: it was
 * started in the background and is waiting for a free slot in the pool. */
export type SubRunStatus = "queued" | "running" | "done" | "stopped" | "error";

/** Is this child still to come? Queued and working are one thing to every
 * reader — neither is a result — and the only difference is whether it has
 * begun. One predicate so no surface can accidentally read a waiting agent as
 * a finished one. */
export const stillWorking = (status: SubRunStatus): boolean =>
  status === "running" || status === "queued";

/**
 * One delegated child run. Its *work* lives in `child_conversation_id`, which
 * is an ordinary conversation — the Agents tab opens it with the same
 * `listMessages` / artifact calls any conversation uses.
 */
export interface SubagentRun {
  id: string;
  parent_conversation_id: string;
  parent_message_id: string | null;
  child_conversation_id: string;
  agent: string;
  task: string;
  status: SubRunStatus;
  stop_reason: StopReason | null;
  result: string | null;
  steps: number;
  started_at: number;
  ended_at: number | null;
}

/** An agent type the lead may hand a job to: `general`, plus spawnable personas. */
export interface AgentType {
  name: string;
  description: string;
  persona_id: string | null;
}

export const listSubagentRuns = (conversationId: string) =>
  invoke<SubagentRun[]>("list_subagent_runs_cmd", { conversationId });

export const getSubagentRun = (id: string) =>
  invoke<SubagentRun | null>("get_subagent_run_cmd", { id });

/**
 * Stop one child and everything it started, keeping what it has (`SUB-7`).
 * Resolves `false` when the run had already finished.
 */
export const stopRun = (runId: string) => invoke<boolean>("stop_run_cmd", { runId });

/** Say something to a child that is already working (`SUB-6`). */
export const steerSubagent = (runId: string, text: string) =>
  invoke<boolean>("steer_subagent_cmd", { runId, text });

export const listAgentTypes = () => invoke<AgentType[]>("list_agent_types_cmd");

/** One line of the Usage panel (`OBS-2`): a day, a model, or a conversation. */
export interface UsageBucket {
  /** A UTC day start in epoch ms, a model name, or a conversation id. */
  key: string;
  /** A conversation's title, when it still exists. */
  label: string | null;
  provenance: "local" | "cloud" | "endpoint" | string;
  prompt_tokens: number;
  output_tokens: number;
  runs: number;
  /** `null` means the price is unknown, which is never the same as free. */
  cost_usd: number | null;
}

export interface PricedUsage {
  total: UsageBucket;
  by_day: UsageBucket[];
  by_model: UsageBucket[];
  by_conversation: UsageBucket[];
  /** At least one model has no price, so the total is a floor, not a figure. */
  some_prices_unknown: boolean;
}

export const usageSummary = (days = 30) => invoke<PricedUsage>("usage_summary_cmd", { days });

/** `HRN-UI-4`: write a kept result into the chat's working folder. Returns the
 * path it landed at; rejects when the chat has no folder. */
export const saveKeptResult = (conversationId: string, reference: string, text: string) =>
  invoke<string>("save_kept_result_cmd", { conversationId, reference, text });

/**
 * Fold every turn up to `uptoMessageId` into the conversation's summary and
 * return it (CTX-3). Only changes what is *sent* to the model — no message is
 * deleted or hidden.
 */
export const compactConversation = (
  conversationId: string,
  uptoMessageId: string,
  target?: ChatTarget
) =>
  invoke<string>("compact_conversation_cmd", { conversationId, uptoMessageId, target });

/** One compaction this conversation has been through (`CTX-5`). */
export interface Compaction {
  /** When it happened, epoch ms. */
  at: number;
  /** The summary as it was written at the time. */
  text: string;
  /** The first message it stands in for, if that could still be resolved. */
  from_message_id: string | null;
  /** The last message it stands in for. */
  upto_message_id: string;
  /** How many messages this pass replaced. */
  replaced: number;
  /** Whether it folded an earlier summary into itself. When true, the text has
   * been through a model twice and detail the first pass dropped is not
   * recoverable from this one. */
  merged_earlier: boolean;
}

/**
 * Every compaction this conversation has been through, oldest first (`CTX-5`).
 *
 * The conversation itself carries only the newest summary, because that is what
 * gets sent. This is the record of how it got there, which survives being
 * compacted again.
 */
export const conversationSummaries = (conversationId: string) =>
  invoke<Compaction[]>("conversation_summaries_cmd", { conversationId });

// ---- durable memory (MEM) ----

/** A durable entry — a fact or lesson — as stored on disk. */
export interface Fact {
  name: string;
  description: string;
  kind: string;
  created: string;
  source_conversation: string | null;
  body: string;
  /** When this fact last actually reached a prompt (SEM-UI-4) — set only by
   * `listMemoryFacts`, not present on a lesson. */
  last_used_at?: number | null;
  /** `"global" | "topical" | null` (`SCP-1`) — facts only. `null` means not
   * yet classified, which reads as global until the backfill catches it. */
  scope: string | null;
  /** How many times reflection has drawn this same lesson again (`RPT-1`).
   * `null`/`undefined` reads as 1 — never written yet. Facts never set this. */
  recurrence?: number | null;
  /** YYYY-MM-DD of the most recent recurrence bump (`RPT-1`). */
  last_seen?: string | null;
  /** YYYY-MM-DD after which this fact is swept automatically (`TTL-1`).
   * `null` means it never expires. Lessons never set this. */
  expires_at?: string | null;
}

/** What gets prepended to every conversation (MEM-3). */
export interface MemoryContext {
  index: string;
  soul: string;
  /** PRO-6: the synthesized style profile's body, or "" when none exists yet. */
  about_you: string;
  fact_count: number;
}

/** `memory/PROFILE.md` (PRO-1) — SMP-5 gives this no name in the UI; it is
 * untitled prose at the top of the memory page, not a "profile" tab. */
export interface Profile {
  version: number;
  updated: string;
  source_count: number;
  edited: boolean;
  body: string;
}

export interface ChangeProposal {
  id: string;
  target: string;
  slug: string | null;
  proposed_text: string;
  /** Why the change is being asked for — for a critic-demoted lesson, the
   *  critic's objection. Shown while pending; never kept on the entry. */
  rationale: string;
  /** The entry's own one-line summary, kept if applied. Null for targets with
   *  no separate summary, and for rows written before schema v8. */
  description: string | null;
  status: string;
  created_at: number;
}

export interface Consolidation {
  deletes: string[];
  edits: { name: string; text: string }[];
  merges: { keep: string; drop: string[]; text: string }[];
}

export const getMemoryContext = () => invoke<MemoryContext>("get_memory_context_cmd");

/** What `recall_for` produces for one turn (SEM-3): the always-injected
 * index block, plus whatever lessons were actually retrieved by
 * meaning — shaped as `SearchHit`s so the timeline can render them through
 * the same provenance UI as `search_history` (SEM-5). */
export interface RecallResult {
  index: string;
  matches: SearchHit[];
  /** Names of the facts that actually reached the prompt this turn (WHY-2) —
   * what the character cap kept, not everything that was considered. */
  injected_facts: string[];
}

export const recallFor = (query: string) => invoke<RecallResult>("recall_for_cmd", { query });

// ---- context manifest (WHY): what shaped one answer ----

/** The compact record `finalizeMessage`'s `contextJson` stores per assistant
 * message (WHY-2) — slugs and an id, never prompt text. Field names are
 * snake_case on purpose: this object is serialized as-is and read back by
 * `context_manifest_cmd`'s Rust struct of the same shape. */
export interface ContextRefs {
  persona_id: string | null;
  soul_present: boolean;
  /** PRO-6/WHY-2: whether the synthesized profile reached this turn's prompt. */
  about_you_present: boolean;
  facts: string[];
  lessons: string[];
  files: string[];
}

/** One labelled slice of what shaped an answer (WHY-1/3). `always_on` layers
 * (soul, persona, about you, notes, session) ride on every turn; the rest
 * were brought in for this question specifically. Empty `text` renders as
 * "nothing from here" (WHY-5), never omitted. */
export interface ContextLayer {
  label: string;
  text: string;
  sources: string[];
  always_on: boolean;
}

export interface ContextManifest {
  /** `false` only for a historical request whose message predates `WHY-2`
   * (or failed before finalizing) — the panel says so rather than guessing. */
  recorded: boolean;
  layers: ContextLayer[];
}

/** The live manifest when `messageId` is omitted (the composer chip); the
 * manifest recorded for that specific answer when it's given ("why this
 * answer?"). */
export const contextManifest = (conversationId: string, messageId?: string) =>
  invoke<ContextManifest>("context_manifest_cmd", { conversationId, messageId });

export const listMemoryFacts = () => invoke<Fact[]>("list_memory_facts_cmd");

export const updateMemoryFact = (name: string, body: string, description?: string) =>
  invoke<void>("update_memory_fact_cmd", { name, body, description });

/** The user overriding a fact's scope by hand (`SCP-UI-1`) — they are the
 * final authority on their own standing instructions, classifier or not. */
export const setFactScope = (name: string, scope: "global" | "topical") =>
  invoke<void>("set_fact_scope_cmd", { name, scope });

/** Moves the fact to trash and returns the trash filename, for undo. */
export const forgetMemoryFact = (name: string) =>
  invoke<string>("forget_memory_fact_cmd", { name });

export const restoreMemoryFact = (file: string) =>
  invoke<void>("restore_memory_fact_cmd", { file });

export const setSoul = (text: string) => invoke<void>("set_soul_cmd", { text });

export const getProfile = () => invoke<Profile | null>("get_profile_cmd");

/** `PRO-2`/`PRO-4`: `force: false` is the automatic trigger (debounce, daily
 * tick) — it respects the volume gate and autonomy rung and resolves to
 * `null` when it decided not to act, never throwing for that. `force: true`
 * is the user's own `Rewrite this` (`PRO-UI-2`), which ignores both. */
export const rebuildProfile = (force: boolean) => invoke<Profile | null>("rebuild_profile_cmd", { force });

/** The user overwriting the synthesis with their own words (`PRO-UI-2`). */
export const editProfile = (text: string) => invoke<Profile>("edit_profile_cmd", { text });

/** `PRO-9`: undo the most recent rebuild. `null` if there's nothing to undo. */
export const undoProfileRebuild = () => invoke<Profile | null>("undo_profile_rebuild_cmd");

export const openMemoryDir = () => invoke<void>("open_memory_dir_cmd");

/** Zip the memory folder next to itself and reveal it. Returns the zip path. */
export const exportMemoryZip = () => invoke<string>("export_memory_zip_cmd");

export const listChangeProposals = () => invoke<ChangeProposal[]>("list_change_proposals_cmd");

/** `target` routes `GLD-2`'s before/after check the same way a chat turn is
 * routed, so a cloud-only setup is still guarded. */
export const resolveChangeProposal = (id: string, accept: boolean, target?: ChatTarget) =>
  invoke<void>("resolve_change_proposal_cmd", { id, accept, target });

/** `MAIL-UI-2`'s `Edit`: rewrite a pending proposal's text before accepting it. */
export const updateChangeProposalText = (id: string, proposedText: string) =>
  invoke<void>("update_change_proposal_text_cmd", { id, proposedText });

/** Ask the model to propose a tidy-up. Nothing is applied until apply_consolidation. */
export const consolidateMemory = (target?: ChatTarget) =>
  invoke<Consolidation>("consolidate_memory_cmd", { target });

export const getPendingConsolidation = () =>
  invoke<Consolidation | null>("get_pending_consolidation_cmd");

export const applyConsolidation = (accept: boolean, target?: ChatTarget) =>
  invoke<void>("apply_consolidation_cmd", { accept, target });

// ---- the autopoietic layer: reflection, skills, vitality (Phase 11) ----

/** Subscribe to an app-level backend event — the self-maintenance processes
 * (reflection, healing) run outside any chat stream and announce themselves
 * this way rather than through an agent-run channel. No-op in the browser. */
export function onAppEvent<T>(name: string, handler: (payload: T) => void): void {
  if (!inTauri()) return;
  listen<T>(name, (e) => handler(e.payload)).catch(() => {
    /* the app still works without the announcement */
  });
}

/** A durable-self write announced outside a chat stream (REF-3). Same shape as
 * the `memory_write` agent event. */
export interface MemoryWriteEvent {
  op: string;
  name: string;
  description: string;
  collection: string;
  undo_token: string;
}

/** The watchdog restarted (or failed to restart) the engine (HEAL-1). */
export interface HealedEvent {
  attempt: number;
  ok: boolean;
}

/** `TTL-2`: short-lived facts let go, at startup or overnight. */
export interface ExpirySweptEvent {
  count: number;
}

/** `GLD-2`: a self-change was checked against the golden set, found to make
 * things worse, and automatically reverted. */
export interface GoldenRevertedEvent {
  count: number;
}

/** A scheduled job (SCH) just claimed the one run slot. */
export interface JobStartedEvent {
  job_id: string;
  job_name: string;
}

/** A scheduled job finished, one way or another. */
export interface JobFinishedEvent {
  job_id: string;
  result: string;
  built_in: boolean;
}

/** A lesson the agent drew from a finished conversation (REF-2). */
export interface LessonDraft {
  name: string;
  description: string;
  body: string;
  confidence: string;
}

/** One tool's success record over a window (HEAL-2 / LOOP-UI-1). */
export interface ToolHealth {
  tool_name: string;
  ok: number;
  total: number;
}

/** How the organism is doing, in counts and words (ORG-1). */
export interface Vitality {
  facts: number;
  lessons: number;
  /** Agent Skills discoverable and switched on (`SKL-5`). */
  skills: number;
  /** How many times a skill has been read this install (`SKL-5`). */
  skill_uses: number;
  quarantined: string[];
  engine_restarts_session: number;
  pending_proposals: number;
  last_reflection: number | null;
  tool_health: ToolHealth[];
}

/** What one reflection pass produced. `saved` is in effect now; `proposed` is
 * waiting for the user (the `lessons` rung is set to ask-first). */
export interface Reflection {
  saved: LessonDraft[];
  proposed: LessonDraft[];
}

export const reflectConversation = (conversationId: string, target?: ChatTarget) =>
  invoke<Reflection>("reflect_conversation_cmd", { conversationId, target });

/** REF-3b: digest a few conversations that were left behind — closed with the
 * app, or abandoned before the leaving hook existed. Returns how many it read
 * back. Counted in the backend, which is the only side that knows how long each
 * conversation actually is. */
export const catchUpReflection = (target?: ChatTarget) =>
  invoke<number>("catch_up_reflection_cmd", { target });

export const listLessons = () => invoke<Fact[]>("list_lessons_cmd");

/** Moves the lesson to trash and returns the trash filename, for undo. */
export const forgetLesson = (name: string) => invoke<string>("forget_lesson_cmd", { name });

export const getVitality = (modelName?: string) =>
  invoke<Vitality>("get_vitality_cmd", { modelName });

/** Omit `modelName` for a local model — the backend fills in the running one. */
export const getToolHealth = (modelName?: string) =>
  invoke<ToolHealth[]>("get_tool_health_cmd", { modelName });

/** The Health tab's Golden section (`GLD-UI-1`): did the last self-change
 * check find anything worse? */
export interface GoldenStatus {
  passed: number;
  total: number;
  failing: string[];
  checked_at: number;
}

/** The last recorded golden-set run, read passively (no run triggered). */
export const getGoldenStatus = () => invoke<GoldenStatus | null>("get_golden_status_cmd");

/** "Check me now" — always runs a fresh pass. */
export const checkGolden = (target?: ChatTarget) =>
  invoke<GoldenStatus>("check_golden_cmd", { target });

export const restoreQuarantined = (file: string) =>
  invoke<void>("restore_quarantined_cmd", { file });

export const deleteQuarantined = (file: string) =>
  invoke<void>("delete_quarantined_cmd", { file });

/** Seed a conversation's workspace surface from a skill's bundled template
 * (`SKL-5`, carrying `RCP-UI-2` forward). */
export const setSurface = (conversationId: string, treeJson: string) =>
  invoke<string>("set_surface_cmd", { conversationId, treeJson });

// ---- workspace blocks + session state (Generative UI) ----

export const listBlocks = (conversationId: string) =>
  invoke<DbBlock[]>("list_blocks_cmd", { conversationId });

export const updateBlockState = (id: string, stateJson: string) =>
  invoke<void>("update_block_state_cmd", { id, stateJson });

export const getSessionState = (conversationId: string) =>
  invoke<string | null>("get_session_state_cmd", { conversationId });

export const setSessionState = (conversationId: string, stateJson: string) =>
  invoke<void>("set_session_state_cmd", { conversationId, stateJson });

export const resolvePermission = (id: string, decision: Decision) =>
  invoke<void>("resolve_permission_cmd", { id, decision });

// ---- built-in toolsets (Phase 9A, TOOL-6). Renamed from "skills" (TSET-1) —
// that name now belongs to Agent Skills (SKL), a different concept: a
// prompt-level capability pack, not a tool group. Settings still calls these
// "Tools" (TSET-4); "toolset" is only the internal/wire name. ----

export interface ToolsetInfo {
  id: string;
  label: string;
  description: string;
  enabled: boolean;
  /** True if enabling sends data off the device or runs code — UI warns. */
  sensitive: boolean;
}

export const listToolsets = () => invoke<ToolsetInfo[]>("list_toolsets_cmd");
export const setToolsetEnabled = (id: string, enabled: boolean) =>
  invoke<void>("set_toolset_enabled_cmd", { id, enabled });

/** How reliably a toolset's tools ran this week (LOOP-UI-1). Absent when no data. */
export interface ToolsetReliability {
  skill_id: string;
  ok_percent: number;
  calls: number;
}
export const getToolStats = () => invoke<ToolsetReliability[]>("get_tool_stats_cmd");

// ---- local image generation setup (Phase 9F) ----

export interface ImageSetupStatus {
  engine_installed: boolean;
  engine_path: string | null;
  model_installed: boolean;
  model_path: string | null;
  toolset_enabled: boolean;
}

export const imageSetupStatus = () => invoke<ImageSetupStatus>("image_setup_status_cmd");

/** One-click: download the hardware-matched image engine + default model, then
 * enable the toolset. Streams download progress. */
export function setupImageGeneration(
  onProgress: (p: DownloadProgress) => void
): Promise<ImageSetupStatus> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<ImageSetupStatus>("setup_image_generation_cmd", { onProgress: ch });
}

/** Install only the hardware-matched image engine (no model). */
export function installImageEngine(
  onProgress: (p: DownloadProgress) => void
): Promise<ImageSetupStatus> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<ImageSetupStatus>("install_image_engine_cmd", { onProgress: ch });
}

export interface ImageModel {
  name: string;
  path: string;
  size_bytes: number;
  is_default: boolean;
}
/** Model families the local engine can drive. The single-file ones load with
 * `-m`; the rest are assembled from a directory of parts. */
export type ImageArchitecture = "sd1" | "sdxl" | "flux" | "z-image" | "qwen-image" | "ideogram4";

/** One file of a model, tagged with the role it plays (`diffusion`, `vae`,
 * `llm`, `clip_l`, `t5xxl`, `uncond_diffusion`, or `model` for single-file). */
export interface ImageComponent {
  role: string;
  url: string;
  filename: string;
  size_bytes: number;
}

/** The settings a model is actually driven at. Distilled models need a low
 * cfg and few steps; SDXL and newer need their native 1024px. Getting these
 * wrong is what makes a good model produce garbage, so they are shown. */
export interface ImageProfile {
  arch: ImageArchitecture;
  cfg_scale: number;
  steps: number;
  size: number;
  sampling: string | null;
  flow_shift: number | null;
}

export interface ImageCatalogEntry {
  id: string;
  name: string;
  note: string;
  size_label: string;
  arch: ImageArchitecture;
  components: ImageComponent[];
  total_bytes: number;
  profile: ImageProfile;
  /** How this model runs on the user's machine — judged on the transformer,
   * which is the part that has to be GPU-resident, not the whole download. */
  fit: Fit;
  vram_label: string;
}

export const imageCatalog = () => invoke<ImageCatalogEntry[]>("image_catalog_cmd");
export const listImageModels = () => invoke<ImageModel[]>("list_image_models_cmd");

/** A model in the "Images & video" picker group (`PIK-1`) — the one shape
 * every media backend, local or hosted, is normalised to. */
export interface MediaModel {
  id: string;
  name: string;
  backend_id: string;
  backend_label: string;
  modality: "image" | "video";
  price_label?: string | null;
  supports_edit: boolean;
  supported_aspect_ratios: string[];
  supported_resolutions: string[];
  max_duration_secs?: number | null;
}

export const listMediaModels = (modality?: "image" | "video") =>
  invoke<MediaModel[]>("list_media_models_cmd", { modality: modality ?? null });

/** The declared route (`PIK-2`): generate against an exact model id from the
 * picker, with optional references (`EDT-1`/`EDT-2`) and lineage. */
/** A background media generation (`JOB-1`). Both generation commands return
 * one of these as soon as the work is *accepted* — the artifact itself arrives
 * later on the `poiesis-media-job` event. */
export interface MediaJob {
  id: string;
  conversation_id: string | null;
  message_id: string | null;
  modality: "image" | "video";
  status: "running" | "done" | "failed" | "cancelled";
  prompt: string;
  model_id: string | null;
  aspect_ratio: string | null;
  started_at: number;
  finished_at: number | null;
  artifact_id: string | null;
  error: string | null;
}

/** A job changing state. `artifact` is present exactly when `status` is
 * `"done"`, so the stream can render the media without a second round trip. */
export interface MediaJobEvent {
  job_id: string;
  conversation_id: string | null;
  message_id: string | null;
  status: MediaJob["status"];
  artifact: Artifact | null;
  error: string | null;
}

/** `PIK-4`'s knobs are optional everywhere: a backend that can't honour one
 * reports it in `meta_json.ignored` rather than failing the generation. */
export const generateMedia = (args: {
  modelId: string;
  modality: "image" | "video";
  prompt: string;
  conversationId?: string | null;
  messageId?: string | null;
  aspectRatio?: string;
  resolution?: string;
  seed?: number;
  steps?: number;
  negative?: string;
  durationSecs?: number;
  references?: string[];
  parentArtifactId?: string;
}) =>
  invoke<MediaJob>("generate_media_cmd", {
    conversationId: args.conversationId ?? null,
    messageId: args.messageId ?? null,
    modelId: args.modelId,
    modality: args.modality,
    prompt: args.prompt,
    aspectRatio: args.aspectRatio ?? null,
    resolution: args.resolution ?? null,
    seed: args.seed ?? null,
    steps: args.steps ?? null,
    negative: args.negative ?? null,
    durationSecs: args.durationSecs ?? null,
    references: args.references ?? null,
    parentArtifactId: args.parentArtifactId ?? null,
  });

/** Generate an image directly (not via the chat model). Submits a job; the
 * artifact arrives on the event. The composer path and the `generate_image`
 * tool path converge on the same real artifact (`ART-2`). */
export const generateImage = (args: {
  prompt: string;
  conversationId?: string | null;
  messageId?: string | null;
  modelPath?: string;
  negative?: string;
  width?: number;
  height?: number;
  steps?: number;
  seed?: number;
}) =>
  invoke<MediaJob>("generate_image_cmd", {
    conversationId: args.conversationId ?? null,
    messageId: args.messageId ?? null,
    prompt: args.prompt,
    modelPath: args.modelPath ?? null,
    negative: args.negative ?? null,
    width: args.width ?? null,
    height: args.height ?? null,
    steps: args.steps ?? null,
    seed: args.seed ?? null,
  });

/** `STR-4`: a partial image mid-generation. Best-effort — a dropped one costs
 * nothing, and the finished picture always arrives on the job event. */
export interface MediaPartialEvent {
  job_id: string;
  data_uri: string;
}

/** `CST-2`: what media has cost. Derived from the artifacts themselves, so it
 * can't drift from what was actually made. No enforcement — just the number. */
export interface MediaSpend {
  usd: number;
  images: number;
  videos: number;
}
export interface MediaSpendReport {
  month: MediaSpend;
  all_time: MediaSpend;
}
export const mediaSpend = () => invoke<MediaSpendReport>("media_spend_cmd");

/** Stop a running generation. `false` means it had already finished. */
export const cancelMediaJob = (jobId: string) =>
  invoke<boolean>("cancel_media_job_cmd", { jobId });

/** Generations still in flight for a conversation, so a reload re-attaches to
 * them instead of showing a turn that looks abandoned. */
export const listRunningMediaJobs = (conversationId: string) =>
  invoke<MediaJob[]>("list_running_media_jobs_cmd", { conversationId });
export const setDefaultImageModel = (path: string) =>
  invoke<void>("set_default_image_model_cmd", { path });
export const deleteImageModel = (path: string) =>
  invoke<void>("delete_image_model_cmd", { path });

export function downloadImageModel(
  url: string,
  filename: string,
  onProgress: (p: DownloadProgress) => void
): Promise<void> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<void>("download_image_model_cmd", { url, filename, onProgress: ch });
}

/** Download a catalog entry by id. Multi-file models arrive as one directory
 * with a manifest, and report a single progress figure across all their parts,
 * so a four-file download still reads as one download. */
export function downloadImageCatalogModel(
  id: string,
  onProgress: (p: DownloadProgress) => void
): Promise<void> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<void>("download_image_catalog_model_cmd", { id, onProgress: ch });
}

// ---- embedding engine (Perception, EMB) ----
// "I use this to recall things by meaning instead of by keyword. It runs on
// the CPU, so it never takes memory from the model you chat with."

export interface EmbedSetupStatus {
  engine_installed: boolean;
  model_installed: boolean;
  model_name: string | null;
  model_path: string | null;
  running: boolean;
}
export interface EmbedCatalogEntry {
  name: string;
  note: string;
  size_label: string;
  url: string;
  filename: string;
  dim: number;
}

export const embedEngineStatus = () => invoke<EmbedSetupStatus>("embed_setup_status_cmd");

/** One-click: ensure the shared llama.cpp binary is present, then download
 * the default recall model. */
export function installEmbedEngine(
  onProgress: (p: DownloadProgress) => void
): Promise<EmbedSetupStatus> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<EmbedSetupStatus>("install_embed_engine_cmd", { onProgress: ch });
}

/** Stop the engine and remove the installed recall model (undoes install). */
export const removeEmbedEngine = () => invoke<EmbedSetupStatus>("remove_embed_engine_cmd");

export const embedCatalog = () => invoke<EmbedCatalogEntry[]>("embed_catalog_cmd");
export const listEmbedModels = () => invoke<ModelEntry[]>("list_embed_models_cmd");
export const setDefaultEmbedModel = (id: string) => invoke<void>("set_default_embed_model_cmd", { id });
export const deleteEmbedModel = (id: string) => invoke<void>("delete_embed_model_cmd", { id });

export function downloadEmbedModel(
  args: { url: string; name: string; filename: string },
  onProgress: (p: DownloadProgress) => void
): Promise<ModelEntry> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<ModelEntry>("download_embed_model_cmd", {
    url: args.url,
    name: args.name,
    filename: args.filename,
    onProgress: ch,
  });
}

// ---- reranking engine (Perception, RRK) ----
// "Re-reads the closest matches before answering. A little slower, and
// another 540 MB." Optional, off by default — never mention *embedding*,
// *reranker*, *bi-encoder* or *cross-encoder* on screen (SMP-3b/SMP-8a).

export interface RerankSetupStatus {
  engine_installed: boolean;
  model_installed: boolean;
  model_name: string | null;
  model_path: string | null;
  running: boolean;
  enabled: boolean;
}
export interface RerankCatalogEntry {
  name: string;
  note: string;
  size_label: string;
  url: string;
  filename: string;
}

export const rerankEngineStatus = () => invoke<RerankSetupStatus>("rerank_setup_status_cmd");

/** One-click: ensure the shared llama.cpp binary is present, then download
 * the default re-read model. Installing does not turn it on — see
 * `setRerankEnabled`. */
export function installRerankEngine(
  onProgress: (p: DownloadProgress) => void
): Promise<RerankSetupStatus> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<RerankSetupStatus>("install_rerank_engine_cmd", { onProgress: ch });
}

/** Stop the engine, turn it off, and remove the installed re-read model. */
export const removeRerankEngine = () => invoke<RerankSetupStatus>("remove_rerank_engine_cmd");

/** RRK-UI-2's toggle / SMP-3's `Sharper` choice. */
export const setRerankEnabled = (enabled: boolean) => invoke<void>("set_rerank_enabled_cmd", { enabled });

export const rerankCatalog = () => invoke<RerankCatalogEntry[]>("rerank_catalog_cmd");
export const listRerankModels = () => invoke<ModelEntry[]>("list_rerank_models_cmd");
export const setDefaultRerankModel = (id: string) => invoke<void>("set_default_rerank_model_cmd", { id });
export const deleteRerankModel = (id: string) => invoke<void>("delete_rerank_model_cmd", { id });

export function downloadRerankModel(
  args: { url: string; name: string; filename: string },
  onProgress: (p: DownloadProgress) => void
): Promise<ModelEntry> {
  const ch = new Channel<DownloadProgress>();
  ch.onmessage = onProgress;
  return invoke<ModelEntry>("download_rerank_model_cmd", {
    url: args.url,
    name: args.name,
    filename: args.filename,
    onProgress: ch,
  });
}

// ---- folder indexing (Perception, IDX) ----

export interface SkippedFile {
  path: string;
  reason: string;
}
export interface IndexRootView {
  path: string;
  /** idle | building | stale */
  state: string;
  file_count: number;
  chunk_count: number;
  skipped: SkippedFile[];
  size_bytes: number;
  updated_at: number;
  /** Only set when `state === "stale"` (IDX-UI-3). */
  changed_count: number | null;
}
export interface IndexProgress {
  files_done: number;
  files_total: number;
}

/** `null` when no folder is attached, or it's never been read. */
export const indexStatus = (conversationId: string) =>
  invoke<IndexRootView | null>("index_status_cmd", { conversationId });

export function buildIndex(
  conversationId: string,
  onProgress: (p: IndexProgress) => void
): Promise<IndexRootView> {
  const ch = new Channel<IndexProgress>();
  ch.onmessage = onProgress;
  return invoke<IndexRootView>("build_index_cmd", { conversationId, onProgress: ch });
}

/** SMP-4a: true when attaching this folder should start reading it straight
 * away — never built, never stopped (SMP-4d), and folder reading is on. */
export const shouldAutoIndex = (conversationId: string) =>
  invoke<boolean>("should_auto_index_cmd", { conversationId });

export const cancelIndex = (conversationId: string) =>
  invoke<boolean>("cancel_index_cmd", { conversationId });

export const forgetIndex = (path: string) => invoke<void>("forget_index_cmd", { path });

export const listIndexRoots = () => invoke<IndexRootView[]>("list_index_roots_cmd");

// ---- duplicate detection (Perception, PHS) ----

export interface DuplicateGroup {
  kind: "image" | "document";
  /** "identical" | "near-duplicate" (images); "similar" (documents). */
  relation: string;
  files: string[];
}

/** `Tree.tsx`'s folder-level "Find duplicates" (`PHS-UI-1`). `path` is the
 * folder to scan — any folder in the tree, not only the attached root. */
export const findDuplicates = (conversationId: string, path: string) =>
  invoke<DuplicateGroup[]>("find_duplicates_cmd", { conversationId, path });

/** Sends one file from a duplicate group to the same trash `RecentChanges.tsx`
 * already renders, so removing a duplicate is undoable like any other change. */
export const trashFile = (conversationId: string, path: string) =>
  invoke<void>("trash_file_cmd", { conversationId, path });

// ---- personas (Phase 9B, CHT-4 / CHT-7) ----

export interface Persona {
  id: string;
  name: string;
  system_prompt: string;
  model_id: string | null;
  params_json: string | null;
  is_default: boolean;
  created_at: number;
  updated_at: number;
  /** `PER-1`: JSON array of allowed toolset ids, or `null` for every enabled toolset. */
  tools_json: string | null;
  /** `SKL-6`: JSON array of allowed Agent Skill names, or `null` for every enabled skill. */
  skills_json: string | null;
  /** `SUB-3`: one line saying when to hand this agent a job. The lead reads it. */
  description: string | null;
  /** `SUB-3`: may the agent delegate work to this one. Off by default. */
  spawnable: boolean;
}

export const listPersonas = () => invoke<Persona[]>("list_personas_cmd");
export const createPersona = (args: {
  name: string;
  systemPrompt: string;
  modelId?: string | null;
  paramsJson?: string | null;
  toolsJson?: string | null;
  skillsJson?: string | null;
  description?: string | null;
  spawnable?: boolean;
}) =>
  invoke<Persona>("create_persona_cmd", {
    name: args.name,
    systemPrompt: args.systemPrompt,
    modelId: args.modelId ?? null,
    paramsJson: args.paramsJson ?? null,
    toolsJson: args.toolsJson ?? null,
    skillsJson: args.skillsJson ?? null,
    description: args.description ?? null,
    spawnable: args.spawnable ?? false,
  });
export const updatePersona = (persona: Persona) =>
  invoke<void>("update_persona_cmd", { persona });
export const deletePersona = (id: string) => invoke<void>("delete_persona_cmd", { id });
export const setDefaultPersona = (id: string) =>
  invoke<void>("set_default_persona_cmd", { id });
export const setConversationPersona = (
  conversationId: string,
  personaId: string | null,
  overridesJson: string | null
) =>
  invoke<void>("set_conversation_persona_cmd", {
    conversationId,
    personaId,
    overridesJson,
  });

export const listPermissions = () => invoke<Grant[]>("list_permissions_cmd");
export const addPermission = (path: string, mode: "read" | "read-write") =>
  invoke<Grant>("add_permission_cmd", { path, mode });
export const revokePermission = (id: string) => invoke<void>("revoke_permission_cmd", { id });

export const browserState = (conversationId: string) =>
  invoke<BrowserPanelState | null>("browser_state_cmd", { conversationId });
export const stopBrowser = (conversationId: string) =>
  invoke<void>("stop_browser_cmd", { conversationId });
/** Forget a finished session's record, so it stops coming back on re-open. */
export const forgetBrowserSession = (conversationId: string) =>
  invoke<void>("forget_browser_session_cmd", { conversationId });
export const listCapabilityGrants = () => invoke<CapabilityGrant[]>("list_capability_grants_cmd");
export const revokeCapabilityGrant = (id: string) =>
  invoke<void>("revoke_capability_grant_cmd", { id });
export const listActivity = (limit?: number) =>
  invoke<ActivityEntry[]>("list_activity_cmd", { limit });

// ---- cloud providers (Phase 7, BYOK) ----

/** One card on the Providers page (`PRV-2`), built by the backend: chat
 * providers and media-only backends alike, so no provider is hard-coded here. */
export interface ProviderInfo {
  id: string;
  name: string;
  /** "cloud" is a chat provider (its key may unlock media too); "media" is
   * a media-only backend with its own key. */
  kind: "cloud" | "media" | string;
  key_set: boolean;
  key_hint: string;
  console_url: string;
  /** Any of "chat", "image", "video". */
  unlocks: string[];
  /** The last real request's account problem (bad key, no credit), if any. */
  last_error: string | null;
}
export interface CloudModel {
  id: string;
  name: string;
  provider: string;
  model: string;
  vision: boolean;
  /** Whether the provider will accept a `tools` array for this model. False
   * means the agent loop can't call tools on it — OpenRouter answers such a
   * request with a bare 404. */
  tools: boolean;
  /** USD per million tokens, when known (`MOD-5`). Null is unknown, not free. */
  prompt_per_mtok: number | null;
  output_per_mtok: number | null;
}

export const listProviders = () => invoke<ProviderInfo[]>("list_providers_cmd");
export const setProviderKey = (provider: string, key: string) =>
  invoke<void>("set_provider_key_cmd", { provider, key });
/** `PRV-3`: check the key with the provider and save it only if it works.
 * Rejects with the sentence to show when it doesn't. */
export const verifyProviderKey = (provider: string, key: string) =>
  invoke<void>("verify_provider_key_cmd", { provider, key });
export const clearProviderKey = (provider: string) =>
  invoke<void>("clear_provider_key_cmd", { provider });
export const listCloudModels = () => invoke<CloudModel[]>("list_cloud_models_cmd");

// ---- your own model servers (Ollama / LM Studio) ----

export interface EndpointInfo {
  id: string;
  label: string;
  base_url: string;
  ctx_size: number;
  enabled: boolean;
  key_set: boolean;
}
export interface EndpointModel {
  id: string;
  endpoint_id: string;
  endpoint_label: string;
  name: string;
  model: string;
  vision: boolean;
  tools: boolean;
  ctx_size: number;
}
export interface EndpointProbe {
  ok: boolean;
  /** `ok` with zero models is a real state: LM Studio answers `/v1/models`
   * as soon as its server runs, before a model is loaded into it. */
  model_count: number;
  error: string | null;
  /** Set when the typed address failed but a `127.0.0.1` rewrite worked (the
   * Windows `localhost` → `::1` gotcha). Store this, not what was typed. */
  resolved_base_url: string | null;
}

export const listEndpoints = () => invoke<EndpointInfo[]>("list_endpoints_cmd");
export const addEndpoint = (label: string, baseUrl: string, apiKey?: string, ctxSize?: number) =>
  invoke<EndpointInfo>("add_endpoint_cmd", { label, baseUrl, apiKey, ctxSize });
export const updateEndpoint = (
  id: string,
  label: string,
  baseUrl: string,
  ctxSize: number,
  apiKey?: string
) => invoke<void>("update_endpoint_cmd", { id, label, baseUrl, ctxSize, apiKey });
export const setEndpointEnabled = (id: string, enabled: boolean) =>
  invoke<void>("set_endpoint_enabled_cmd", { id, enabled });
export const deleteEndpoint = (id: string) => invoke<void>("delete_endpoint_cmd", { id });
/** `endpointId` lets an already-saved endpoint be tested with its stored key,
 * which never leaves the credential store and so can't be passed back here.
 * A typed `apiKey` wins, so the add form can test a key before saving it. */
export const testEndpoint = (baseUrl: string, apiKey?: string, endpointId?: string) =>
  invoke<EndpointProbe>("test_endpoint_cmd", { baseUrl, apiKey, endpointId });
export const listEndpointModels = () => invoke<EndpointModel[]>("list_endpoint_models_cmd");

// ---- MCP connectors (Phase 6) ----

export interface McpTool {
  name: string;
  description: string;
  input_schema: unknown;
}
export interface ConnectorView {
  id: string;
  name: string;
  url: string | null;
  transport: string;
  enabled: boolean;
  has_auth: boolean;
  tools: McpTool[];
  created_at: number;
}
export interface ConnectorStatus {
  ok: boolean;
  tool_count: number;
  error: string | null;
}

export const listConnectors = () => invoke<ConnectorView[]>("list_connectors_cmd");
export const addConnector = (
  name: string,
  url: string,
  token?: string,
  transport?: "http" | "stdio"
) => invoke<ConnectorView>("add_connector_cmd", { name, url, token, transport });
export const exportConnectors = () => invoke<string>("export_connectors_cmd");
export const importConnectors = (json: string) =>
  invoke<number>("import_connectors_cmd", { json });
export const testConnector = (id: string) => invoke<ConnectorStatus>("test_connector_cmd", { id });
export const setConnectorEnabled = (id: string, enabled: boolean) =>
  invoke<void>("set_connector_enabled_cmd", { id, enabled });
export const deleteConnector = (id: string) => invoke<void>("delete_connector_cmd", { id });

// ---- working folder + Workbench ----

/** How much the agent may change inside the attached folder. */
export type FolderTrust = "read-only" | "confirm" | "auto";

/** One row in the Workbench tree. */
export interface FileNode {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified: number;
}

/** One reversible file operation, listed in "Recent changes". */
export interface TrashEntry {
  id: string;
  conversation_id: string;
  op: string;
  path: string;
  prev_path: string | null;
  blob_path: string | null;
  created_at: number;
  undone: boolean;
}

/** Open the folder picker. Resolves to null if the user cancelled; rejects with
 * a plain-language reason if the folder is one Poiesis refuses to work in. */
export const pickFolder = () => invoke<string | null>("pick_folder_cmd");
/** Pick attachment files. Routed through Rust so the choice counts as consent. */
export const pickFiles = () => invoke<string[]>("pick_files_cmd");
/** Pick a single `.zip` archive (`Add from zip…`, `SKL-4`). */
export const pickZipFile = () => invoke<string | null>("pick_zip_file_cmd");

export const setConversationFolder = (id: string, path: string | null) =>
  invoke<void>("set_conversation_folder_cmd", { id, path });
export const setConversationTrust = (id: string, trust: FolderTrust) =>
  invoke<void>("set_conversation_trust_cmd", { id, trust });

// ---- projects (`PRJ-1`/`PRJ-3`) ----

export const listProjects = (includeArchived?: boolean) =>
  invoke<DbProject[]>("list_projects_cmd", { includeArchived });
/** Both arguments optional (`PRJ-1a`): most projects are not about a
 * directory. A folder that is already a project comes back rather than
 * erroring. */
export const createProject = (rootPath?: string | null, name?: string) =>
  invoke<DbProject>("create_project_cmd", { rootPath, name });
/** `PRJ-7`: the instructions every session in this project carries. */
export const setProjectInstructions = (id: string, instructions: string | null) =>
  invoke<void>("set_project_instructions_cmd", { id, instructions });
/** `PRJ-3a`: give a project a folder, or with `null` take it away. Removing it
 * leaves the project and all its sessions in place. */
export const setProjectRoot = (id: string, rootPath: string | null) =>
  invoke<DbProject>("set_project_root_cmd", { id, rootPath });
export const renameProject = (id: string, name: string) =>
  invoke<void>("rename_project_cmd", { id, name });
export const setProjectTrust = (id: string, trust: FolderTrust) =>
  invoke<void>("set_project_trust_cmd", { id, trust });
/** Hides the project and its sessions. Nothing on disk is touched, ever. */
export const setProjectArchived = (id: string, archived: boolean) =>
  invoke<void>("set_project_archived_cmd", { id, archived });
export const setProjectTabs = (id: string, tabsJson: string | null) =>
  invoke<void>("set_project_tabs_cmd", { id, tabsJson });
export const setConversationProject = (conversationId: string, projectId: string | null) =>
  invoke<void>("set_conversation_project_cmd", { conversationId, projectId });

export const readDirTree = (path: string, conversationId?: string, showHidden?: boolean) =>
  invoke<FileNode[]>("read_dir_tree_cmd", { path, conversationId, showHidden });
export const readTextFile = (path: string, conversationId?: string, maxBytes?: number) =>
  invoke<string>("read_text_file_cmd", { path, conversationId, maxBytes });

/** A file opened in an editable tab. `read_only` is a reason, not a flag, so
 * the tab can say *why* it won't take an edit. */
export interface EditableFile {
  content: string;
  /** Epoch millis, handed back on save to detect a write underneath us. */
  modified: number;
  read_only: string | null;
}
/** Unlike `readTextFile` this never truncates — a clipped buffer saved back
 * would write the clip over the file. Big files are refused instead. */
export const readFileForEdit = (path: string, conversationId?: string) =>
  invoke<EditableFile>("read_file_for_edit_cmd", { path, conversationId });
export const writeTextFile = (
  path: string,
  content: string,
  conversationId?: string,
  expectedModified?: number
) =>
  invoke<EditableFile>("write_text_file_cmd", {
    path,
    content,
    conversationId,
    expectedModified,
  });
export const openPath = (path: string, conversationId?: string) =>
  invoke<void>("open_path_cmd", { path, conversationId });
export const revealPath = (path: string, conversationId?: string) =>
  invoke<void>("reveal_path_cmd", { path, conversationId });

// ---- app-data overview (Settings -> Working dir) ----

/** One top-level item under the app-data folder. */
export interface DataDirEntry {
  name: string;
  is_dir: boolean;
  size_bytes: number;
}

export interface DataDirOverview {
  path: string;
  total_bytes: number;
  entries: DataDirEntry[];
}

export const dataDirOverview = () => invoke<DataDirOverview>("data_dir_overview_cmd");

export function formatDiskSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}

/** Materialise an artifact into the working folder. Returns the written path. */
export const saveArtifactToFolder = (
  conversationId: string,
  artifactId: string,
  dest: string
) => invoke<string>("save_artifact_to_folder_cmd", { conversationId, artifactId, dest });

// ---- the coding half (`CODING_PLAN`) ----

export type TaskKind = "build" | "check" | "test" | "lint" | "other";

/** One task a project declares (`COD-1`). */
export interface ProjectTask {
  name: string;
  argv: string[];
  cwd: string;
  kind: TaskKind;
  source: string;
}

export interface ProjectCard {
  languages: string[];
  manifests: string[];
  package_managers: string[];
  tasks: ProjectTask[];
  git: boolean;
  branch: string | null;
  instructions_file: string | null;
  readme: boolean;
}

export type ExecPolicy = "off" | "ask" | "allow";

export interface ProjectAllow {
  tasks: string[];
  commands: string[];
  run_command: boolean;
}

/** `COD-UI-1`: what the project header shows. */
export interface ProjectCardView {
  card: ProjectCard;
  policy: ExecPolicy;
  policy_is_own: boolean;
  allow: ProjectAllow;
  tasks_enabled: boolean;
  card_built_at: number | null;
}

/** `COD-10`: one finding from a build, check or test run. */
export interface Diagnostic {
  file: string;
  line: number | null;
  col: number | null;
  severity: "error" | "warning" | "failure";
  message: string;
  code: string | null;
}

export interface DiffLine {
  kind: "context" | "added" | "removed";
  text: string;
  old_no: number | null;
  new_no: number | null;
}

export interface Hunk {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  lines: DiffLine[];
}

/** `PRJ-UI-3`: one file the agent changed. */
export interface FileChange {
  path: string;
  display: string;
  status: "added" | "modified" | "deleted" | "moved";
  from: string | null;
  added: number;
  removed: number;
  hunks: Hunk[];
  binary: boolean;
  too_large: boolean;
  entry_ids: string[];
  last_at: number;
}

export interface ChangeSet {
  files: FileChange[];
  added: number;
  removed: number;
  since: number;
  this_run: boolean;
}

/** `COD-UI-5`: what running a code artifact produced. */
export interface CodeRun {
  language: string;
  output: string;
  exit_code: number | null;
  timed_out: boolean;
  outcome: string;
  diagnostics: Diagnostic[];
  duration_ms: number;
}

export const projectCard = (id: string, refresh?: boolean) =>
  invoke<ProjectCardView | null>("project_card_cmd", { id, refresh });
export const setProjectExecPolicy = (id: string, policy: ExecPolicy | "inherit") =>
  invoke<void>("set_project_exec_policy_cmd", { id, policy });
export const setProjectTaskAllowed = (id: string, task: string, allowed: boolean) =>
  invoke<void>("set_project_task_allowed_cmd", { id, task, allowed });
export const setProjectCommands = (id: string, runCommand: boolean, forget?: string) =>
  invoke<void>("set_project_commands_cmd", { id, runCommand, forget });
export const conversationChanges = (conversationId: string) =>
  invoke<ChangeSet>("conversation_changes_cmd", { conversationId });
/** Put files back: one `path`, or every changed file when it is left out. */
export const undoChanges = (conversationId: string, path?: string) =>
  invoke<void>("undo_changes_cmd", { conversationId, path });
export const keepChanges = (conversationId: string) =>
  invoke<void>("keep_changes_cmd", { conversationId });
export const runCodeArtifact = (id: string) => invoke<CodeRun>("run_code_artifact_cmd", { id });

export const listTrash = (conversationId: string, limit?: number) =>
  invoke<TrashEntry[]>("list_trash_cmd", { conversationId, limit });
export const undoFileOp = (id: string) => invoke<void>("undo_file_op_cmd", { id });

// ---- multimodal attachments (Phase 5) ----

export const readImageDataUri = (path: string, conversationId?: string) =>
  invoke<string>("read_image_data_uri_cmd", { path, conversationId });
export const extractPdfText = (path: string, conversationId?: string) =>
  invoke<string>("extract_pdf_text_cmd", { path, conversationId });

/** Save a Canvas artifact to disk (CHT-6 download). `content` is the artifact's
 * source path for the `image` kind, its raw text otherwise. */
export const saveArtifactFile = (dest: string, kind: string, content: string) =>
  invoke<void>("save_artifact_cmd", { dest, kind, content });

/** One line an artifact's live preview printed (`ART-5`). `level` is a console
 * method name, or `uncaught` for a thrown error the page never handled. */
export type ConsoleEntry = {
  level: "log" | "info" | "warn" | "error" | "debug" | "uncaught";
  text: string;
  /** `file:line:col`, when the webview says where. */
  source?: string;
};

/** Hand the preview's console output to the agent side, so `read_artifact` can
 * return it. Fire-and-forget: a dropped log line must never break a preview. */
export const recordArtifactConsole = (artifactId: string, entries: ConsoleEntry[]) =>
  invoke<void>("record_artifact_console_cmd", { artifactId, entries });

/** Forget what a preview printed — it reloaded, so those lines are about code
 * that is no longer running. */
export const clearArtifactConsole = (artifactId: string) =>
  invoke<void>("clear_artifact_console_cmd", { artifactId });

/** `ART-6`: the loopback origin html artifacts are served from, or `null` if
 * the server didn't start (the Canvas then renders the source inline, as it
 * used to). Asked for once per session — the port and token don't change. */
let previewBase: Promise<string | null> | null = null;
export function previewBaseUrl(): Promise<string | null> {
  if (!previewBase) {
    previewBase = inTauri()
      ? invoke<string | null>("preview_base_url_cmd").catch(() => null)
      : Promise.resolve(null);
  }
  return previewBase;
}

/** A stable short key for a string, so an updated artifact gets a URL the
 * webview treats as new. Without it the iframe keeps showing the old page:
 * same src, no reload. */
export function contentVersion(s: string): string {
  let h = 5381;
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0;
  return (h >>> 0).toString(36);
}

// ---- scheduler (SCH): the quiet night shift ----

export type Cadence = "hourly" | "six-hourly" | "daily" | "weekly";

export interface ScheduledJob {
  id: string;
  name: string;
  prompt: string;
  cadence: Cadence;
  /** Folder this job's tools may reach, if any. Null for the built-in job. */
  scope: string | null;
  enabled: boolean;
  next_run_at: number;
  last_run_at: number | null;
  last_result: string | null;
  /** True only for the seeded nightly-reflection job: it can be turned off
   *  but not deleted, and has no editable prompt or scope. */
  built_in: boolean;
  /** The chat this task was made out of, if it was made from one. */
  source_conversation_id: string | null;
  /** The last few runs, newest first. */
  runs: JobRun[];
}

/** One completed run. Each is a real conversation in the rail — open it to
 *  read what the task actually did, rather than trusting a summary line. */
export interface JobRun {
  conversation_id: string;
  at: number;
  summary: string;
}

export interface ScheduledJobInput {
  name: string;
  prompt: string;
  cadence: Cadence;
  scope: string | null;
  enabled: boolean;
  source_conversation_id?: string | null;
}

/** What the Rail shows while a job runs (SCH-UI-4). */
export interface RunningJob {
  job_id: string;
  job_name: string;
  started_at: number;
}

/** The most recent nightly first-person summary (SCH-UI-1/2). */
export interface Digest {
  text: string;
  created_at: number;
  unread: boolean;
}

export const listScheduledJobs = () => invoke<ScheduledJob[]>("list_scheduler_jobs_cmd");
export const createScheduledJob = (input: ScheduledJobInput) =>
  invoke<ScheduledJob>("create_scheduler_job_cmd", { input });
export const updateScheduledJob = (id: string, input: ScheduledJobInput) =>
  invoke<ScheduledJob>("update_scheduler_job_cmd", { id, input });
export const deleteScheduledJob = (id: string) => invoke<void>("delete_scheduler_job_cmd", { id });
/** SCH-UI-3's "Run now" — runs outside the ticker but through the same
 *  concurrency-1 guard, so it rejects if another job is already running. */
export const runScheduledJobNow = (id: string) => invoke<string>("run_scheduler_job_now_cmd", { id });
export const schedulerStatus = () => invoke<RunningJob | null>("scheduler_status_cmd");
export const stopScheduledJob = () => invoke<boolean>("stop_scheduler_job_cmd");
export const getSchedulerDigest = () => invoke<Digest | null>("get_scheduler_digest_cmd");
export const markDigestRead = () => invoke<void>("mark_digest_read_cmd");

// ---- mail accounts (MAIL-UI-1) ----

export type MailSecurity = "tls" | "starttls";

export interface MailAccount {
  id: string;
  label: string;
  email: string;
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  username: string;
  auth: string;
  /** "tls" (implicit, 993/465) | "starttls" (upgrade, 143/587, local bridges). */
  security: MailSecurity;
  enabled: boolean;
  created_at: number;
}

export interface MailTestResult {
  ok: boolean;
  message_count: number | null;
  error: string | null;
}

export const listMailAccounts = () => invoke<MailAccount[]>("list_mail_accounts_cmd");
export const addMailAccount = (args: {
  label: string;
  email: string;
  imapHost: string;
  imapPort: number;
  smtpHost: string;
  smtpPort: number;
  username: string;
  password: string;
  security: MailSecurity;
}) => invoke<MailAccount>("add_mail_account_cmd", args);
export const testMailAccount = (id: string) => invoke<MailTestResult>("test_mail_account_cmd", { id });
export const setMailAccountEnabled = (id: string, enabled: boolean) =>
  invoke<void>("set_mail_account_enabled_cmd", { id, enabled });
export const deleteMailAccount = (id: string) => invoke<void>("delete_mail_account_cmd", { id });

// ---- Agent Skills (SKL) ----

export interface SkillView {
  name: string;
  description: string;
  when_to_use: string | null;
  source: "personal" | "project" | "app";
  dir: string;
  enabled: boolean;
  unsupported: string[];
  /** `OUT-2`: every activation ever, and how many had a tool failure after. */
  used: number;
  rough: number;
  /** `SKL-4`: `TRU-1`'s reading of the body, 0–3. Shown before enabling, never
   * a block — a skill scoring 3 still installs. */
  risk: number;
  risk_flags: string[];
}

/** Where the user's own skills live (`~/.poiesis/skills/`), created if absent
 * so the path the Skills tab shows is always a real folder. */
export const personalSkillsDir = () => invoke<string>("personal_skills_dir_cmd");

/** One skill found in another agent's folder, offered for import (`SKL-4`).
 * Listing is not reading — nothing here reaches a prompt until it's copied. */
export interface ImportableSkill {
  agent: string;
  name: string;
  description: string;
  dir: string;
  risk: number;
  risk_flags: string[];
  already_have: boolean;
}

export const discoverableSkillImports = (extraRoots?: string[]) =>
  invoke<ImportableSkill[]>("discoverable_skill_imports_cmd", {
    extraRoots: extraRoots ?? null,
  });

/** Copy the chosen skills in. Resolves with the names that *failed*, so a
 * partial import can say which ones didn't make it. */
export const importSkills = (dirs: string[]) => invoke<string[]>("import_skills_cmd", { dirs });

/** `TRU-1`'s verdict on one piece of text. */
export interface TextScan {
  risk: number;
  flags: string[];
  snippet: string;
}

/** `SKL-UI-2`: score a proposed `SKILL.md` that isn't on disk yet, so the
 * install card can show the same risk line the Skills tab shows. */
export const scanSkillText = (text: string) =>
  invoke<TextScan>("scan_skill_text_cmd", { text });

export const listSkills = (workingFolder?: string | null) =>
  invoke<SkillView[]>("list_skills_cmd", { workingFolder: workingFolder ?? null });
export const setSkillEnabled = (
  source: string,
  name: string,
  enabled: boolean,
  target?: ChatTarget
) => invoke<void>("set_skill_enabled_cmd", { source, name, enabled, target });
export const createSkill = (name: string, description: string, whenToUse: string, body: string) =>
  invoke<SkillView>("create_skill_cmd", { name, description, whenToUse, body });
export const updateSkill = (name: string, description: string, whenToUse: string, body: string) =>
  invoke<SkillView>("update_skill_cmd", { name, description, whenToUse, body });
export const installSkill = (sourceDir: string) =>
  invoke<SkillView>("install_skill_cmd", { sourceDir });
export const installSkillZip = (archivePath: string) =>
  invoke<SkillView>("install_skill_zip_cmd", { archivePath });
/** A skill's bundled `assets/surface.json`, if it ships one (`SKL-5`). */
export const skillSurface = (name: string, workingFolder?: string | null) =>
  invoke<string | null>("skill_surface_cmd", { name, workingFolder: workingFolder ?? null });
/** A skill's full body markdown, for `View`/`Edit` (`SKL-UI-1`). */
export const skillBody = (name: string, workingFolder?: string | null) =>
  invoke<string>("skill_body_cmd", { name, workingFolder: workingFolder ?? null });

export const forgetSkill = (name: string) => invoke<void>("forget_skill_cmd", { name });

// ---- mapping helpers ----

/** `PLN-1`: where one item of a run's plan stands. `dropped` keeps the item
 * and its reason — a plan that quietly loses items cannot be trusted to have
 * been the plan. Mirrors `agent::plan::PlanStatus`. */
export type PlanItemStatus = "todo" | "doing" | "done" | "dropped";

export interface PlanItem {
  text: string;
  status: PlanItemStatus;
  /** Why a dropped item was dropped. Only ever set for `dropped`. */
  why?: string | null;
  /** True for an item added after the plan was first written. */
  added?: boolean;
}

/** One run's plan. `revisions` counts rewrites, not status changes — rewriting
 * the plan is the event worth announcing (`PLN-UI-3`); ticking an item off is
 * not. `previous` holds the earlier versions, oldest first. */
export interface PlanView {
  items: PlanItem[];
  revisions: number;
  previous?: string[][];
}

/** Parse a stored `plan_json` back into the plan a run worked to (`PLN-UI-5`).
 * A row that will not parse reads as no plan rather than failing the reload —
 * one unreadable plan must not cost the user the conversation. */
export function parsePlan(planJson: string | null): PlanView | undefined {
  if (!planJson) return undefined;
  try {
    const plan = JSON.parse(planJson) as PlanView;
    return plan?.items?.length ? plan : undefined;
  } catch {
    return undefined;
  }
}

/** Parse a backend message row's steps_json into AgentStep[]. */
export function parseSteps(stepsJson: string | null): AgentStep[] | undefined {
  if (!stepsJson) return undefined;
  try {
    return JSON.parse(stepsJson) as AgentStep[];
  } catch {
    return undefined;
  }
}
