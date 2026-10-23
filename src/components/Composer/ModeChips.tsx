import { useAppStore } from "../../lib/store";
import { usePersonaAppsLine } from "../Apps/personaAppsLine";
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
export default function ModeChips() {
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const workspaceShown = useAppStore((s) => s.workspaceShown);
  const setWorkspaceShown = useAppStore((s) => s.setWorkspaceShown);
  const toolsEnabled = useAppStore((s) => s.toolsEnabled);
  const setWorkspaceMode = useAppStore((s) => s.setWorkspaceMode);
  const setToolsEnabled = useAppStore((s) => s.setToolsEnabled);
  const applyPersona = useAppStore((s) => s.applyPersona);
  const requestComposer = useAppStore((s) => s.requestComposer);
  const stopGoal = useAppStore((s) => s.stopGoal);
  const goal = useAppStore((s) => (s.activeConversationId ? s.goals[s.activeConversationId] : undefined));
  const convId = useAppStore((s) => s.activeConversationId);
  // `CLI-UI-6`: only when this chat has turned agent apps off, and only if there
  // is an agent app that could have been used.
  const enginesOff = useAppStore(
    (s) =>
      !!s.conversations.find((c) => c.id === s.activeConversationId)?.enginesOff &&
      s.engines.some((e) => e.enabled && e.delegate_ready)
  );
  const setEnginesOff = useAppStore((s) => s.setEnginesOff);
  const persona = useAppStore((s) => {
    const id = s.conversations.find((c) => c.id === s.activeConversationId)?.personaId;
    return id ? (s.personas.find((p) => p.id === id) ?? null) : null;
  });
  // `CA-UI-6`: a persona narrowed to some apps says which.
  const appsLine = usePersonaAppsLine(persona?.id);

  const chips: {
    key: string;
    label: string;
    command: string;
    off: () => void;
    offLabel: string;
    /** Said in the tooltip before "Change this". */
    hint?: string | null;
    /** Replaces "open the command" as what the label does. */
    open?: { run: () => void; title: string };
  }[] = [];
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
  if (enginesOff && convId) {
    chips.push({
      key: "external",
      label: "External agents off",
      command: "/external ",
      off: () => void setEnginesOff(convId, false),
      offLabel: "Let me hand work to external agents again",
    });
  }
  // `GOL-UI-1`: while a goal is running it is a mode of the chat like the others.
  if (goal?.status === "active" && convId) {
    const said = goal.text.length > 24 ? `${goal.text.slice(0, 23).trimEnd()}…` : goal.text;
    chips.push({
      key: "goal",
      label: `Goal: ${said}`,
      command: "/goal stop",
      off: () => stopGoal(convId),
      offLabel: "Stop working toward this goal",
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
  if (!chips.length) return null;

  return (
    <div className="mode-chips" role="group" aria-label="How I'm set up in this chat">
      {chips.map((c) => (
        <span className="mode-chip" key={c.key}>
          <button
            className="mode-chip-label"
            title={c.open?.title ?? (c.hint ? `${c.hint} · Change this` : "Change this")}
            onClick={() => (c.open ? c.open.run() : requestComposer(c.command))}
          >
            {c.label}
          </button>
          <button className="mode-chip-x" aria-label={c.offLabel} title={c.offLabel} onClick={c.off}>
            ×
          </button>
        </span>
      ))}
    </div>
  );
}
