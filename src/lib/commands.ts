/**
 * The command registry, frontend side (`REG-3`).
 *
 * One manifest (`shared/commands.json`) names every `/` command; this file gives
 * each one a handler over a store action that already exists, decides which are
 * available right now and why not (Rule 5), and parses what the user typed.
 * `+` adds content to a message, `/` does something to the conversation or the
 * run — the two never share an item.
 */
import manifest from "../../shared/commands.json";
import { cloudTarget, useAppStore, type AppState } from "./store";
import * as api from "./api";
import { ranked } from "./fuzzy";
import { scheduleDraft } from "./when";
import { parseGoalArgs } from "./goal";
import { openVoice } from "./voice/controller";
import type { Model } from "./types";

export type CommandKind = "ui" | "action" | "modifier" | "skill";
export type CommandSection =
  | "turn"
  | "conversation"
  | "me"
  | "modes"
  | "agents"
  | "create"
  | "work"
  | "inspect"
  | "skills";
export type ArgSource = "effort" | "personas" | "models" | "turns" | "facts" | "skills" | "onoff";
export type Need = "conv" | "run" | "norun" | "folder" | "changes" | "interrupted" | "turn";

export interface CommandArgs {
  type: "none" | "text" | "choice" | "number";
  hint?: string;
  required?: boolean;
  source?: ArgSource;
}

export interface CommandSpec {
  name: string;
  aliases: string[];
  section: CommandSection;
  kind: CommandKind;
  args: CommandArgs;
  summary: string;
  who: "user" | "both";
  agentClass?: string;
  suggestible?: boolean;
  needs: Need[];
  trace?: boolean;
}

export interface CommandView extends CommandSpec {
  /** Why it can't run right now, or `null`. Shown in place of the summary. */
  disabledReason: string | null;
  /** Meaningless in this state (`/stop` with no run): left out of the menu. */
  hidden: boolean;
  source: "builtin" | "skill" | "mcp";
  skill?: api.SkillView;
  /** The connector's prompt behind a `mcp` command. */
  prompt?: api.McpPromptView;
}

export interface ParsedCommand {
  name: string;
  args: string;
  spec: CommandView;
}

export type CommandResult =
  /** Finished. `note` is what the transcript says about it. */
  | { kind: "done"; note?: string }
  /** A chip for the next message; the input is cleared. With `send`, the words
   * after the argument go out as the message at once (`/plan add dark mode`). */
  | { kind: "modifier"; send?: string }
  /** A skill command: send this as the user's message. */
  | { kind: "send"; text: string; skill?: string; skillArgs?: string }
  /** The command needs an argument the user has not given: back to the argument phase. */
  | { kind: "needsArg"; message?: string }
  /** Ask first, then run. */
  | { kind: "confirm"; title: string; body?: string; confirmLabel: string; run: () => Promise<CommandResult | void> }
  /** `/help`: open the whole menu. */
  | { kind: "help" }
  | { kind: "error"; message: string };

export const SECTION_ORDER: CommandSection[] = [
  "turn",
  "conversation",
  "me",
  "modes",
  "agents",
  "create",
  "work",
  "inspect",
  "skills",
];

export const SECTION_LABEL: Record<CommandSection, string> = {
  turn: "This turn",
  conversation: "Conversation",
  me: "Me",
  modes: "Modes and persona",
  agents: "Agents and time",
  create: "Create",
  work: "Work",
  inspect: "Inspect",
  skills: "My skills",
};

export const SPECS: CommandSpec[] = (manifest as unknown as CommandSpec[]).map((c) => ({
  ...c,
  aliases: c.aliases ?? [],
  needs: c.needs ?? [],
}));

const BY_NAME = new Map<string, CommandSpec>();
for (const spec of SPECS) {
  BY_NAME.set(spec.name, spec);
  for (const alias of spec.aliases) BY_NAME.set(alias, spec);
}

export function isBuiltinName(name: string): boolean {
  return BY_NAME.has(name.toLowerCase());
}

// ---- state the `needs` list asks about ----

const INTERRUPTED = new Set(["aborted", "timeout", "max_steps"]);

