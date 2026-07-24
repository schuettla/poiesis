import { create } from "zustand";
import type {
  AgentStep,
  Attachment,
  BlockView,
  Conversation,
  Message,
  Mode,
  Model,
  ModelFilter,
  Provenance,
  View,
} from "./types";
import { mockConversations, mockModels } from "./mockData";
import * as api from "./api";

const SYSTEM_PROMPT_KEY = "system_prompt";
const READING_SCALE_KEY = "reading_scale";
const TELEMETRY_KEY = "telemetry_enabled";
const DEFAULT_SYSTEM_PROMPT =
  "You are Nexus, a helpful, capable assistant that shows its work. Be concise and clear.";

interface AppState {
  bootstrapped: boolean;

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
  providers: api.ProviderInfo[];
  selectedModelId: string;
  modelFilter: ModelFilter;
  engineReady: boolean;
  /** Which library model the running engine currently holds (null = none). */
  loadedModelId: string | null;
  loadingModel: { id: string; label: string } | null;
  selectModel: (id: string) => void;
  setModelFilter: (f: ModelFilter) => void;
  refreshLibrary: () => Promise<void>;
  refreshCloud: () => Promise<void>;
  loadModelById: (id: string) => Promise<void>;
  stopEngine: () => Promise<void>;

  // conversations
  conversations: Conversation[];
  activeConversationId: string | null;
  busy: boolean;
  systemPrompt: string;
  /** Whether built-in skills are offered to the model (TOOL-3, TOOL-6). */
  toolsEnabled: boolean;
  setToolsEnabled: (on: boolean) => void;
  /** Workspace mode: the chat view flips to the composed-interface layout —
   * the agent's UI is the interaction point, the message stream is a log. */
  workspaceMode: boolean;
  setWorkspaceMode: (on: boolean) => void;
  /** Composer "Create image" mode: the next message generates an image directly. */
  imageMode: boolean;
  setImageMode: (on: boolean) => void;
  /** Generate an image from `prompt` and show it inline in the chat (9F). */
  createImage: (prompt: string, modelPath?: string | null) => Promise<void>;

  // personas (CHT-4 / CHT-7)
  personas: api.Persona[];
  refreshPersonas: () => Promise<void>;
  createPersona: (args: {
    name: string;
    systemPrompt: string;
    modelId?: string | null;
    temperature?: number;
  }) => Promise<void>;
  updatePersona: (persona: api.Persona) => Promise<void>;
  deletePersona: (id: string) => Promise<void>;
  setDefaultPersona: (id: string) => Promise<void>;
  /** Apply (or clear) a persona on the active conversation. */
  applyPersona: (conversationId: string, personaId: string | null) => Promise<void>;
  /** Set a one-off per-conversation temperature override (CHT-7). */
  setConversationTemperature: (conversationId: string, temperature: number | null) => Promise<void>;

  // accessibility + privacy (§5.5, §6.3)
  readingScale: number;
  setReadingScale: (scale: number) => Promise<void>;
  telemetryEnabled: boolean;
  setTelemetryEnabled: (on: boolean) => Promise<void>;

  // library / all artifacts
  allArtifacts: api.Artifact[];
  refreshAllArtifacts: () => Promise<void>;
  viewArtifact: (artifact: api.Artifact) => Promise<void>;

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

  // artifacts / canvas panel (CHT-6)
  artifacts: Record<string, api.Artifact[]>;
  canvasOpen: boolean;
  activeArtifactId: string | null;
  openCanvas: (artifactId?: string) => void;
  closeCanvas: () => void;
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
    available: true,
    provider: m.provider,
    cloudModel: m.model,
  }));
}

