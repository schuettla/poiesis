/** Default and favorite models (`MOD-3`, `MOD-4`).
 *
 * Pure functions over a small settings-backed record, so the rules (the
 * default is always favorite #1, a default can't be unstarred, the fallback
 * order) live in one place and are tested without the store. */
import type { Model } from "./types";
import type { ModelEntry } from "./api";

/** One favorites list per Models tab: chat, and images & video together. */
export type FavoriteTab = "chat" | "media";

export interface ModelPrefs {
  /** Any model id (`local`, `endpoint:`, `cloud:`, `media:`). No video default. */
  defaults: { chat: string | null; image: string | null };
  /** Ordered; the order is the preference. */
  favorites: { chat: string[]; media: string[] };
  /** Last known name per favorite, so one that is unavailable right now can
   * still be named ("Claude Sonnet isn't available"). */
  labels: Record<string, string>;
}

export const PREF_KEYS = {
  defaultChat: "default_model.chat",
  defaultImage: "default_model.image",
  favChat: "models.favorites.chat",
  favMedia: "models.favorites.media",
  labels: "models.labels",
} as const;

export const EMPTY_PREFS: ModelPrefs = {
  defaults: { chat: null, image: null },
  favorites: { chat: [], media: [] },
  labels: {},
};

export function parseIdList(raw: string | null | undefined): string[] {
  if (!raw) return [];
  try {
    const v: unknown = JSON.parse(raw);
    if (!Array.isArray(v)) return [];
    return [...new Set(v.filter((x): x is string => typeof x === "string" && x.length > 0))];
  } catch {
    return [];
  }
}

export function parseLabels(raw: string | null | undefined): Record<string, string> {
  if (!raw) return {};
  try {
    const v: unknown = JSON.parse(raw);
    if (!v || typeof v !== "object" || Array.isArray(v)) return {};
    return Object.fromEntries(
      Object.entries(v as Record<string, unknown>).filter(
        (e): e is [string, string] => typeof e[1] === "string"
      )
    );
  } catch {
    return {};
  }
}

export const isMediaModel = (m: Model | undefined) => m?.modality === "image" || m?.modality === "video";
export const isChatModel = (m: Model | undefined) => !!m && !isMediaModel(m);
export const favoriteTabOf = (m: Model | undefined): FavoriteTab => (isMediaModel(m) ? "media" : "chat");

/** The default that leads a tab's favorites. */
export function defaultFor(prefs: ModelPrefs, tab: FavoriteTab): string | null {
  return tab === "chat" ? prefs.defaults.chat : prefs.defaults.image;
}

function withFirst(list: string[], id: string): string[] {
  return [id, ...list.filter((x) => x !== id)];
}

function remember(prefs: ModelPrefs, model: Model | undefined): Record<string, string> {
  return model ? { ...prefs.labels, [model.id]: model.name } : prefs.labels;
}

/** Make `model` the default for its kind. It becomes favorite #1. Video
 * models can't be the default; asking is a no-op. */
export function setDefault(prefs: ModelPrefs, model: Model): ModelPrefs {
  if (model.modality === "video") return prefs;
  const tab = favoriteTabOf(model);
  const defaults =
    tab === "chat" ? { ...prefs.defaults, chat: model.id } : { ...prefs.defaults, image: model.id };
  return {
    defaults,
    favorites: { ...prefs.favorites, [tab]: withFirst(prefs.favorites[tab], model.id) },
    labels: remember(prefs, model),
  };
}

/** Star or unstar. New stars go to the end. Returns `null` when refused:
 * the default can't be unstarred ("Pick another default first"). */
export function toggleFavorite(prefs: ModelPrefs, model: Model): ModelPrefs | null {
  const tab = favoriteTabOf(model);
  const list = prefs.favorites[tab];
  if (list.includes(model.id)) {
    if (defaultFor(prefs, tab) === model.id) return null;
    return { ...prefs, favorites: { ...prefs.favorites, [tab]: list.filter((x) => x !== model.id) } };
  }
  return {
    ...prefs,
    favorites: { ...prefs.favorites, [tab]: [...list, model.id] },
    labels: remember(prefs, model),
  };
}

/** Move a favorite one slot up (`-1`) or down (`+1`). The default holds slot
 * 1: it doesn't move, and nothing moves above it. */
