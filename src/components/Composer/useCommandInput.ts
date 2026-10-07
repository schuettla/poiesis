import { useEffect, useMemo, useState } from "react";
import { useAppStore } from "../../lib/store";
import * as api from "../../lib/api";
import {
  SECTION_LABEL,
  SECTION_ORDER,
  argChoices,
  commandViews,
  parseCommandLine,
  rankCommands,
  runCommand,
  visibleViews,
  type ArgChoice,
  type CommandResult,
  type CommandSection,
  type CommandView,
} from "../../lib/commands";

/**
 * Everything the composer's `/` menu needs, held in one place (`CMP-1`): the
 * query derived from the text, the highlighted row, dismissal, key handling and
 * running a command.
 *
 * The menu is *derived* from the text rather than held in an "is it open" flag:
 * backspacing past the slash closes it on its own, so it can never be showing
 * for text that isn't there. Only a leading `/` counts — mid-sentence slashes
 * are dates and paths.
 */

const RECENT_KEY = "poiesis.recentCommands";
const RECENT_MAX = 4;

function readRecent(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    const list = raw ? JSON.parse(raw) : [];
    return Array.isArray(list) ? list.filter((x) => typeof x === "string").slice(0, RECENT_MAX) : [];
  } catch {
    return [];
  }
}

function pushRecent(name: string) {
  try {
    const next = [name, ...readRecent().filter((n) => n !== name)].slice(0, RECENT_MAX);
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    /* recents are a convenience */
  }
}

/** One selectable row of the name phase. */
export interface CommandRow {
  key: string;
  view: CommandView;
}

export type MenuLine =
  | { kind: "header"; key: string; label: string }
  | { kind: "command"; key: string; row: CommandRow; index: number; sectionHint?: string }
  | { kind: "choice"; key: string; choice: ArgChoice; index: number }
  | { kind: "note"; key: string; text: string };

/** What the menu should draw, and how it is navigated. */
export interface CommandMenuModel {
  open: boolean;
  phase: "name" | "args";
  lines: MenuLine[];
  /** How many lines can be highlighted. */
  count: number;
  index: number;
  /** The `/` button's popover, with its own filter field (`CMP-4`). */
  popover: boolean;
  filter: string;
  /** Said to a screen reader when Enter lands on a disabled row. */
  announcement: string;
  listId: string;
}

/** A menu that is not showing: nothing to draw, nothing to highlight. */
function closed(phase: "name" | "args", announcement: string, listId: string): CommandMenuModel {
  return {
    open: false,
    phase,
    lines: [],
    count: 0,
    index: 0,
    popover: false,
    filter: "",
    announcement,
    listId,
  };
}

export interface PendingConfirm {
  title: string;
  body?: string;
  confirmLabel: string;
  run: () => Promise<CommandResult | void>;
}

interface Options {
  value: string;
  setValue: (v: string) => void;
  focus: () => void;
  /** Send a skill command as a message with the skill preloaded (`SKC-2`). */
  sendSkill: (text: string, skill: string, skillArgs?: string) => void;
  /** Send plain text as the message — the words after a modifier command. */
  sendText: (text: string) => void;
  /** Whether the composer is already talking to a live run. */
  busy: boolean;
}