/** The persisted assistant turns of the active chat, newest last. */
function answers(s: AppState) {
  const conv = s.conversations.find((c) => c.id === s.activeConversationId);
  return (conv?.messages ?? []).filter(
    (m) => m.role === "assistant" && !m.streaming && !m.id.startsWith("a-") && m.text.trim().length > 0
  );
}

/** Why a need is not met, or `null` when it is. */
function unmet(need: Need, s: AppState): { reason: string; hide: boolean } | null {
  const convId = s.activeConversationId;
  const conv = s.conversations.find((c) => c.id === convId);
  switch (need) {
    case "conv":
      return conv ? null : { reason: "needs a conversation", hide: false };
    case "run":
      return s.activeRun ? null : { reason: "nothing is running", hide: true };
    case "norun":
      return s.busy ? { reason: "I'm busy right now", hide: false } : null;
    case "folder":
      return conv?.folderPath ? null : { reason: "needs a working folder", hide: false };
    case "changes": {
      const set = convId ? s.changeSets[convId] : undefined;
      return set && set.files.length > 0
        ? null
        : { reason: "I haven't changed any files", hide: false };
    }
    case "interrupted": {
      const last = conv?.messages[conv.messages.length - 1];
      return last?.role === "assistant" && !last.streaming && last.stopReason && INTERRUPTED.has(last.stopReason)
        ? null
        : { reason: "nothing to continue", hide: true };
    }
    case "turn":
      return answers(s).length > 0 ? null : { reason: "nothing to go on yet", hide: false };
  }
}

function resolveNeeds(needs: Need[], s: AppState): { disabledReason: string | null; hidden: boolean } {
  let reason: string | null = null;
  for (const n of needs) {
    const u = unmet(n, s);
    if (!u) continue;
    if (u.hide) return { disabledReason: u.reason, hidden: true };
    reason = reason ?? u.reason;
  }
  return { disabledReason: reason, hidden: false };
}

/** The menu's commands for this moment: built-ins, then the user's skills.
 *
 * Builds a fresh array, so call it inside `useMemo` over stable slices — never
 * inside a zustand selector (v5 compares snapshots by identity and a selector
 * that returns a new array re-renders forever). */
export function commandViews(s: AppState): CommandView[] {
  const views: CommandView[] = SPECS.map((spec) => ({
    ...spec,
    ...resolveNeeds(spec.needs, s),
    source: "builtin" as const,
  }));
  const skills = s.skills
    .filter((sk) => sk.enabled && sk.user_invocable !== false && !isBuiltinName(sk.name))
    // Most used first; the sort is stable, so ties keep the store's order.
    .map((sk, order) => ({ sk, order }))
    .sort((a, b) => b.sk.used - a.sk.used || a.order - b.order)
    .map(({ sk }) => sk);
  for (const sk of skills) {
    views.push({
      name: sk.name,
      aliases: [],
      section: "skills",
      kind: "skill",
      args: { type: "text", hint: sk.argument_hint ?? "details (optional)", required: false },
      summary: skillSummary(sk),
      who: "user",
      needs: [],
      disabledReason: null,
      hidden: false,
      source: "skill",
      skill: sk,
    });
  }
  // A connected server's prompts sit beside the skills (`UCM`). A name that is
  // already a command, a skill, or another prompt gets its connector's name in
  // front, so nothing is ever shadowed or ambiguous.
  // Aliases count too: `/clear` must stay the built-in, since a name match wins
  // over an alias match in `parseCommandLine`.
  const taken = new Set(views.flatMap((v) => [v.name, ...v.aliases].map((n) => n.toLowerCase())));
  for (const p of s.mcpPrompts) {
    let name = commandName(p.name);
    if (!name) continue;
    if (taken.has(name.toLowerCase())) name = commandName(`${p.connector_name}-${p.name}`);
    if (!name || taken.has(name.toLowerCase())) continue;
    taken.add(name.toLowerCase());
    views.push({
      name,
      aliases: [],
      section: "skills",
      kind: "skill",
      args: { type: "text", hint: promptHint(p), required: p.arguments.some((a) => a.required) },
      summary: p.description ? `from ${p.connector_name}: ${p.description}` : `from ${p.connector_name}`,
      who: "user",
      needs: [],
      disabledReason: null,
      hidden: false,
      source: "mcp",
      prompt: p,
    });
  }
  return views;
}

