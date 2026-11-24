/**
 * The phone's state. The desktop is the only place with state (plan, II.2): this holds what was last fetched,
 * the turn that is streaming from this phone, and the questions waiting for an answer.
 */
import { create } from "zustand";
import * as link from "./link";
import { getSpeechBridge } from "./voice/bridge";
import {
  readEvent,
  type Approval,
  type ChatRow,
  type LinkState,
  type PhoneArtifact,
  type PhoneEvent,
  type PhoneMessage,
  type Waiting,
} from "./types";

/** The turn running from this phone, as it arrives. */
export interface Live {
  text: string;
  notice: string | null;
  error: string | null;
  artifacts: { id: string; title: string; kind: string; content: string }[];
  /** One line per block the phone cannot draw. */
  notes: string[];
  helper: boolean;
  /** True from the first message until the stream ends. */
  running: boolean;
}

const blankLive = (): Live => ({ text: "", notice: null, error: null, artifacts: [], notes: [], helper: false, running: true });

/** A photo the person picked, already a data URI. */
export interface Photo {
  name: string;
  dataUri: string;
}

type Screen = "chats" | "chat";

interface Store {
  ready: boolean;
  paired: boolean;
  desktop: string | null;
  link: LinkState;
  screen: Screen;
  chats: ChatRow[];
  active: string | null;
  messages: Record<string, PhoneMessage[]>;
  artifacts: Record<string, PhoneArtifact[]>;
  live: Record<string, Live>;
  approvals: Approval[];
  waiting: Waiting[];
  /** A line for the person when something went wrong outside a chat. */
  error: string | null;

  boot: () => Promise<void>;
  pairWith: (text: string, label: string) => Promise<boolean>;
  unpair: () => Promise<void>;
  refresh: () => Promise<void>;
  resume: () => Promise<void>;
  showChats: () => void;
  openChat: (id: string) => Promise<void>;
  newChat: () => void;
  send: (text: string, photos?: Photo[], opts?: { spoken?: boolean }) => Promise<void>;
  /** One plain line under the latest message of the open chat (for example, that voice stopped). */
  tell: (text: string) => void;
  stop: () => Promise<void>;
  answer: (id: string, allow: boolean) => Promise<void>;
  resolveWaiting: (id: string, accept: boolean, text?: string) => Promise<void>;
}

