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
}

export interface DbAttachment {
  id: string;
  kind: string;
  name: string;
  path: string;
}
export interface DbMessage {
  id: string;
  conversation_id: string;
  role: "user" | "assistant";
  content: string;
  model_name: string | null;
  model_provenance: string | null;
  steps_json: string | null;
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
  attachments?: { kind: string; name: string; path: string }[];
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

export const finalizeMessage = (id: string, content: string, stepsJson?: string) =>
  invoke<void>("finalize_message_cmd", { id, content, stepsJson });

export const searchConversations = (query: string) =>
  invoke<DbConversation[]>("search_conversations_cmd", { query });

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

export type AgentEvent =
  | { type: "step_start"; id: string; verb: string; target: string }
  | { type: "step_done"; id: string; result: string | null }
  | { type: "step_error"; id: string; error: string }
  | { type: "token"; text: string }
  | { type: "artifact"; id: string; title: string; kind: string; content: string }
  | { type: "block"; id: string; message_id: string | null; kind: string; title: string; data: unknown }
  | { type: "block_update"; id: string; title: string; data: unknown }
  | { type: "state_update"; state: unknown }
  | { type: "permission"; request: PermissionRequest }
  | { type: "memory_write"; op: string; name: string; description: string; collection: string; undo_token: string }
  | { type: "recall"; id: string; matches: SearchHit[] }
  | { type: "proposal"; id: string; target: string; rationale: string }
  | { type: "file_changed"; op: string; path: string; undo_token: string }
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
}

/** One hit from the agent's search over its own past (RCL-1). */
export interface SearchHit {
  source: "chat" | "memory";
  conversation_id: string | null;
  /** Conversation title, or the memory entry's name. */
  title: string;
  created_at: number;
  snippet: string;
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
}

export type Decision = "deny" | "once" | "chat" | "forever";

export interface Grant {
  id: string;
  path: string;
  mode: string;
  created_at: number;
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
  provenance: "local" | "cloud";
  provider?: string;
  model?: string;
}