/** A prompt's name as something that can be typed after `/`. */
function commandName(raw: string): string {
  return raw.trim().replace(/[^A-Za-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "");
}

/** `topic ⟨name⟩` style hint: required arguments plain, optional ones with `?`. */
function promptHint(p: api.McpPromptView): string {
  if (!p.arguments.length) return "details (optional)";
  return p.arguments.map((a) => (a.required ? a.name : `${a.name}?`)).join(" ");
}

/** What a connector's prompt is asked with. `name=value` fills that argument
 * (quote a value with spaces); the words left over go to the first argument that
 * is still empty, so a prompt with one argument just takes what is typed. */
export function parsePromptArgs(
  text: string,
  defs: api.McpPromptView["arguments"]
): { values: Record<string, string>; missing: string[] } {
  const values: Record<string, string> = {};
  const names = new Set(defs.map((d) => d.name));
  let rest = text;
  rest = rest.replace(/(^|\s)([A-Za-z0-9_-]+)=("([^"]*)"|\S+)/g, (whole, lead: string, key: string, raw: string, quoted?: string) => {
    if (!names.has(key)) return whole;
    values[key] = quoted ?? raw;
    return lead;
  });
  rest = rest.trim();
  if (rest) {
    const target = defs.find((d) => values[d.name] === undefined);
    if (target) values[target.name] = rest;
  }
  const missing = defs.filter((d) => d.required && !values[d.name]).map((d) => d.name);
  return { values, missing };
}

/** `YYYY-MM-DD` as `2 Oct`. `null` for anything that is not one. */
function shortDate(iso: string | null | undefined): string | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso ?? "");
  if (!m) return null;
  const month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][
    Number(m[2]) - 1
  ];
  return month ? `${Number(m[3])} ${month}` : null;
}

/** What a skill's row says (`CPX-4`). One I wrote myself says where it came
 * from, once, and nothing else — no badge, no count, no highlight. */
function skillSummary(sk: api.SkillView): string {
  if (sk.origin === "poiesis") {
    const when = shortDate(sk.created);
    return when ? `I learned this on ${when}` : "I learned this";
  }
  return sk.when_to_use ? `when: ${sk.when_to_use}` : sk.description;
}

/** Everything the menu may show, hidden ones dropped. */
export const visibleViews = (views: CommandView[]) => views.filter((v) => !v.hidden);

/** Ranked on the name, then aliases, then the summary; name hits come first. */
export function rankCommands(views: CommandView[], query: string): CommandView[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return views;
  const byName = ranked(views, (v) => v.name, words, 100);
  const rest = views.filter((v) => !byName.includes(v));
  const byAlias = ranked(rest, (v) => v.aliases.join(" "), words, 100);
  const rest2 = rest.filter((v) => !byAlias.includes(v));
  const bySummary = ranked(rest2, (v) => v.summary, words, 100);
  return [...byName, ...byAlias, ...bySummary];
}

/** Only a leading `/` followed by a known name counts. An unknown `/word` (a
 * path, a date, a skill that isn't enabled) is an ordinary message. */
export function parseCommandLine(text: string, views: CommandView[]): ParsedCommand | null {
  const m = /^\/([A-Za-z0-9_-]+)(?:\s+([\s\S]*))?$/.exec(text.trim() === text ? text : text.trimStart());
  if (!m) return null;
  const word = m[1].toLowerCase();
  const spec =
    views.find((v) => v.name.toLowerCase() === word) ??
    views.find((v) => v.aliases.some((a) => a.toLowerCase() === word));
  if (!spec) return null;
  return { name: spec.name, args: (m[2] ?? "").trim(), spec };
}

// ---- argument choices (`CMP-5`) ----

export interface ArgChoice {
  /** What goes after the command name when this is chosen. */
  value: string;
  label: string;
  hint?: string;
}