export const useStore = create<Store>()((set, get) => ({
  ready: false,
  paired: false,
  desktop: null,
  link: "connecting",
  screen: "chats",
  chats: [],
  active: null,
  messages: {},
  artifacts: {},
  live: {},
  approvals: [],
  waiting: [],
  error: null,

  boot: async () => {
    try {
      const s = await link.state();
      set(() => ({ paired: s.paired, desktop: s.name, link: s.link, ready: true }));
      if (!s.paired) return;
      void link.onLink((l) => set(() => ({ link: l })));
      await get().resume();
    } catch (e) {
      set(() => ({ ready: true, error: link.words(e) }));
    }
  },

  pairWith: async (text, label) => {
    try {
      const name = await link.pair(text, label);
      set(() => ({ paired: true, desktop: name, link: "connected", error: null }));
      void link.onLink((l) => set(() => ({ link: l })));
      await get().refresh();
      startFeed();
      return true;
    } catch (e) {
      set(() => ({ error: link.words(e) }));
      return false;
    }
  },

  unpair: async () => {
    stopFeed();
    try {
      await link.forget();
    } catch {
      /* the pairing is dropped here either way */
    }
    set(() => ({ paired: false, desktop: null, chats: [], active: null, messages: {}, artifacts: {}, live: {}, approvals: [], waiting: [], screen: "chats" }));
  },

  refresh: async () => {
    try {
      const [chats, waiting] = await Promise.all([link.call<ChatRow[]>("conversations"), link.call<Waiting[]>("waiting")]);
      set(() => ({ chats: orderChats(chats), waiting, error: null, link: "connected" }));
    } catch (e) {
      set(() => ({ error: link.words(e) }));
    }
  },

  // The app came to the front: reconnect, refresh the list and the questions, and listen again.
  resume: async () => {
    const s = await link.connect().catch(() => null);
    if (s) set(() => ({ link: s.link }));
    await get().refresh();
    startFeed();
  },

  showChats: () => set(() => ({ screen: "chats" })),

  openChat: async (id) => {
    set(() => ({ active: id, screen: "chat" }));
    try {
      await link.call("open_chat", { conversation_id: id });
      await loadChat(id);
    } catch (e) {
      set(() => ({ error: link.words(e) }));
    }
  },

  // An empty chat opens at once; the desktop makes it when the first message goes out, so no empty chats pile up.
  newChat: () => set(() => ({ active: null, screen: "chat" })),

  send: async (text, photos = [], opts = {}) => {
    const words = text.trim();
    if (!words && photos.length === 0) return;
    let conv = get().active;
    if (!conv) {
      try {
        const made = await link.call<{ conversation_id: string }>("new_chat");
        conv = made.conversation_id;
        set((s) => ({ active: made.conversation_id, messages: { ...s.messages, [made.conversation_id]: [] } }));
      } catch (e) {
        set(() => ({ error: link.words(e) }));
        if (opts.spoken) getSpeechBridge()?.fail(link.words(e));
        return;
      }
    }
    const chat = conv;
    const mine: PhoneMessage = {
      id: `local-${Date.now()}`,
      role: "user",
      content: words,
      created_at: Date.now(),
    };
    const attachments = photos.map((p) => ({ kind: "image", name: p.name, dataUri: p.dataUri }));
    // A turn runs in this chat already: the words are added to it (the desktop steers). Its answer keeps arriving on
    // the first stream, so the live turn is left as it is.
    if (get().live[chat]?.running) {
      set((s) => ({ messages: { ...s.messages, [chat]: [...(s.messages[chat] ?? []), mine] } }));
      try {
        await link.stream("send", { text: words, conversation_id: chat, attachments }, (raw) => {
          const event = readEvent(raw);
          if (event) onTurnEvent(chat, event);
        });
      } catch (e) {
        patchLive(chat, (l) => ({ ...l, notice: link.words(e) }));
      }
      return;
    }
    set((s) => ({
      messages: { ...s.messages, [chat]: [...(s.messages[chat] ?? []), mine] },
      live: { ...s.live, [chat]: blankLive() },
    }));
    // A spoken turn hands its answer to the voice session as it arrives, and stays open until it was heard.
    const voice = opts.spoken ? getSpeechBridge() : null;
    const generation = voice?.current() ?? 0;
    try {
      await link.stream("send", { text: words, conversation_id: chat, attachments, spoken: !!opts.spoken }, (raw) => {
        const event = readEvent(raw);
        if (!event) return;
        onTurnEvent(chat, event);
        if (voice && event.type === "token") voice.token(generation, event.text);
        if (voice && event.type === "step_start") voice.step(generation, event.verb);
      });
    } catch (e) {
      patchLive(chat, (l) => ({ ...l, error: link.words(e) }));
      voice?.fail(link.words(e));
    }
    if (voice) await voice.end(generation);
    // The saved chat is the truth: what was streamed is replaced by what the desktop kept.
    await loadChat(chat).catch(() => undefined);
    set((s) => {
      const l = s.live[chat];
      const rest = { ...s.live };
      // A failure line stays until the next message; a good turn leaves nothing behind.
      if (l?.error || l?.notice) rest[chat] = { ...l, running: false, text: "" };
      else delete rest[chat];
      return { live: rest };
    });
    void get().refresh();
  },

  tell: (text) => {
    const chat = get().active;
    if (chat) patchLive(chat, (l) => ({ ...l, notice: text, running: false }), true);
  },

  stop: async () => {
    try {
      await link.call("stop");
    } catch (e) {
      set(() => ({ error: link.words(e) }));
    }
  },

  answer: async (id, allow) => {
    const card = get().approvals.find((a) => a.id === id);
    // The card goes at once. If the desktop says it was no longer open, the line says so.
    set((s) => ({ approvals: s.approvals.filter((a) => a.id !== id) }));
    try {
      await link.call("answer", { request_id: id, allow });
    } catch (e) {
      const chat = card?.conversationId ?? get().active;
      if (chat) patchLive(chat, (l) => ({ ...l, notice: link.words(e), running: false }), true);
      else set(() => ({ error: link.words(e) }));
    }
  },

  resolveWaiting: async (id, accept, text) => {
    try {
      await link.call("resolve_waiting", { id, accept, text });
      set((s) => ({ waiting: s.waiting.filter((w) => w.id !== id) }));
    } catch (e) {
      set(() => ({ error: link.words(e) }));
    }
  },
}));

