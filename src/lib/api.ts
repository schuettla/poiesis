// Typed wrappers over Tauri's `invoke` + streaming channels. The frontend can
// also run in a plain browser (vite dev without Tauri) for fast UI iteration;
// `inTauri()` lets callers fall back to mock data in that case.

import { Channel, invoke as tauriInvoke } from "@tauri-apps/api/core";
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
}

export interface PermissionRequest {
  id: string;
  summary: string;
  path: string;
  mode: "read" | "read-write";
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

// ---- multimodal attachments (Phase 5) ----

export const readImageDataUri = (path: string) =>
  invoke<string>("read_image_data_uri_cmd", { path });
export const extractPdfText = (path: string) =>
  invoke<string>("extract_pdf_text_cmd", { path });

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
