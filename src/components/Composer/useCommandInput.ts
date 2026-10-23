import { useEffect, useMemo, useState } from "react";
import { useAppStore } from "../../lib/store";
import * as api from "../../lib/api";
import { useEffort } from "./EffortPicker";
import {
  SECTION_LABEL,
  SECTION_ORDER,
  applyControl,
  argChoices,
  commandViews,
  controlFor,
  parseCommandLine,
  rankCommands,
  runCommand,
  visibleViews,
  type ArgChoice,
  type CommandResult,
  type CommandSection,
  type CommandView,
  type LiveControl,
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
  /** A command, with the setting it controls on the row when it has one. */
  | {
      kind: "command";
      key: string;
      row: CommandRow;
      index: number;
      control: LiveControl | null;
      sectionHint?: string;
      /** Set while typing: the row also shows its command, with this marked. */
      query?: string;
    }
  /** Something I did lately, as a quick button on the first page. */
  | { kind: "recent"; key: string; row: CommandRow; index: number; control: LiveControl | null }
  /** A group of commands: choosing it opens them. */
  | { kind: "category"; key: string; section: CommandSection; label: string; hint: string; index: number }
  | { kind: "choice"; key: string; choice: ArgChoice; index: number; checked?: boolean }
  | { kind: "note"; key: string; text: string };

/** Where the first page's groups have been opened to. `pick` is a list of
 * choices for one setting, opened from inside its group. */
type Panel = { section: CommandSection } | { pick: string; from: CommandSection } | null;

type SelectableLine = Extract<MenuLine, { index: number }>;
const selectable = (lines: MenuLine[]) => lines.filter((l): l is SelectableLine => "index" in l);

/** What the menu should draw, and how it is navigated. */
export interface CommandMenuModel {
  open: boolean;
  phase: "name" | "args";
  lines: MenuLine[];
  /** Set inside a group: what the way back is headed with. */
  panel: { title: string } | null;
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
    panel: null,
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
  sendSkill: (text: string, skill: string, skillArgs?: string, delegateTo?: string) => void;
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
  const delegateTargets = useAppStore((s) => s.delegateTargets);
  const engines = useAppStore((s) => s.engines);
  const storeBusy = useAppStore((s) => s.busy);
  // What the rows with a switch or a choice on them show. Read when the lines
  // are built, so the menu has to rebuild when any of them moves.
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const toolsEnabled = useAppStore((s) => s.toolsEnabled);
  const turnModifiers = useAppStore((s) => s.turnModifiers);
  const selectedModelId = useAppStore((s) => s.selectedModelId);
  // The thinking level in force, the way the model dialog works it out.
  const effortNow = useEffort(turnModifiers.effort).current.value;

  // `commandViews` builds a fresh array, so it lives in a memo over the slices
  // it reads — not in a selector.
  const views = useMemo(
    () => commandViews(useAppStore.getState()),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [skills, conversations, activeConversationId, activeRun, changeSets, personas, models, modelPrefs, storeBusy, engines]
  );

  const [dismissed, setDismissed] = useState(false);
  const [index, setIndex] = useState(0);
  const [popover, setPopover] = useState(false);
  const [panel, setPanel] = useState<Panel>(null);
  // The `/` button asks for the dialog of groups; typing the slash yourself gets
  // the plain list, with completion, because you are already typing.
  const [fromButton, setFromButton] = useState(false);
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
    setPanel(null);
  }, [queryKey]);
  // A menu that closed starts from its first page next time.
  useEffect(() => {
    if (phase === null) setPanel(null);
  }, [phase]);
  // The dialog is for the bare slash; the moment anything else is typed it is a search.
  useEffect(() => {
    if (value !== "/") setFromButton(false);
  }, [value]);

  /** Only the empty query browses; anything typed searches every command. */
  const browsing = phase === "name" && !query.trim();

  function openPanel(next: Panel) {
    setPanel(next);
    setIndex(0);
  }

  /** One step back: from a list of choices to its group, from a group to the first page. */
  function goBack() {
    openPanel(panel && "pick" in panel ? { section: panel.from } : null);
  }

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

  // `/delegate` names agents: read what this chat can use when its argument opens.
  const needsTargets = phase === "args" && argView?.args.lead === "agents";
  useEffect(() => {
    if (needsTargets) void useAppStore.getState().loadDelegateTargets();
  }, [needsTargets, activeConversationId]);

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
      lines.push({ kind: "header", key: "h:arg", label: argView.title });
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
        lines.push({ kind: "note", key: "note:run", text: "Press Enter to do it" });
      } else if (a.lead && !/\s/.test(typed)) {
        // `/delegate [agent] task`: while the first word is still being typed it
        // may be an agent. Once there is a space after it, the rest is the task.
        const choices = argChoices(a.lead, typed, useAppStore.getState());
        for (const choice of choices) {
          lines.push({ kind: "choice", key: `choice:${choice.value}`, choice, index: count++ });
        }
        lines.push({
          kind: "note",
          key: "note:arg",
          text: choices.length ? "Pick who, or just type the task  ·  press Enter to go" : `${a.hint ?? "task"}  ·  press Enter to go`,
        });
      } else {
        const hint = a.hint ?? (a.type === "number" ? "a number" : "text");
        lines.push({
          kind: "note",
          key: "note:arg",
          text:
            a.required && !typed.trim()
              ? `I need this to go on: ${hint}`
              : `${hint}  ·  press Enter to go`,
        });
      }
      return { open: true, phase, lines, panel: null, count, index, popover: false, filter: "", announcement, listId };
    }

    // The name phase.
    const state = useAppStore.getState();
    const visible = visibleViews(views);
    const q = query.trim();
    const controlOf = (v: CommandView) =>
      v.source === "builtin" ? controlFor(v.name, state, { effort: effortNow }) : null;
    const commandLine = (v: CommandView, sectionHint?: string, query?: string): MenuLine => ({
      kind: "command",
      key: `cmd:${v.source}:${v.name}`,
      row: { key: v.name, view: v },
      index: count++,
      control: controlOf(v),
      sectionHint,
      query,
    });
    let heading: { title: string } | null = null;

    const pickView = panel && "pick" in panel ? visible.find((v) => v.name === panel.pick) : undefined;
    const pickControl = pickView ? controlOf(pickView) : null;
    const sectionOpen = panel && "section" in panel ? panel.section : null;

    if (q) {
      for (const v of rankCommands(visible, q)) lines.push(commandLine(v, SECTION_LABEL[v.section], q));
    } else if (pickView && pickControl?.kind === "pick") {
      heading = { title: pickView.title };
      for (const choice of pickControl.options) {
        lines.push({
          kind: "choice",
          key: `pick:${choice.value}`,
          choice,
          index: count++,
          checked: choice.value === pickControl.value,
        });
      }
    } else if (sectionOpen && visible.some((v) => v.section === sectionOpen)) {
      heading = { title: SECTION_LABEL[sectionOpen] };
      for (const v of visible.filter((x) => x.section === sectionOpen)) lines.push(commandLine(v));
    } else if (!popover && !fromButton) {
      // Typed slash: every command at once, under the group names, to type into.
      for (const section of SECTION_ORDER as CommandSection[]) {
        const inSection = visible.filter((v) => v.section === section);
        if (!inSection.length) continue;
        lines.push({ kind: "header", key: `h:${section}`, label: SECTION_LABEL[section] });
        for (const v of inSection) lines.push(commandLine(v, undefined, ""));
      }
    } else {
      for (const name of readRecent()) {
        const v = visible.find((x) => x.name === name);
        if (v) lines.push({ kind: "recent", key: `recent:${v.name}`, row: { key: v.name, view: v }, index: count++, control: controlOf(v) });
      }
      for (const section of SECTION_ORDER as CommandSection[]) {
        const inSection = visible.filter((v) => v.section === section);
        if (!inSection.length) continue;
        lines.push({
          kind: "category",
          key: `cat:${section}`,
          section,
          label: SECTION_LABEL[section],
          hint: inSection.map((v) => v.title).join(", "),
          index: count++,
        });
      }
    }
    return { open: count > 0, phase: "name", lines, panel: heading, count, index, popover, filter, announcement, listId };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phase, dismissed, argView, parsedText, query, views, facts, index, popover, filter, announcement, panel, fromButton, workspaceMode, toolsEnabled, turnModifiers, selectedModelId, effortNow, personas, models, conversations, delegateTargets]);

  // ---- completion while typing ----
  // The rest of the highlighted match, drawn faint after what has been typed:
  // the command's name first, then the choice for its argument. Tab or → takes it.
  const ghost = (() => {
    if (!model.open || model.popover || !parsedText) return "";
    const row = selectable(model.lines)[model.index];
    if (!row) return "";
    if (
      model.phase === "name" &&
      !parsedText.hasSpace &&
      parsedText.name &&
      row.kind === "command" &&
      !row.row.view.disabledReason
    ) {
      const name = row.row.view.name;
      return name.length > parsedText.name.length && name.toLowerCase().startsWith(parsedText.name)
        ? name.slice(parsedText.name.length)
        : "";
    }
    if (model.phase === "args" && row.kind === "choice") {
      const typed = parsedText.rest;
      const value = row.choice.value;
      return typed && value.length > typed.length && value.toLowerCase().startsWith(typed.toLowerCase())
        ? value.slice(typed.length)
        : "";
    }
    return "";
  })();

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
        if (result.skill) sendSkill(result.text, result.skill, result.skillArgs, result.delegateTo);
        else sendText(result.text);
        break;
      case "needsArg":
        setValue(`/${cmd.name} ${result.keep ? `${result.keep} ` : ""}`);
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

  /** Change a row's setting where it stands. The menu stays open: seeing the
   * switch move is the answer, and the next one is a click away. */
  async function changeControl(view: CommandView, value: string) {
    if (view.disabledReason) {
      setAnnouncement(view.disabledReason);
      return;
    }
    const state = useAppStore.getState();
    const result = await applyControl(view.name, value, state);
    if (result.kind === "error") {
      setError(result.message);
      setAnnouncement(result.message);
      return;
    }
    setError(null);
    pushRecent(view.name);
    if (result.kind === "done" && view.trace) {
      state.recordCommand(state.activeConversationId, {
        name: view.name,
        args: value,
        by: "user",
        outcome: "done",
        note: result.note,
      });
    }
  }

  /** A segment, a switch or a choice clicked on the row at `i`. */
  function controlAt(i: number, value?: string) {
    const line = selectable(model.lines)[i];
    if (!line || (line.kind !== "command" && line.kind !== "recent")) return;
    const control = line.control;
    if (control?.kind === "switch") void changeControl(line.row.view, value ?? (control.on ? "off" : "on"));
    else if (control && value !== undefined) void changeControl(line.row.view, value);
  }

  /** `typing`: Enter on a command being typed. A typed `/plan` runs, like it
   * always did; the same row clicked, or Enter on it while browsing, flips it. */
  function activate(line: MenuLine | undefined, fromTab: boolean, typing = false) {
    if (!line) return;
    if (line.kind === "category") {
      openPanel({ section: line.section });
      return;
    }
    if (line.kind === "command" || line.kind === "recent") {
      const v = line.row.view;
      if (v.disabledReason) {
        setAnnouncement(v.disabledReason);
        return;
      }
      if (line.control?.kind === "switch" && !fromTab && !typing) {
        void changeControl(v, line.control.on ? "off" : "on");
        return;
      }
      // A long list of choices opens in place; searching has no place for it,
      // so there it goes on to the argument like any other command.
      if (line.control?.kind === "pick" && browsing && !popover && !fromTab) {
        openPanel({ pick: v.name, from: v.section });
        return;
      }
      if (popover) {
        chooseFromPopover(v);
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
    if (line.kind === "choice" && panel && "pick" in panel) {
      const view = views.find((v) => v.name === panel.pick);
      if (view) void changeControl(view, line.choice.value);
      goBack();
      return;
    }
    if (line.kind === "choice" && argView?.args.lead) {
      // An agent for `/delegate`. One that cannot be used here opens its settings
      // instead of going into the box; any other goes in, and the task follows.
      const c = line.choice;
      if (c.disabledReason) {
        if (c.engineId) {
          useAppStore.getState().openEngines(c.engineId);
          setValue("");
        } else {
          setAnnouncement(c.disabledReason);
        }
        return;
      }
      setValue(`/${argView.name} ${c.value} `);
      focus();
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
    const rows = selectable(model.lines);
    const el = e.currentTarget;
    if (
      e.key === "ArrowRight" &&
      ghost &&
      el.selectionStart === value.length &&
      el.selectionEnd === value.length
    ) {
      // At the end of the text, → takes the faint completion, as Tab does.
      e.preventDefault();
      activate(rows[model.index], true);
      return true;
    }
    if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && browsing && !model.popover) {
      // Nothing is typed after the slash, so the arrows are free to move
      // through the menu: along a row's choices, into a group, back out of it.
      const row = rows[model.index];
      const control = row && (row.kind === "command" || row.kind === "recent") ? row.control : null;
      const dir = e.key === "ArrowLeft" ? -1 : 1;
      if (row && control?.kind === "segments" && (row.kind === "command" || row.kind === "recent")) {
        e.preventDefault();
        const at = control.options.findIndex((o) => o.value === control.value);
        const next = control.options[at < 0 ? (dir > 0 ? 0 : control.options.length - 1) : at + dir];
        if (next && next.value !== control.value) void changeControl(row.row.view, next.value);
        return true;
      }
      if (dir > 0 && row && (row.kind === "category" || control?.kind === "pick")) {
        e.preventDefault();
        activate(row, false);
        return true;
      }
      if (dir < 0 && panel) {
        e.preventDefault();
        goBack();
        return true;
      }
      return false;
    }
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
      // Inside a group, Escape steps out of it before it closes anything.
      if (panel && browsing) {
        goBack();
        return true;
      }
      setDismissed(true);
      setPopover(false);
      return true;
    }
    if (e.key === "Enter" || e.key === "Tab") {
      if (e.key === "Tab" && e.shiftKey) return false;
      const picked = rows[model.index];
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
      activate(picked, e.key === "Tab", !browsing);
      return true;
    }
    return false;
  }

  /** The `/` button (`CMP-4`): with nothing typed it types the slash; with text
   * it opens a popover that prepends the choice. */
  function openFromButton() {
    if (!value.trim() || value === "/") {
      setValue("/");
      setFromButton(true);
      setDismissed(false);
      focus();
      return;
    }
    setFilter("");
    setPanel(null);
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
    ghost,
    views,
    error,
    clearError: () => setError(null),
    confirm,
    clearConfirm: () => setConfirm(null),
    tryRun,
    onKeyDown,
    setIndex,
    activateAt: (i: number) => activate(selectable(model.lines)[i], false),
    controlAt,
    goBack,
    openFromButton,
    closePopover: () => setPopover(false),
    setFilter,
    /** Escape in the box: dismiss the menu, keep the text. */
    dismiss: () => setDismissed(true),
    /** `aria-activedescendant` for the input. */
    activeId: model.open && model.count ? `${model.listId}-opt-${model.index}` : undefined,
  };
}