/** Remote chats first, then by when they last moved. */
export function orderChats(chats: ChatRow[]): ChatRow[] {
  return [...chats].sort((a, b) => {
    const ra = a.remote_sender ? 1 : 0;
    const rb = b.remote_sender ? 1 : 0;
    return rb - ra || b.updated_at - a.updated_at;
  });
}

async function loadChat(id: string): Promise<void> {
  const [messages, artifacts] = await Promise.all([
    link.call<PhoneMessage[]>("messages", { conversation_id: id }),
    link.call<PhoneArtifact[]>("artifacts", { conversation_id: id }),
  ]);
  useStore.setState((s) => ({ messages: { ...s.messages, [id]: messages }, artifacts: { ...s.artifacts, [id]: artifacts } }));
}

function patchLive(chat: string, change: (l: Live) => Live, create = false) {
  useStore.setState((s) => {
    const now = s.live[chat] ?? (create ? { ...blankLive(), running: false } : null);
    return now ? { live: { ...s.live, [chat]: change(now) } } : {};
  });
}

/** One event of the turn this phone started. */
export function onTurnEvent(chat: string, event: PhoneEvent) {
  switch (event.type) {
    case "token":
      patchLive(chat, (l) => ({ ...l, text: l.text + event.text }));
      return;
    case "error":
      patchLive(chat, (l) => ({ ...l, error: event.message }));
      return;
    case "steered":
      patchLive(chat, (l) => ({ ...l, notice: "Added to what I am doing now." }));
      return;
    case "remote_notice":
      patchLive(chat, (l) => ({ ...l, notice: event.text }));
      return;
    case "artifact":
      patchLive(chat, (l) => ({
        ...l,
        artifacts: [...l.artifacts.filter((a) => a.id !== event.id), { id: event.id, title: event.title, kind: event.kind, content: event.content }],
      }));
      return;
    case "block":
      patchLive(chat, (l) => ({ ...l, notes: l.notes.includes(event.title) ? l.notes : [...l.notes, event.title] }));
      return;
    case "sub":
      patchLive(chat, (l) => ({ ...l, helper: true }));
      return;
    case "permission":
      addApproval({ ...event.request, conversationId: chat });
      return;
    case "permission_gone":
      dropApproval(event.id);
      return;
    default:
      return;
  }
}

function addApproval(card: Approval) {
  useStore.setState((s) => (s.approvals.some((a) => a.id === card.id) ? {} : { approvals: [...s.approvals, card] }));
}

function dropApproval(id: string) {
  useStore.setState((s) => ({ approvals: s.approvals.filter((a) => a.id !== id) }));
}

// ---- the feed: events the phone did not start ----

let feedToken = 0;
const FEED_RETRY_MS = 3000;

/** What the desktop pushes: questions that open or close, and a reason to refresh. */
export function onFeed(raw: unknown) {
  if (!raw || typeof raw !== "object") return;
  const { name, payload } = raw as { name?: string; payload?: unknown };
  if (name === "poiesis-remote-turn") {
    const p = payload as { conversation_id?: string; event?: unknown } | null;
    const event = readEvent(p?.event);
    if (!event || !p?.conversation_id) return;
    // The turn this phone started is already followed on its own stream; only the questions are shared.
    if (event.type === "permission") addApproval({ ...event.request, conversationId: p.conversation_id });
    else if (event.type === "permission_gone") dropApproval(event.id);
    else if (event.type === "turn_finalized") {
      const s = useStore.getState();
      if (!s.live[p.conversation_id]?.running && s.messages[p.conversation_id]) void loadChat(p.conversation_id).catch(() => undefined);
    }
  } else if (name === "poiesis-away") {
    void useStore.getState().refresh();
  }
}

/** Listen to the desktop's feed, and again when it drops. A new start ends the old loop: the shell replaces its stream. */
export function startFeed() {
  const token = ++feedToken;
  void (async () => {
    while (token === feedToken) {
      try {
        await link.stream("subscribe", {}, onFeed);
      } catch {
        /* the link dropped: try again below */
      }
      if (token !== feedToken) return;
      await new Promise((r) => setTimeout(r, FEED_RETRY_MS));
    }
  })();
}

export function stopFeed() {
  feedToken++;
}