export const EFFORT_CHOICES: ArgChoice[] = [
  { value: "off", label: "Off", hint: "answer straight away" },
  { value: "low", label: "Low", hint: "a moment's thought" },
  { value: "medium", label: "Medium", hint: "a few moving parts" },
  { value: "high", label: "High", hint: "slow and costly" },
];

export function isChatModel(m: Model): boolean {
  return !m.modality || m.modality === "chat";
}

/** The options for a `choice` argument, filtered by what has been typed. */
export function argChoices(
  source: ArgSource,
  typed: string,
  s: AppState,
  extra?: { facts?: api.Fact[] }
): ArgChoice[] {
  const words = typed.toLowerCase().split(/\s+/).filter(Boolean);
  const filter = (list: ArgChoice[]) =>
    words.length ? ranked(list, (c) => `${c.value} ${c.label}`, words, 20) : list;
  switch (source) {
    case "effort":
      return filter(EFFORT_CHOICES);
    case "onoff":
      return filter([
        { value: "on", label: "On" },
        { value: "off", label: "Off" },
      ]);
    case "personas":
      return filter([
        { value: "none", label: "No persona" },
        ...s.personas.map((p) => ({ value: p.name, label: p.name })),
      ]);
    case "models": {
      const favourites = new Set(s.modelPrefs.favorites.chat);
      const chat = s.models.filter(isChatModel);
      const sorted = [...chat].sort((a, b) => Number(favourites.has(b.id)) - Number(favourites.has(a.id)));
      return filter(sorted.map((m) => ({ value: m.name, label: m.name, hint: m.provenance })));
    }
    case "skills":
      return filter(
        s.skills
          .filter((k) => k.enabled && !isBuiltinName(k.name))
          .map((k) => ({ value: k.name, label: k.name, hint: k.description }))
      );
    case "facts":
      return filter(
        (extra?.facts ?? []).map((f) => ({
          value: f.name,
          label: f.name,
          hint: f.description,
        }))
      );
    case "turns":
      // `RWD-UI-3`: your last ten messages, newest first. The number is how far
      // back; the label is what you said.
      return filter(
        userTurns(s)
          .slice(0, 10)
          .map((m, i) => ({ value: String(i + 1), label: `${i + 1} · ${preview(m.text)}`, hint: undefined }))
      );
  }
}

/** The persisted messages you wrote in the active chat, newest first. */
export function userTurns(s: AppState) {
  const conv = s.conversations.find((c) => c.id === s.activeConversationId);
  return (conv?.messages ?? [])
    .filter((m) => m.role === "user" && !m.id.startsWith("u-") && m.text.trim().length > 0)
    .reverse();
}

function preview(text: string): string {
  const first = (text.split(/\r?\n/)[0] ?? "").trim();
  return first.length > 48 ? `${first.slice(0, 47).trimEnd()}…` : first;
}

// ---- handlers ----

type Handler = (args: string, s: AppState) => Promise<CommandResult> | CommandResult;

const done = (note?: string): CommandResult => ({ kind: "done", note });
const fail = (message: string): CommandResult => ({ kind: "error", message });

function activeId(s: AppState): string | null {
  return s.activeConversationId;
}

function lastAnswer(s: AppState) {
  const list = answers(s);
  return list[list.length - 1];
}

/** `/effort high add tests` → `high` and `add tests`. */
function splitFirst(args: string): { head: string; tail: string } {
  const t = args.trim();
  const i = t.search(/\s/);
  return i < 0 ? { head: t, tail: "" } : { head: t.slice(0, i), tail: t.slice(i).trim() };
}

function onOff(args: string): boolean | null | "bad" {
  const a = args.trim().toLowerCase();
  if (!a) return null;
  if (["on", "yes", "true", "1"].includes(a)) return true;
  if (["off", "no", "false", "0"].includes(a)) return false;
  return "bad";
}

function findByName<T extends { name: string }>(list: T[], typed: string): T | undefined {
  const t = typed.trim().toLowerCase();
  return (
    list.find((x) => x.name.toLowerCase() === t) ??
    ranked(list, (x) => x.name, t.split(/\s+/).filter(Boolean), 1)[0]
  );
}