function composeModels(lib: api.ModelEntry[], cloud: api.CloudModel[]): Model[] {
  return [...localToModels(lib), ...cloudToModels(cloud)];
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
          kind: a.kind === "pdf" ? "pdf" : "image",
          name: a.name,
          path: a.path,
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

export const useAppStore = create<AppState>((set, get) => ({
  bootstrapped: false,

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

  // Selecting a local model loads it into the engine; cloud models (later)
  // just become the active choice without spawning anything.
  selectModel: (id) => {
    set({ selectedModelId: id });
    const m = get().models.find((x) => x.id === id);
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
      const models = composeModels(lib, s.cloudModels);
      return {
        libraryModels: lib,
        models,
        selectedModelId: models.find((m) => m.id === s.selectedModelId)
          ? s.selectedModelId
          : lib.find((e) => e.is_default)?.id ?? models[0]?.id ?? s.selectedModelId,
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
    set((s) => ({
      providers,
      cloudModels,
      models: composeModels(s.libraryModels, cloudModels),
    }));
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
  imageMode: false,
  setImageMode: (imageMode) => set({ imageMode }),

  createImage: async (prompt, modelPath) => {
    const state = get();
    const convId = state.activeConversationId;
    const text = prompt.trim();
    if (!convId || state.busy || !text) return;
    const conv = state.conversations.find((c) => c.id === convId);
    const isFirstMessage = !conv || conv.messages.length === 0;

    const userMsg: Message = {
      id: `u-${Date.now()}`,
      role: "user",
      text,
      createdAt: Date.now(),
    };
    const assistantId = `a-${Date.now()}`;
    const assistantMsg: Message = {
      id: assistantId,
      role: "assistant",
      model: { name: "Image", provenance: "local" },
      text: "Creating image…",
      streaming: true,
      createdAt: Date.now() + 1,
    };
    set((s) => ({
      busy: true,
      conversations: s.conversations.map((c) =>
        c.id === convId
          ? { ...c, updatedAt: Date.now(), messages: [...c.messages, userMsg, assistantMsg] }
          : c
      ),
    }));
    if (isFirstMessage) get().renameConversation(convId, deriveTitle(text));

    if (!api.inTauri()) {
      patchAssistant(set, convId, assistantId, {
        text: "_Run the desktop app to generate images._",
        streaming: false,
      });
      set({ busy: false });
      return;
    }

    try {
      await api.appendMessage({ conversationId: convId, role: "user", content: text });
    } catch {
      /* non-fatal */
    }
    try {
      const path = await api.generateImage({ prompt: text, modelPath: modelPath ?? undefined });
      const attachment: Attachment = { id: `img-${Date.now()}`, kind: "image", name: "image.png", path };
      patchAssistant(set, convId, assistantId, {
        text: "",
        streaming: false,
        attachments: [attachment],
      });
      try {
        await api.appendMessage({
          conversationId: convId,
          role: "assistant",
          content: "",
          modelName: "Image",
          modelProvenance: "local",
          attachments: [{ kind: "image", name: "image.png", path }],
        });
      } catch {
        /* non-fatal */
      }
    } catch (e) {
      patchAssistant(set, convId, assistantId, {
        text: `That didn't work: ${String(e)}`,
        streaming: false,
      });
    } finally {
      set({ busy: false });
    }
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
  createPersona: async ({ name, systemPrompt, modelId, temperature }) => {
    if (!api.inTauri()) return;
    const paramsJson =
      typeof temperature === "number" ? JSON.stringify({ temperature }) : null;
    await api.createPersona({ name, systemPrompt, modelId: modelId ?? null, paramsJson });
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

    const [rows, prompt, readingScaleRaw, telemetryRaw] = await Promise.all([
      api.listConversations(),
      api.getSetting(SYSTEM_PROMPT_KEY),
      api.getSetting(READING_SCALE_KEY),
      api.getSetting(TELEMETRY_KEY),
    ]);
    let conversations = rows.map(toConversation);
    if (conversations.length === 0) {
      const created = await api.createConversation("New chat");
      conversations = [toConversation(created)];
    }
    const readingScale = readingScaleRaw ? Number(readingScaleRaw) || 1 : 1;
    applyReadingScale(readingScale);
    set({
      conversations,
      activeConversationId: conversations[0].id,
      systemPrompt: prompt ?? DEFAULT_SYSTEM_PROMPT,
      readingScale,
      telemetryEnabled: telemetryRaw === "true",
      bootstrapped: true,
    });
    await get().refreshLibrary();
    // Cloud models load in the background (network) — don't block startup.
    get().refreshCloud();
    get().refreshPersonas();
    await get().setActiveConversation(conversations[0].id);
  },

  setActiveConversation: async (id) => {
    // A conversation carries its own workspace flag — switching sessions adopts
    // that session's layout (composed surface vs. classic message stream).
    const conv = get().conversations.find((c) => c.id === id);
    set({
      activeConversationId: id,
      view: "chat",
      canvasOpen: false,
      workspaceMode: !!conv?.workspace,
    });
    if (!api.inTauri()) return;
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
    const messages = rows.map(toMessage);
    for (const m of messages) {
      if (blocksByMessage[m.id]) m.blocks = blocksByMessage[m.id];
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
    }));
    // Load any saved artifacts for the Canvas panel (CHT-6).
    try {
      const arts = await api.listArtifacts(id);
      set((s) => ({
        artifacts: { ...s.artifacts, [id]: arts },
        activeArtifactId: arts.length ? arts[arts.length - 1].id : null,
      }));
    } catch {
      /* ignore */
    }
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
      conversations: s.conversations.map((c) =>
        c.id === convId
          ? { ...c, updatedAt: Date.now(), messages: [...c.messages, userMsg, assistantMsg] }
          : c
      ),
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
    const effectiveSystemPrompt = composeSystemPrompt(baseSystemPrompt, {
      conv: get().conversations.find((c) => c.id === convId),
      sessionState: get().sessionState[convId],
      toolsEnabled: get().toolsEnabled,
      surface: get().surfaces[convId],
    });
    const effectiveTemperature =
      conv?.overrides?.temperature ?? personaTemperature(persona);

    // Build the model context: system prompt + prior turns + this one.
    const priorTurns = (conv?.messages ?? [])
      .filter((m) => m.text.trim().length > 0)
      .map((m) => ({ role: m.role as "user" | "assistant", content: m.text }));
    const turns: api.ChatTurnMessage[] = [
      { role: "system", content: effectiveSystemPrompt },
      ...priorTurns,
      { role: "user", content: userContent },
    ];

    await streamAssistantTurn(set, get, {
      convId,
      assistantId,
      persistedAssistantId,
      turns,
      model,
      temperature: effectiveTemperature,
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
    const modelContent = `${humanText}\n\n\`\`\`nexus-action\n${JSON.stringify(payload)}\n\`\`\``;

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
      conversations: s.conversations.map((c) =>
        c.id === convId
          ? { ...c, updatedAt: Date.now(), messages: [...c.messages, userMsg, assistantMsg] }
          : c
      ),
    }));

    if (!api.inTauri()) {
      patchAssistant(set, convId, assistantId, { streaming: false });
      set(() => ({ busy: false }));
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
      set(() => ({ busy: false }));
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
    const effectiveSystemPrompt = composeSystemPrompt(baseSystemPrompt, {
      conv: get().conversations.find((c) => c.id === convId),
      sessionState: get().sessionState[convId],
      toolsEnabled: get().toolsEnabled,
      surface: get().surfaces[convId],
    });
    const effectiveTemperature =
      conv?.overrides?.temperature ?? personaTemperature(persona);

    const priorTurns = (conv?.messages ?? [])
      .filter((m) => m.text.trim().length > 0)
      .map((m) => ({ role: m.role as "user" | "assistant", content: m.text }));
    const turns: api.ChatTurnMessage[] = [
      { role: "system", content: effectiveSystemPrompt },
      ...priorTurns,
      { role: "user", content: modelContent },
    ];

    await streamAssistantTurn(set, get, {
      convId,
      assistantId,
      persistedAssistantId,
      turns,
      model,
      temperature: effectiveTemperature,
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
    set((s) => ({ pendingPermissions: s.pendingPermissions.filter((p) => p.id !== id) }));
    if (api.inTauri()) await api.resolvePermission(id, decision);
  },

  artifacts: {},
  canvasOpen: false,
  activeArtifactId: null,
  openCanvas: (artifactId) =>
    set((s) => ({ canvasOpen: true, activeArtifactId: artifactId ?? s.activeArtifactId })),
  closeCanvas: () => set({ canvasOpen: false }),
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
      activeArtifactId: artifact.id,
      canvasOpen: true,
      view: artifact.conversation_id ? "chat" : s.view,
    }));
  },
}));

