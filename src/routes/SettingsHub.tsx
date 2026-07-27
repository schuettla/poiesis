import { useAppStore } from "../lib/store";
import type { View } from "../lib/types";
import PoiesisMark from "../components/Mark/PoiesisMark";
import Models from "./Models";
import Engine from "./Engine";
import Apps from "./Apps";
import Self from "./Self";
import Settings from "./Settings";
import "./SettingsHub.css";

const TABS: { view: View; label: string; icon: string }[] = [
  { view: "settings", label: "General", icon: "⚙" },
  { view: "models", label: "Models", icon: "▤" },
  { view: "engine", label: "Engine", icon: "◧" },
  { view: "apps", label: "Apps", icon: "◇" },
  { view: "self", label: "Self", icon: "" },
];

/** The settings hub: everything that used to be its own rail entry (Models,
 * Engine, Apps, Self, Settings) now lives behind the header cog, with its own
 * secondary navigation so the main rail stays about conversations. */
export default function SettingsHub() {
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const soulPending = useAppStore((s) => s.changeProposals.some((p) => p.target === "soul"));
  const selfPending = useAppStore((s) => s.changeProposals.some((p) => p.target !== "soul"));
  const consolidationPending = useAppStore((s) => s.consolidationPending);
  const badgeFor = (v: View) =>
    (v === "settings" && soulPending) || (v === "self" && (selfPending || consolidationPending));

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
        {view === "self" && <Self />}
      </div>
    </div>
  );
}