/** One tidy-up at a time: it is a model call, and the input stays put while it runs. */
let tidying = false;

export const HANDLERS: Record<string, Handler> = {
  // ---- this turn ----
  effort: (args, s) => {
    const { head, tail } = splitFirst(args);
    const v = head.toLowerCase();
    if (!v) return { kind: "needsArg" };
    if (!EFFORT_CHOICES.some((c) => c.value === v)) return fail("Say off, low, medium or high.");
    s.setTurnModifier({ effort: v });
    return { kind: "modifier", send: tail || undefined };
  },
  steps: (args, s) => {
    const { head, tail } = splitFirst(args);
    if (!head) return { kind: "needsArg" };
    const n = Number(head);
    if (!Number.isFinite(n) || n < 1) return fail("Give me a number of steps from 1 to 50.");
    s.setTurnModifier({ maxSteps: Math.min(50, Math.round(n)) });
    return { kind: "modifier", send: tail || undefined };
  },
  plan: (args, s) => {
    // `PLF`: the next run may read but not change anything. `/plan add dark mode`
    // sets the chip and sends the words at once.
    s.setTurnModifier({ planFirst: true, planFirstIsDefault: undefined });
    return { kind: "modifier", send: args.trim() || undefined };
  },
  btw: (args, s) => {
    const q = args.trim();
    if (!q) return { kind: "needsArg" };
    if (!api.inTauri()) return fail("I can only answer on the side in the desktop app.");
    // `BTW-1`: it never touches a live run; the card is not the conversation.
    void s.askSide(q);
    return done();
  },
  stop: (_args, s) => {
    s.stopGenerating();
    return done();
  },
  continue: async (_args, s) => {
    await s.resumeLastRun();
    return done();
  },
  retry: async (_args, s) => {
    const last = lastAnswer(s);
    if (!last) return fail("There is nothing to try again yet.");
    await s.forkFromMessage(last.id);
    return done();
  },

  // ---- conversation ----
  new: async (args, s) => {
    const typed = args.trim();
    if (!typed) {
      await s.newConversation();
      return done();
    }
    const skill = findByName(
      s.skills.filter((k) => k.enabled),
      typed
    );
    if (!skill) return fail(`I don't have a skill called "${typed}".`);
    await s.startFromSkill(skill);
    return done();
  },
  rename: async (args, s) => {
    const id = activeId(s);
    const title = args.trim();
    if (!id) return fail("Start a conversation first.");
    if (!title) return { kind: "needsArg" };
    await s.renameConversation(id, title);
    return done(`renamed this chat to "${title}"`);
  },
  fork: async (_args, s) => {
    const id = activeId(s);
    const last = lastAnswer(s);
    if (!id || !last || !api.inTauri()) return fail("There is nothing to branch yet.");
    // `UCM-2`: inclusive, so the branch is the conversation exactly as it stands.
    const { conversation } = await api.forkConversation(id, last.id, true);
    await s.openSessionFromFork(conversation);
    return done("branched this chat");
  },
  rewind: (args, s) => {
    const turns = userTurns(s);
    const n = Number(args.trim());
    if (!args.trim()) return { kind: "needsArg" };
    const turn = Number.isInteger(n) && n >= 1 ? turns[n - 1] : undefined;
    if (!turn) return fail("I can't find that turn. Pick one from the list.");
    // `RWD-UI-1`: the same dialog as the button on a turn.
    s.requestRewind(turn.id);
    return done();
  },
  export: async (_args, s) => {
    const id = activeId(s);
    if (!id) return fail("Start a conversation first.");
    if (!api.inTauri()) return fail("I can only save files in the desktop app.");
    // Leaving the dialog without choosing a place saves nothing, and says nothing.
    await s.exportConversation(id);
    return done();
  },
  compact: async (args, s) => {
    const id = activeId(s);
    if (!id || !api.inTauri()) return fail("Start a conversation first.");
    const note = await s.compactNow(id, args.trim() || undefined);
    return note ? fail(note) : done();
  },
  context: (_args, s) => {
    const id = activeId(s);
    if (!id) return fail("Start a conversation first.");
    s.openContextPanel({ conversationId: id });
    return done();
  },
  why: (_args, s) => {
    const id = activeId(s);
    const last = lastAnswer(s);
    if (!id || !last) return fail("I haven't answered anything yet.");
    s.openContextPanel({ conversationId: id, messageId: last.id });
    return done();
  },
  talk: (_args, s) => {
    // `VOC-UI-2`: voice mode lives over the chat, so the mic is always seen (VXP-3).
    if (s.workspaceMode) return fail("Voice opens in the chat view. Leave workspace mode first.");
    // The voice surface says itself what is missing or went wrong.
    void openVoice();
    return done();
  },

  // ---- me ----
  self: (_args, s) => {
    s.openSelf("memory");
    return done();
  },
  autonomy: (_args, s) => {
    s.openSelf("autonomy");
    return done();
  },

  remember: async (args, s) => {
    const text = args.trim();
    if (!text) return { kind: "needsArg" };
    if (!api.inTauri()) return fail("I can only keep notes in the desktop app.");
    const saved = await api.rememberFact(activeId(s), text);
    // The same toast, and the same Undo, as a fact I saved myself (`MEM-UI-3`).
    useAppStore.setState({
      memoryToast: {
        op: "save",
        name: saved.name,
        description: saved.description,
        collection: "facts",
        undoToken: "",
      },
    });
    await s.refreshMemoryContext();
    s.noteGlobalFactChange();
    void s.maybeOfferRecall();
    return done(`remembered “${saved.description}”`);
  },
  forget: async (args, s) => {
    const typed = args.trim();
    if (!typed) return { kind: "needsArg" };
    if (!api.inTauri()) return fail("I can only forget notes in the desktop app.");
    const facts = await api.listMemoryFacts();
    const exact = facts.find((f) => f.name.toLowerCase() === typed.toLowerCase());
    const matches = exact
      ? [exact]
      : ranked(facts, (f) => `${f.name} ${f.description}`, typed.toLowerCase().split(/\s+/).filter(Boolean), 5);
    if (matches.length === 0) return fail(`I don't know anything like "${typed}".`);
    if (matches.length > 1) return fail("That fits more than one thing I know. Pick one from the list.");
    const fact = matches[0];
    const token = await api.forgetMemoryFact(fact.name);
    useAppStore.setState({
      memoryToast: {
        op: "forget",
        name: fact.name,
        description: fact.description,
        collection: "facts",
        undoToken: token,
      },
    });
    await s.refreshMemoryContext();
    return done(`forgot “${fact.description || fact.name}”`);
  },
  always: async (args, s) => {
    const text = args.trim();
    if (!text) return { kind: "needsArg" };
    if (!api.inTauri()) return fail("I can only keep standing instructions in the desktop app.");
    // `UCM-7`: the user owns the soul, so this is a write, not a proposal. The
    // text it replaces rides on the toast so Undo can put it back exactly.
    const prior = (await api.getMemoryContext()).soul;
    const next = `${prior.trimEnd()}${prior.trim() ? "\n" : ""}- ${text}`;
    await api.setSoul(next);
    useAppStore.setState({
      memoryToast: {
        op: "always",
        name: "standing instructions",
        description: text,
        collection: "facts",
        undoToken: prior,
      },
    });
    await s.refreshMemoryContext();
    return done(`I'll always: ${text}`);
  },
  reflect: (_args, s) => {
    const id = activeId(s);
    if (!id) return fail("Start a conversation first.");
    // Thinking back is a couple of model calls. The rail's mark shows it going
    // (`PRES-2`), so the composer is not held up waiting for it.
    void s.reflectConversation(id);
    return done("thought back over this conversation");
  },
  checkup: (_args, s) => {
    if (!api.inTauri()) return fail("I can only check myself in the desktop app.");
    // Traced by `runCheckup` itself: its result is a card, persisted in the note.
    void s.runCheckup();
    return done();
  },
  tidy: async (_args, s) => {
    if (!api.inTauri()) return fail("I can only tidy my notes in the desktop app.");
    if (tidying) return fail("I'm already tidying.");
    tidying = true;
    try {
      // A real model call, routed like a chat turn. Nothing is applied: what I
      // propose waits on the Self panel for a yes.
      await api.consolidateMemory(cloudTarget());
      await s.refreshChangeProposals();
    } finally {
      tidying = false;
    }
    s.openSelf("memory");
    return done("tidied up my memory");
  },

  // ---- modes and persona ----
  workspace: (args, s) => {
    const want = onOff(args);
    if (want === "bad") return fail("Say on or off.");
    const next = want === null ? !s.workspaceMode : want;
    s.setWorkspaceMode(next);
    // Workspace needs `render_ui`, so turning it on turns tools on too.
    if (next) s.setToolsEnabled(true);
    return done(next ? "turned Workspace mode on" : "turned Workspace mode off");
  },
  tools: (args, s) => {
    const want = onOff(args);
    if (want === null) return { kind: "needsArg" };
    if (want === "bad") return fail("Say on or off.");
    if (!want && s.workspaceMode) return fail("Workspace mode needs my tools. Turn it off first.");
    s.setToolsEnabled(want);
    return done(want ? "let me use my tools" : "took my tools away");
  },
  persona: async (args, s) => {
    const id = activeId(s);
    if (!id) return fail("Start a conversation first.");
    const typed = args.trim();
    if (!typed) return { kind: "needsArg" };
    if (["none", "no persona"].includes(typed.toLowerCase())) {
      await s.applyPersona(id, null);
      return done("removed the persona");
    }
    const persona = findByName(s.personas, typed);
    if (!persona) return fail(`I don't know a persona called "${typed}".`);
    await s.applyPersona(id, persona.id);
    return done(`became ${persona.name}`);
  },
  model: (args, s) => {
    const typed = args.trim();
    if (!typed) return { kind: "needsArg" };
    const model = findByName(s.models.filter(isChatModel), typed);
    if (!model) return fail(`I don't have a model called "${typed}".`);
    s.selectModel(model.id);
    return done(`switched to ${model.name}`);
  },

  // ---- agents and time ----
  agents: (_args, s) => {
    s.setDockView("agents");
    s.setDockOpen(true);
    return done();
  },
  schedule: (args, s) => {
    const id = activeId(s);
    if (!id) return fail("Start a conversation first.");
    const task = args.trim();
    if (!task) {
      s.scheduleConversation(id);
      return done();
    }
    // `UCM-8`: with words, open Tasks with a draft that already says them.
    // Nothing is scheduled until the user presses Save.
    const conv = s.conversations.find((c) => c.id === id);
    useAppStore.setState({ taskDraft: scheduleDraft(conv?.title ?? "New task", id, task) });
    s.setView("tasks");
    return done();
  },

  // ---- create ----
  image: (args, s) => createCommand("image", args, s),
  video: (args, s) => createCommand("video", args, s),

  // ---- work ----
  goal: (args, s) => {
    const id = activeId(s);
    if (!id) return fail("Start a conversation first.");
    const parsed = parseGoalArgs(args);
    if (parsed === null) {
      return {
        kind: "needsArg",
        message: "Say what you want and when it is done, like /goal fix the build until npm test exits 0.",
      };
    }
    const current = s.goals[id];
    if (parsed === "stop") {
      if (current?.status !== "active") return fail("I'm not working toward a goal here.");
      s.stopGoal(id);
      return done("stopped working toward the goal");
    }
    if (current?.status === "active") return fail("I'm already working toward a goal here. Say /goal stop first.");
    if (s.busy) return fail("I'm busy right now");
    if (!api.inTauri()) return fail("I can only work toward a goal in the desktop app.");
    // Not awaited: the goal is every round of it, and the box is free meanwhile.
    void s.startGoal(id, parsed.text, parsed.until);
    return done(`started a goal: ${parsed.text}`);
  },
  changes: (_args, s) => {
    s.setDockView("changes");
    s.setDockOpen(true);
    return done();
  },
  undo: (args, s) => {
    const id = activeId(s);
    if (!id) return fail("Start a conversation first.");
    const files = s.changeSets[id]?.files ?? [];
    const typed = args.trim();
    const target = typed
      ? files.find((f) => f.path.toLowerCase().endsWith(typed.toLowerCase()))
      : undefined;
    if (typed && !target) return fail(`I didn't change a file called "${typed}".`);
    const n = target ? 1 : files.length;
    return {
      kind: "confirm",
      title: target
        ? `Take back my changes to ${target.path.split(/[\\/]/).pop()}?`
        : `Take back my changes to ${n} file${n === 1 ? "" : "s"}?`,
      body: "The files go back to how they were before I touched them.",
      confirmLabel: "Take back",
      run: async () => {
        await s.undoChanges(id, target?.path);
        return done(target ? "took back my changes to one file" : "took back my file changes");
      },
    };
  },
  keep: async (_args, s) => {
    const id = activeId(s);
    if (!id) return fail("Start a conversation first.");
    await s.keepChanges(id);
    return done("kept my file changes");
  },
  permissions: (_args, s) => {
    s.setView("settings");
    return done();
  },

  // ---- inspect ----
  usage: (_args, s) => {
    s.openUsage(activeId(s));
    return done();
  },
  activity: (_args, s) => {
    s.setView("activity");
    return done();
  },
  help: () => ({ kind: "help" }),
};