type StoreSet = (fn: (s: AppState) => Partial<AppState>) => void;

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
  }
): Promise<void> {
  const { convId, assistantId, persistedAssistantId, turns, model, temperature } = opts;
  let acc = "";
  const steps: AgentStep[] = [];
  const blocks: BlockView[] = [];
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
            };
            set((st) => {
              const existing = st.artifacts[convId] ?? [];
              return {
                artifacts: { ...st.artifacts, [convId]: [...existing, artifact] },
                canvasOpen: true,
                activeArtifactId: e.id,
              };
            });
            break;
          }
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
        target:
          model.provenance === "cloud"
            ? { provenance: "cloud", provider: model.provider, model: model.cloudModel }
            : { provenance: "local" },
      }
    );
  } catch (err) {
    acc = acc || `That didn't work: ${String(err)}`;
    patchAssistant(set, convId, assistantId, { text: acc, streaming: false });
  } finally {
    set(() => ({ busy: false }));
    try {
      const stepsJson = steps.length ? JSON.stringify(steps) : undefined;
      await api.finalizeMessage(persistedAssistantId, acc, stepsJson);
    } catch {
      /* ignore */
    }
  }
}

// ---- session state helpers (Generative UI, Phase C) ----

/** Append a compact rendering of durable session state to the system prompt. */
function withSessionState(prompt: string, state: Record<string, unknown> | undefined): string {
  if (!state || Object.keys(state).length === 0) return prompt;
  return `${prompt}\n\n## Session state (durable; update with the remember tool)\n${JSON.stringify(state)}`;
}