export function useCommandInput({ value, setValue, focus, sendSkill, sendText, busy }: Options) {
  const skills = useAppStore((s) => s.skills);
  const conversations = useAppStore((s) => s.conversations);
  const activeConversationId = useAppStore((s) => s.activeConversationId);
  const activeRun = useAppStore((s) => s.activeRun);
  const changeSets = useAppStore((s) => s.changeSets);
  const personas = useAppStore((s) => s.personas);
  const models = useAppStore((s) => s.models);
  const modelPrefs = useAppStore((s) => s.modelPrefs);
  const storeBusy = useAppStore((s) => s.busy);

  // `commandViews` builds a fresh array, so it lives in a memo over the slices
  // it reads — not in a selector.
  const views = useMemo(
    () => commandViews(useAppStore.getState()),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [skills, conversations, activeConversationId, activeRun, changeSets, personas, models, modelPrefs, storeBusy]
  );

  const [dismissed, setDismissed] = useState(false);
  const [index, setIndex] = useState(0);
  const [popover, setPopover] = useState(false);
  const [filter, setFilter] = useState("");
  const [announcement, setAnnouncement] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<PendingConfirm | null>(null);
  const [facts, setFacts] = useState<api.Fact[]>([]);

  // ---- parse the text ----
  const parsedText = useMemo(() => {
    const m = /^\/(\S*)(?:\s([\s\S]*))?$/.exec(value);
    if (!m) return null;
    return { name: m[1].toLowerCase(), hasSpace: m[2] !== undefined, rest: m[2] ?? "" };
  }, [value]);

  const argView = useMemo(() => {
    if (!parsedText?.hasSpace) return null;
    return (
      views.find((v) => v.name.toLowerCase() === parsedText.name && !v.hidden) ??
      views.find((v) => v.aliases.some((a) => a.toLowerCase() === parsedText.name) && !v.hidden) ??
      null
    );
  }, [parsedText, views]);

  const phase: "name" | "args" | null = !parsedText
    ? popover
      ? "name"
      : null
    : !parsedText.hasSpace
      ? "name"
      : argView
        ? "args"
        : null;

  const query = popover && !parsedText ? filter : (parsedText?.name ?? "");

  // The menu's text changes → the highlight starts at the top, and a dismissed
  // menu comes back.
  const queryKey = `${phase}|${query}|${argView?.name ?? ""}|${parsedText?.rest ?? ""}`;
  useEffect(() => {
    setIndex(0);
    setDismissed(false);
    setError(null);
  }, [queryKey]);

  // `/forget` lists facts: fetch them once when its argument phase opens.
  const needsFacts = phase === "args" && argView?.args.source === "facts";
  useEffect(() => {
    if (!needsFacts || !api.inTauri()) return;
    let live = true;
    api.listMemoryFacts().then((f) => live && setFacts(f)).catch(() => {});
    return () => {
      live = false;
    };
  }, [needsFacts]);

  // ---- the lines ----
  const model = useMemo<CommandMenuModel>(() => {
    const listId = "composer-command-menu";
    if (phase === null || dismissed) {
      return closed("name", announcement, listId);
    }
    const lines: MenuLine[] = [];
    let count = 0;

    if (phase === "args" && argView) {
      const typed = parsedText?.rest ?? "";
      const a = argView.args;
      if (argView.source !== "builtin") {
        // A skill, or a connector's prompt, takes free text; there is nothing to complete.
        return closed(phase, announcement, listId);
      }
      if (a.type === "choice" && a.source) {
        const choices = argChoices(a.source, typed, useAppStore.getState(), { facts });
        for (const choice of choices) {
          lines.push({ kind: "choice", key: `choice:${choice.value}`, choice, index: count++ });
        }
        if (!choices.length) {
          lines.push({
            kind: "note",
            key: "note:none",
            text: typed ? `Nothing matches "${typed}"` : "Nothing to choose from",
          });
        }
      } else if (a.type === "none") {
        lines.push({ kind: "note", key: "note:run", text: `/${argView.name}  ·  ↵ run` });
      } else {
        const hint = a.hint ?? (a.type === "number" ? "a number" : "text");
        lines.push({
          kind: "note",
          key: "note:arg",
          text:
            a.required && !typed.trim()
              ? `/${argView.name} ⟨${hint}⟩  ·  I need this to go on`
              : `/${argView.name} ⟨${hint}⟩  ·  ↵ run`,
        });
      }
      return { open: true, phase, lines, count, index, popover: false, filter: "", announcement, listId };
    }

    // The name phase.
    const visible = visibleViews(views);
    const q = query.trim();
    if (q) {
      for (const v of rankCommands(visible, q)) {
        lines.push({
          kind: "command",
          key: `cmd:${v.source}:${v.name}`,
          row: { key: v.name, view: v },
          index: count++,
          sectionHint: SECTION_LABEL[v.section],
        });
      }
    } else {
      const recent = readRecent()
        .map((n) => visible.find((v) => v.name === n))
        .filter((v): v is CommandView => !!v);
      if (recent.length) {
        lines.push({ kind: "header", key: "h:recent", label: "Recent" });
        for (const v of recent) {
          lines.push({ kind: "command", key: `recent:${v.name}`, row: { key: v.name, view: v }, index: count++ });
        }
      }
      for (const section of SECTION_ORDER as CommandSection[]) {
        const inSection = visible.filter((v) => v.section === section);
        if (!inSection.length) continue;
        lines.push({ kind: "header", key: `h:${section}`, label: SECTION_LABEL[section] });
        for (const v of inSection) {
          lines.push({ kind: "command", key: `cmd:${v.source}:${v.name}`, row: { key: v.name, view: v }, index: count++ });
        }
      }
    }
    return { open: count > 0, phase: "name", lines, count, index, popover, filter, announcement, listId };
  }, [phase, dismissed, argView, parsedText, query, views, facts, index, popover, filter, announcement]);

  // ---- running ----
  // Plain functions, not memoised: they close over the composer's latest
  // attachments and send handler, and a stale copy would send the wrong thing.
  async function finish(cmd: NonNullable<ReturnType<typeof parseCommandLine>>) {
    const result = await runCommand(cmd, useAppStore.getState());
    await handleResult(cmd, result);
  }

  async function handleResult(cmd: NonNullable<ReturnType<typeof parseCommandLine>>, result: CommandResult) {
    const state = useAppStore.getState();
    switch (result.kind) {
      case "done":
        setValue("");
        pushRecent(cmd.name);
        setPopover(false);
        if (cmd.spec.trace) {
          state.recordCommand(state.activeConversationId, {
            name: cmd.name,
            args: cmd.args,
            by: "user",
            outcome: "done",
            note: result.note,
          });
        }
        break;
      case "modifier":
        pushRecent(cmd.name);
        setPopover(false);
        setValue("");
        focus();
        // `/effort high add tests`: the chip is set, and the words go out now.
        if (result.send) sendText(result.send);
        break;
      case "send":
        pushRecent(cmd.name);
        setPopover(false);
        setValue("");
        // A connector's prompt is plain text; only a skill carries a name to preload.
        if (result.skill) sendSkill(result.text, result.skill, result.skillArgs);
        else sendText(result.text);
        break;
      case "needsArg":
        setValue(`/${cmd.name} `);
        setPopover(false);
        setError(result.message ?? null);
        focus();
        break;
      case "confirm":
        setConfirm({
          title: result.title,
          body: result.body,
          confirmLabel: result.confirmLabel,
          run: async () => {
            const outcome = await result.run();
            if (outcome) await handleResult(cmd, outcome);
            else await handleResult(cmd, { kind: "done" });
          },
        });
        break;
      case "help":
        setValue("/");
        setPopover(false);
        setDismissed(false);
        focus();
        break;
      case "error":
        setError(result.message);
        break;
    }
  }

  /** Try to run the text as a command. `false` means it is not one, and the
   * caller carries on with the ordinary send. */
  function tryRun(text: string): boolean {
    const state = useAppStore.getState();
    const parsed = parseCommandLine(text, commandViews(state));
    if (!parsed) return false;
    // A user's own skill typed while a run is live is just something to say
    // to it; only the built-ins act on the run.
    if (busy && parsed.spec.source !== "builtin") return false;
    if (parsed.spec.disabledReason) {
      setError(parsed.spec.disabledReason);
      setAnnouncement(parsed.spec.disabledReason);
      return true;
    }
    void finish(parsed);
    return true;
  }

  // ---- keys ----
  function complete(view: CommandView) {
    setValue(`/${view.name} `);
    setPopover(false);
    focus();
  }

  function activate(line: MenuLine | undefined, fromTab: boolean) {
    if (!line) return;
    if (line.kind === "command") {
      const v = line.row.view;
      if (v.disabledReason) {
        setAnnouncement(v.disabledReason);
        return;
      }
      const takesWords = v.args.type !== "none";
      // A skill, a required argument, or Tab all mean "let me type the rest".
      if (fromTab || v.source !== "builtin" || (takesWords && v.args.required)) {
        complete(v);
        return;
      }
      setValue(`/${v.name}`);
      tryRun(`/${v.name}`);
      return;
    }
    if (line.kind === "choice" && argView) {
      const text = `/${argView.name} ${line.choice.value}`;
      if (fromTab) {
        setValue(text);
        return;
      }
      setValue(text);
      tryRun(text);
    }
  }

  /** Returns true when the key was the menu's. */
  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>): boolean {
    if (!model.open) return false;
    const selectable = model.lines.filter((l) => l.kind === "command" || l.kind === "choice");
    if (e.key === "ArrowDown" && model.count) {
      e.preventDefault();
      setIndex((i) => (i + 1) % model.count);
      return true;
    }
    if (e.key === "ArrowUp" && model.count) {
      e.preventDefault();
      setIndex((i) => (i - 1 + model.count) % model.count);
      return true;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      setDismissed(true);
      setPopover(false);
      return true;
    }
    if (e.key === "Enter" || e.key === "Tab") {
      if (e.key === "Tab" && e.shiftKey) return false;
      const picked = selectable[model.index];
      if (model.phase === "args" && !picked) {
        // The free-text rows: Enter runs what was typed, Tab has nothing to add.
        if (e.key === "Tab") {
          e.preventDefault();
          return true;
        }
        e.preventDefault();
        const spec = argView;
        const typed = parsedText?.rest ?? "";
        if (spec && spec.args.required && !typed.trim()) {
          setAnnouncement(`I need ${spec.args.hint ?? "something"} for /${spec.name}`);
          return true;
        }
        tryRun(value);
        return true;
      }
      if (!picked) return false;
      e.preventDefault();
      activate(picked, e.key === "Tab");
      return true;
    }
    return false;
  }

  /** The `/` button (`CMP-4`): with nothing typed it types the slash; with text
   * it opens a popover that prepends the choice. */
  function openFromButton() {
    if (!value.trim()) {
      setValue("/");
      setDismissed(false);
      focus();
      return;
    }
    setFilter("");
    setPopover((p) => !p);
  }

  /** Popover choice: put the command in front of what is already typed. */
  function chooseFromPopover(view: CommandView) {
    if (view.disabledReason) {
      setAnnouncement(view.disabledReason);
      return;
    }
    setValue(`/${view.name} ${value}`);
    setPopover(false);
    focus();
  }

  return {
    model,
    views,
    error,
    clearError: () => setError(null),
    confirm,
    clearConfirm: () => setConfirm(null),
    tryRun,
    onKeyDown,
    setIndex,
    activateAt: (i: number) => {
      const selectable = model.lines.filter((l) => l.kind === "command" || l.kind === "choice");
      activate(selectable[i], false);
    },
    openFromButton,
    closePopover: () => setPopover(false),
    setFilter,
    chooseFromPopover,
    /** Escape in the box: dismiss the menu, keep the text. */
    dismiss: () => setDismissed(true),
    /** `aria-activedescendant` for the input. */
    activeId: model.open && model.count ? `${model.listId}-opt-${model.index}` : undefined,
  };
}
