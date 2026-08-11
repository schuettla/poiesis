import { useAppStore } from "../lib/store";
import type { View } from "../lib/types";
import PoiesisMark from "../components/Mark/PoiesisMark";
import Models from "./Models";
import Engine from "./Engine";
import Apps from "./Apps";
import Skills from "./Skills";
import Self from "./Self";
import Tasks from "./Tasks";
import Activity from "./Activity";
import Settings from "./Settings";
import "./SettingsHub.css";

const TABS: { view: View; label: string; icon: string }[] = [
  { view: "settings", label: "General", icon: "⚙" },
  { view: "models", label: "Models", icon: "▤" },
  { view: "engine", label: "Engine", icon: "◧" },
  { view: "apps", label: "Apps", icon: "◇" },
  { view: "skills", label: "Skills", icon: "▦" },
  { view: "self", label: "Self", icon: "" },
  { view: "tasks", label: "Tasks", icon: "◷" },
  { view: "activity", label: "Activity", icon: "≡" },
];

/** The settings hub: everything that used to be its own rail entry (Models,
 * Engine, Apps, Self, Settings) now lives behind the header cog, with its own
 * secondary navigation so the main rail stays about conversations. */
export default function SettingsHub() {
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const soulPending = useAppStore((s) => s.changeProposals.some((p) => p.target === "soul"));
  // Every proposal that lands in the skills folder badges the Skills tab —
  // that's where accepting it writes. `recipe` is one left unanswered from
  // before skills existed (`SKL-5`); `skill-revision` is a rough skill asking
  // to revise itself (`OUT-2`). Without them here they'd fall through to the
  // Self badge below, which points at the wrong tab.
  const isSkill = (target: string) =>
    target === "skill" || target === "skill-revision" || target === "recipe";
  const selfPending = useAppStore((s) =>
    s.changeProposals.some((p) => p.target !== "soul" && !isSkill(p.target))
  );
  const skillPending = useAppStore((s) => s.changeProposals.some((p) => isSkill(p.target)));
  const consolidationPending = useAppStore((s) => s.consolidationPending);
  const badgeFor = (v: View) =>
    (v === "settings" && soulPending) ||
    (v === "self" && (selfPending || consolidationPending)) ||
    (v === "skills" && skillPending);

  return (
    <div className="settings-hub">
      <nav className="settings-hub-nav" aria-label="Settings sections">
        {TABS.map((t) => (
          <button
            key={t.view}
            className={`settings-hub-tab ${view === t.view ? "active" : ""}`}
            onClick={() => setView(t.view)}
          >
            <span className="sht-icon" aria-hidden="true">
              {t.view === "self" ? <PoiesisMark size={15} /> : t.icon}
            </span>
            <span className="sht-label">{t.label}</span>
            {badgeFor(t.view) && (
              <span
                className="sht-badge"
                role="img"
                aria-label="Changes waiting for review"
                title="Changes waiting for review"
              />
            )}
          </button>
        ))}
      </nav>
      <div className="settings-hub-content">
        {view === "settings" && <Settings />}
        {view === "models" && <Models />}
        {view === "engine" && <Engine />}
        {view === "apps" && <Apps />}
        {view === "skills" && <Skills />}
        {view === "self" && <Self />}
        {view === "tasks" && <Tasks />}
        {view === "activity" && <Activity />}
      </div>
    </div>
  );
}
