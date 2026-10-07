import { useAppStore } from "../../lib/store";
import "./ModeChips.css";

/**
 * `CMP-7`: state that outlasts one message — Workspace mode, tools off, a
 * persona. These used to hang off the `+`, where "on" was only a lit button.
 * Naming them beside the footer makes the current state readable, and each one
 * can be turned off right where it is read.
 *
 * Clicking the label opens the command that changes it, with its argument ready.
 */
export default function ModeChips() {
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const toolsEnabled = useAppStore((s) => s.toolsEnabled);
  const setWorkspaceMode = useAppStore((s) => s.setWorkspaceMode);
  const setToolsEnabled = useAppStore((s) => s.setToolsEnabled);
  const applyPersona = useAppStore((s) => s.applyPersona);
  const requestComposer = useAppStore((s) => s.requestComposer);
  const stopGoal = useAppStore((s) => s.stopGoal);
  const goal = useAppStore((s) => (s.activeConversationId ? s.goals[s.activeConversationId] : undefined));
  const convId = useAppStore((s) => s.activeConversationId);
  const persona = useAppStore((s) => {
    const id = s.conversations.find((c) => c.id === s.activeConversationId)?.personaId;
    return id ? (s.personas.find((p) => p.id === id) ?? null) : null;
  });

  const chips: { key: string; label: string; command: string; off: () => void; offLabel: string }[] = [];
  if (workspaceMode) {
    chips.push({
      key: "workspace",
      label: "Workspace",
      command: "/workspace ",
      off: () => setWorkspaceMode(false),
      offLabel: "Turn Workspace mode off",
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
    });
  }
  if (!chips.length) return null;

  return (
    <div className="mode-chips" role="group" aria-label="How I'm set up in this chat">
      {chips.map((c) => (
        <span className="mode-chip" key={c.key}>
          <button
            className="mode-chip-label"
            title="Change this"
            onClick={() => requestComposer(c.command)}
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
