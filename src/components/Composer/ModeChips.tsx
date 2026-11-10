import { useAppStore, useSelectedModel } from "../../lib/store";
import { tierOf, tierTag } from "../../lib/tier";
import { usePersonaAppsLine } from "../Apps/personaAppsLine";
import { CloseIcon } from "../Icons/Icons";
import "./ModeChips.css";

/**
 * `CMP-7`: state that outlasts one message — Workspace mode, tools off, a
 * persona. These used to hang off the `+`, where "on" was only a lit button.
 * Naming them beside the footer makes the current state readable, and each one
 * can be turned off right where it is read.
 *
 * Clicking the label opens the command that changes it, with its argument ready.
 * Workspace is the exception: its label switches between the workspace and the
 * message stream, because that is the thing you reach for, and the x leaves the mode.
 */
export interface ModeChip {
  key: "workspace" | "tools" | "tier" | "external" | "persona";
  label: string;
  command: string;
  off: () => void;
  offLabel: string;
  /** Said in the tooltip before "Change this". */
  hint?: string | null;
  /** Replaces "open the command" as what the label does. */
  open?: { run: () => void; title: string };
}

/** The chat's standing state, one entry per thing that is not at its default.
 * `ModeChips` draws some of them as chips beside the footer; the composer's
 * "this chat" menu (`ChatSetup`) lists the others. */
export function useModeChips(): ModeChip[] {
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const workspaceShown = useAppStore((s) => s.workspaceShown);
  const setWorkspaceShown = useAppStore((s) => s.setWorkspaceShown);
  const toolsEnabled = useAppStore((s) => s.toolsEnabled);
  const setWorkspaceMode = useAppStore((s) => s.setWorkspaceMode);
  const setToolsEnabled = useAppStore((s) => s.setToolsEnabled);
  const applyPersona = useAppStore((s) => s.applyPersona);
  const convId = useAppStore((s) => s.activeConversationId);
  // `CLI-UI-6`: only when this chat has turned agent apps off, and only if there
  // is an agent app that could have been used.
  const enginesOff = useAppStore(
    (s) =>
      !!s.conversations.find((c) => c.id === s.activeConversationId)?.enginesOff &&
      s.engines.some((e) => e.enabled && e.delegate_ready)
  );
  const setEnginesOff = useAppStore((s) => s.setEnginesOff);
  const showModel = useAppStore((s) => s.showModel);
  const setModelTier = useAppStore((s) => s.setModelTier);
  const model = useSelectedModel();
  const runTier = useAppStore((s) => (s.activeRun?.convId === s.activeConversationId ? s.activeRun?.tier : undefined));
  const persona = useAppStore((s) => {
    const id = s.conversations.find((c) => c.id === s.activeConversationId)?.personaId;
    return id ? (s.personas.find((p) => p.id === id) ?? null) : null;
  });
  // `CA-UI-6`: a persona narrowed to some apps says which.
  const appsLine = usePersonaAppsLine(persona?.id);

  const chips: ModeChip[] = [];
  if (workspaceMode) {
    chips.push({
      key: "workspace",
      label: "Workspace",
      command: "/workspace ",
      off: () => setWorkspaceMode(false),
      offLabel: "Turn Workspace mode off",
      open: {
        run: () => setWorkspaceShown(!workspaceShown),
        title: workspaceShown ? "Show the message stream" : "Open the workspace",
      },
    });
  }
  if (!toolsEnabled) {
    chips.push({
      key: "tools",
      label: "Tools off",
      command: "/tools ",
      off: () => setToolsEnabled(true),
      offLabel: "Let me use my tools again",
    });
  }
  // `TIER-UI-2`: only when the model's tier takes away something the user has on.
  // If they already turned tools off, "Tools off" above says it. While a run is
  // live it says the tier that run really has. The x and the chip that follows
  // it undo each other, so the choice can always be taken back where it was made.
  const shown = runTier ? { ...model, tier: runTier } : model;
  const tag = tierTag(shown);
  if (toolsEnabled && tag) {
    chips.push({
      key: "tier",
      label: tag.chip,
      command: "",
      off: () => void setModelTier(model, "full"),
      offLabel: "Use all features with this model",
      open: { run: () => showModel(model.id), title: `${tag.title} · Change this` },
    });
  } else if (toolsEnabled && tierOf(shown) === "full" && model.tierSet === "full" && model.autoTier !== "full") {
    chips.push({
      key: "tier",
      label: "All features",
      command: "",
      off: () => void setModelTier(model, null),
      offLabel: "Go back to fewer features for this small model",
      open: { run: () => showModel(model.id), title: "You gave this small model every feature. · Change this" },
    });
  }
  if (enginesOff && convId) {
    chips.push({
      key: "external",
      label: "External agents off",
      command: "/external ",
      off: () => void setEnginesOff(convId, false),
      offLabel: "Let me hand work to external agents again",
    });
  }
  if (persona && convId) {
    chips.push({
      key: "persona",
      label: persona.name,
      command: "/persona ",
      off: () => void applyPersona(convId, null),
      offLabel: `Remove the ${persona.name} persona`,
      hint: appsLine,
    });
  }
  return chips;
}

/** What a chip's label does when clicked, and what it says on hover. */
export function chipTitle(c: ModeChip): string {
  return c.open?.title ?? (c.hint ? `${c.hint} · Change this` : "Change this");
}

/**
 * The chips themselves. `only` keeps the ones named: under the composer that is
 * the states that take something away from me (tools, tier, external agents),
 * because those change what I can do and should be seen without opening anything.
 */
export default function ModeChips({ only }: { only?: ModeChip["key"][] }) {
  const all = useModeChips();
  const requestComposer = useAppStore((s) => s.requestComposer);
  const chips = only ? all.filter((c) => only.includes(c.key)) : all;
  if (!chips.length) return null;

  return (
    <div className="mode-chips" role="group" aria-label="How I'm set up in this chat">
      {chips.map((c) => (
        <span className="mode-chip" key={c.key}>
          <button
            className="mode-chip-label"
            title={chipTitle(c)}
            onClick={() => (c.open ? c.open.run() : requestComposer(c.command))}
          >
            {c.label}
          </button>
          <button className="mode-chip-x" aria-label={c.offLabel} title={c.offLabel} onClick={c.off}>
            <CloseIcon size={10} strokeWidth={1.6} />
          </button>
        </span>
      ))}
    </div>
  );
}