/** The message a skill command goes out as: just the words after it, or
 * `Run /name` when there are none. The skill body itself is loaded by the
 * backend before the first token, from `skill` and `skillArgs`. */
function skillSend(cmd: ParsedCommand): CommandResult {
  const args = cmd.args.trim();
  if (cmd.spec.args.required && !args) return { kind: "needsArg" };
  return {
    kind: "send",
    text: args || `Run /${cmd.name}`,
    skill: cmd.name,
    skillArgs: args || undefined,
  };
}

/** A connector's prompt: the server builds the text, with what was typed filled
 * in, and it goes out as the user's message. A server that cannot is said, not
 * swallowed. */
async function mcpSend(cmd: ParsedCommand): Promise<CommandResult> {
  const prompt = cmd.spec.prompt;
  if (!prompt) return fail(`I don't know how to run /${cmd.name} yet.`);
  const { values, missing } = parsePromptArgs(cmd.args, prompt.arguments);
  if (missing.length) {
    return { kind: "needsArg", message: `${cmd.name} needs ${missing.join(", ")}.` };
  }
  try {
    return { kind: "send", text: await api.getMcpPrompt(prompt.connector_id, prompt.name, values) };
  } catch (e) {
    return fail(`${prompt.connector_name} couldn't give me that prompt: ${String(e)}`);
  }
}