export function moveFavorite(prefs: ModelPrefs, tab: FavoriteTab, id: string, delta: -1 | 1): ModelPrefs {
  const list = [...prefs.favorites[tab]];
  const from = list.indexOf(id);
  const to = from + delta;
  const floor = defaultFor(prefs, tab) && list[0] === defaultFor(prefs, tab) ? 1 : 0;
  if (from < floor || to < floor || to >= list.length) return prefs;
  [list[from], list[to]] = [list[to], list[from]];
  return { ...prefs, favorites: { ...prefs.favorites, [tab]: list } };
}

/** Apply a whole new order (drag and drop), keeping the default first. */
export function reorderFavorites(prefs: ModelPrefs, tab: FavoriteTab, ids: string[]): ModelPrefs {
  const known = ids.filter((id) => prefs.favorites[tab].includes(id));
  const rest = prefs.favorites[tab].filter((id) => !known.includes(id));
  let list = [...known, ...rest];
  const def = defaultFor(prefs, tab);
  if (def && list.includes(def)) list = withFirst(list, def);
  return { ...prefs, favorites: { ...prefs.favorites, [tab]: list } };
}

/** Favorites that can be used right now, in the user's order. */
export function availableFavorites(models: Model[], prefs: ModelPrefs, tab: FavoriteTab): Model[] {
  const byId = new Map(models.map((m) => [m.id, m]));
  return prefs.favorites[tab].map((id) => byId.get(id)).filter((m): m is Model => !!m);
}

export interface Resolved {
  id: string | undefined;
  /** The default, when it was set but couldn't be used. */
  skippedDefault: string | null;
  /** How `id` was reached. */
  via: "default" | "favorite" | "fallback";
}

/** Which chat model to use when nothing more specific says: the default,
 * then the next available favorite, then today's fallback (the library's
 * marked model, then any chat model). Never a media model. */
export function resolveChatModel(models: Model[], library: ModelEntry[], prefs: ModelPrefs): Resolved {
  const chat = (id: string | null | undefined) => {
    const m = id ? models.find((x) => x.id === id) : undefined;
    return isChatModel(m) ? m : undefined;
  };
  const def = prefs.defaults.chat;
  if (chat(def)) return { id: def!, skippedDefault: null, via: "default" };
  const skippedDefault = def ?? null;
  const fav = prefs.favorites.chat.map(chat).find((m) => !!m);
  if (fav) return { id: fav.id, skippedDefault, via: "favorite" };
  const fallback =
    chat(library.find((e) => e.is_default)?.id)?.id ??
    models.find((m) => isChatModel(m))?.id ??
    models[0]?.id;
  return { id: fallback, skippedDefault, via: "fallback" };
}

/** The composer's one-time line when the default was skipped. */
export function skippedNotice(prefs: ModelPrefs, resolved: Resolved, models: Model[]): string | null {
  if (!resolved.skippedDefault || !resolved.id) return null;
  const was = prefs.labels[resolved.skippedDefault] ?? "Your default model";
  const now = models.find((m) => m.id === resolved.id)?.name ?? "another model";
  return resolved.via === "favorite"
    ? `${was} isn't available right now. Using ${now}, your next favorite.`
    : `${was} isn't available right now. Using ${now}.`;
}

/** Bring stored prefs in line with the rules and the models that exist:
 * seed the chat default from the library's marked model the first time
 * (`MOD-3` migration), keep each default first in its favorites, and give a
 * new install its first favorite (`MOD-4`) once something is usable. */
export function normalizePrefs(prefs: ModelPrefs, library: ModelEntry[], models: Model[]): ModelPrefs {
  let next = prefs;
  if (!next.defaults.chat) {
    const marked = library.find((e) => e.is_default);
    if (marked) {
      next = {
        ...next,
        defaults: { ...next.defaults, chat: marked.id },
        labels: { ...next.labels, [marked.id]: marked.name },
      };
    }
  }
  for (const tab of ["chat", "media"] as const) {
    const def = defaultFor(next, tab);
    if (def && next.favorites[tab][0] !== def) {
      next = { ...next, favorites: { ...next.favorites, [tab]: withFirst(next.favorites[tab], def) } };
    }
  }
  if (next.favorites.chat.length === 0) {
    const first = models.find((m) => isChatModel(m) && m.available !== false);
    if (first) {
      next = {
        ...next,
        favorites: { ...next.favorites, chat: [first.id] },
        labels: { ...next.labels, [first.id]: first.name },
      };
    }
  }
  return next;
}

/** Did anything the settings store holds change? */
export function prefsChanged(a: ModelPrefs, b: ModelPrefs): boolean {
  return JSON.stringify(a) !== JSON.stringify(b);
}