/** Assemble the full system prompt for a turn: base persona/prompt, then the
 * live workspace-block registry (W3), durable session state, and the
 * block-usage guidance (W4/W5). Kept in one place so `sendMessage` and
 * `sendBlockAction` build identical context. */
function composeSystemPrompt(
  base: string,
  opts: {
    conv: Conversation | undefined;
    sessionState: Record<string, unknown> | undefined;
    toolsEnabled: boolean;
    surface?: BlockView;
  }
): string {
  let out = base;
  // Only mention blocks/surface machinery when the model can actually call the
  // tools — otherwise it imitates tool-call JSON as prose and it leaks raw.
  if (opts.toolsEnabled) {
    const registry = blockRegistry(opts.conv);
    if (registry) out += `\n\n${registry}`;
    const surface = surfaceContext(opts.surface);
    if (surface) out += `\n\n${surface}`;
  }
  out = withSessionState(out, opts.sessionState);
  if (opts.toolsEnabled) out += `\n\n${SURFACE_GUIDANCE}\n\n${BLOCK_GUIDANCE}`;
  return out;
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
].join("\n");

/** W4/W5: teach the model to treat blocks as the surface, not to narrate them,
 * and to acknowledge bare interactions briefly. Only added when tools are on. */
const BLOCK_GUIDANCE = [
  "## Presenting blocks",
  "When you present a block (comparison, plan, collection, form, progress, document), the user sees it rendered in full in their workspace. Do NOT restate the block's contents in prose — after presenting, conclude in at most two sentences.",
  "To change a block that already exists, call `present` with that block's existing `block_id` (see the workspace-block list above) rather than creating a new one.",
  "If the user's message is only a block interaction (a workspace update, or a `nexus-action`), acknowledge it in one short sentence and do not present a menu of follow-up options.",
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