/** `/image` and `/video`: pin the intent (the existing chip), or with a prompt
 * make it straight away on the first model for that kind. */
function createCommand(kind: "image" | "video", args: string, s: AppState): CommandResult {
  const prompt = args.trim();
  if (!prompt) {
    useAppStore.setState({ composerPin: { intent: kind, nonce: Date.now() } });
    return done();
  }
  const candidates = s.models.filter((m) => m.modality === kind);
  const model = candidates.find((m) => m.provenance === "local") ?? candidates[0];
  if (!model) return fail(`I don't have a ${kind} model yet.`);
  void s.createMedia({ prompt, modelId: model.id });
  return done();
}

/** Run one parsed command. Skill commands are not handled here: they send a
 * message, which `useCommandInput` does with the result. */
export async function runCommand(cmd: ParsedCommand, s: AppState): Promise<CommandResult> {
  if (cmd.spec.disabledReason) return fail(cmd.spec.disabledReason);
  // `SKC-2`: a skill command sends a message with that skill preloaded, so the
  // model is not trusted to call `skill` itself.
  if (cmd.spec.source === "mcp") return mcpSend(cmd);
  if (cmd.spec.kind === "skill") return skillSend(cmd);
  const handler = HANDLERS[cmd.name];
  if (!handler) return fail(`I don't know how to run /${cmd.name} yet.`);
  try {
    return await handler(cmd.args, s);
  } catch (e) {
    return fail(String(e));
  }
}