export function agentChat(
  conversationId: string,
  messages: ChatTurnMessage[],
  onEvent: (e: AgentEvent) => void,
  opts?: {
    temperature?: number;
    toolsEnabled?: boolean;
    target?: ChatTarget;
    assistantMessageId?: string;
  }
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

// ---- durable memory (MEM) ----

/** A durable entry — a fact, lesson, or recipe — as stored on disk. */
export interface Fact {
  name: string;
  description: string;
  kind: string;
  created: string;
  source_conversation: string | null;
  body: string;
}

/** What gets prepended to every conversation (MEM-3). */
export interface MemoryContext {
  index: string;
  soul: string;
  fact_count: number;
}

export interface ChangeProposal {
  id: string;
  target: string;
  slug: string | null;
  proposed_text: string;
  rationale: string;
  status: string;
  created_at: number;
}

export interface Consolidation {
  deletes: string[];
  edits: { name: string; text: string }[];
  merges: { keep: string; drop: string[]; text: string }[];
}

export const getMemoryContext = () => invoke<MemoryContext>("get_memory_context_cmd");

export const listMemoryFacts = () => invoke<Fact[]>("list_memory_facts_cmd");

export const updateMemoryFact = (name: string, body: string, description?: string) =>
  invoke<void>("update_memory_fact_cmd", { name, body, description });

/** Moves the fact to trash and returns the trash filename, for undo. */
export const forgetMemoryFact = (name: string) =>
  invoke<string>("forget_memory_fact_cmd", { name });

export const restoreMemoryFact = (file: string) =>
  invoke<void>("restore_memory_fact_cmd", { file });

export const setSoul = (text: string) => invoke<void>("set_soul_cmd", { text });

export const openMemoryDir = () => invoke<void>("open_memory_dir_cmd");

/** Zip the memory folder next to itself and reveal it. Returns the zip path. */
export const exportMemoryZip = () => invoke<string>("export_memory_zip_cmd");

export const listChangeProposals = () => invoke<ChangeProposal[]>("list_change_proposals_cmd");

export const resolveChangeProposal = (id: string, accept: boolean) =>
  invoke<void>("resolve_change_proposal_cmd", { id, accept });

/** Ask the model to propose a tidy-up. Nothing is applied until apply_consolidation. */
export const consolidateMemory = (target?: ChatTarget) =>
  invoke<Consolidation>("consolidate_memory_cmd", { target });

export const getPendingConsolidation = () =>
  invoke<Consolidation | null>("get_pending_consolidation_cmd");

export const applyConsolidation = (accept: boolean) =>
  invoke<void>("apply_consolidation_cmd", { accept });

// ---- the autopoietic layer: reflection, recipes, vitality (Phase 11) ----

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

/** A lesson the agent drew from a finished conversation (REF-2). */
export interface LessonDraft {
  name: string;
  description: string;
  body: string;
  confidence: string;
}

/** A procedure the agent developed with the user and may reuse (RCP-1). */
export interface Recipe {
  name: string;
  description: string;
  trigger: string;
  created: string;
  used: number;
  last_used: string | null;
  steps: string;
  surface_json: string | null;
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
  recipes: number;
  recipe_uses: number;
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

export const listLessons = () => invoke<Fact[]>("list_lessons_cmd");

/** Moves the lesson to trash and returns the trash filename, for undo. */
export const forgetLesson = (name: string) => invoke<string>("forget_lesson_cmd", { name });

export const getVitality = (modelName?: string) =>
  invoke<Vitality>("get_vitality_cmd", { modelName });

/** Omit `modelName` for a local model — the backend fills in the running one. */
export const getToolHealth = (modelName?: string) =>
  invoke<ToolHealth[]>("get_tool_health_cmd", { modelName });

export const listRecipes = () => invoke<Recipe[]>("list_recipes_cmd");

export const forgetRecipe = (name: string) => invoke<string>("forget_recipe_cmd", { name });

export const restoreQuarantined = (file: string) =>
  invoke<void>("restore_quarantined_cmd", { file });

export const deleteQuarantined = (file: string) =>
  invoke<void>("delete_quarantined_cmd", { file });

/** Seed a conversation's workspace surface from a recipe template (RCP-UI-2). */
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

// ---- built-in skills (Phase 9A, TOOL-6) ----

export interface SkillInfo {
  id: string;
  label: string;
  description: string;
  enabled: boolean;
  /** True if enabling sends data off the device or runs code — UI warns. */
  sensitive: boolean;
}

export const listSkills = () => invoke<SkillInfo[]>("list_skills_cmd");
export const setSkillEnabled = (id: string, enabled: boolean) =>
  invoke<void>("set_skill_enabled_cmd", { id, enabled });

/** How reliably a skill's tools ran this week (LOOP-UI-1). Absent when no data. */
export interface SkillReliability {
  skill_id: string;
  ok_percent: number;
  calls: number;
}
export const getToolStats = () => invoke<SkillReliability[]>("get_tool_stats_cmd");

// ---- local image generation setup (Phase 9F) ----

export interface ImageSetupStatus {
  engine_installed: boolean;
  engine_path: string | null;
  model_installed: boolean;
  model_path: string | null;
  skill_enabled: boolean;
}

export const imageSetupStatus = () => invoke<ImageSetupStatus>("image_setup_status_cmd");

/** One-click: download the hardware-matched image engine + default model, then
 * enable the skill. Streams download progress. */
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
export interface ImageCatalogEntry {
  name: string;
  note: string;
  size_label: string;
  url: string;
  filename: string;
}

export const imageCatalog = () => invoke<ImageCatalogEntry[]>("image_catalog_cmd");
export const listImageModels = () => invoke<ImageModel[]>("list_image_models_cmd");

/** Generate an image directly (not via the chat model). Returns the PNG path. */
export const generateImage = (args: {
  prompt: string;
  modelPath?: string;
  negative?: string;
  width?: number;
  height?: number;
  steps?: number;
}) =>
  invoke<string>("generate_image_cmd", {
    prompt: args.prompt,
    modelPath: args.modelPath ?? null,
    negative: args.negative ?? null,
    width: args.width ?? null,
    height: args.height ?? null,
    steps: args.steps ?? null,
  });
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
}

export const listPersonas = () => invoke<Persona[]>("list_personas_cmd");
export const createPersona = (args: {
  name: string;
  systemPrompt: string;
  modelId?: string | null;
  paramsJson?: string | null;
}) =>
  invoke<Persona>("create_persona_cmd", {
    name: args.name,
    systemPrompt: args.systemPrompt,
    modelId: args.modelId ?? null,
    paramsJson: args.paramsJson ?? null,
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
export const listActivity = (limit?: number) =>
  invoke<ActivityEntry[]>("list_activity_cmd", { limit });

// ---- cloud providers (Phase 7, BYOK) ----

export interface ProviderInfo {
  id: string;
  name: string;
  key_set: boolean;
  key_hint: string;
  console_url: string;
}
export interface CloudModel {
  id: string;
  name: string;
  provider: string;
  model: string;
  vision: boolean;
}

export const listProviders = () => invoke<ProviderInfo[]>("list_providers_cmd");
export const setProviderKey = (provider: string, key: string) =>
  invoke<void>("set_provider_key_cmd", { provider, key });
export const clearProviderKey = (provider: string) =>
  invoke<void>("clear_provider_key_cmd", { provider });
export const listCloudModels = () => invoke<CloudModel[]>("list_cloud_models_cmd");

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

export const setConversationFolder = (id: string, path: string | null) =>
  invoke<void>("set_conversation_folder_cmd", { id, path });
export const setConversationTrust = (id: string, trust: FolderTrust) =>
  invoke<void>("set_conversation_trust_cmd", { id, trust });

export const readDirTree = (path: string, conversationId?: string, showHidden?: boolean) =>
  invoke<FileNode[]>("read_dir_tree_cmd", { path, conversationId, showHidden });
export const readTextFile = (path: string, conversationId?: string, maxBytes?: number) =>
  invoke<string>("read_text_file_cmd", { path, conversationId, maxBytes });
export const openPath = (path: string, conversationId?: string) =>
  invoke<void>("open_path_cmd", { path, conversationId });
export const revealPath = (path: string, conversationId?: string) =>
  invoke<void>("reveal_path_cmd", { path, conversationId });

/** Materialise an artifact into the working folder. Returns the written path. */
export const saveArtifactToFolder = (
  conversationId: string,
  artifactId: string,
  dest: string
) => invoke<string>("save_artifact_to_folder_cmd", { conversationId, artifactId, dest });

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

// ---- mapping helpers ----

/** Parse a backend message row's steps_json into AgentStep[]. */
export function parseSteps(stepsJson: string | null): AgentStep[] | undefined {
  if (!stepsJson) return undefined;
  try {
    return JSON.parse(stepsJson) as AgentStep[];
  } catch {
    return undefined;
  }
}
