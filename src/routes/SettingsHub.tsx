import { useAppStore } from "../lib/store";
import { HUB_SECTIONS } from "../lib/types";
import type { View } from "../lib/types";
import PoiesisMark from "../components/Mark/PoiesisMark";
import Models from "./Models";
import Providers from "./Providers";
import Runtime from "./Runtime";
import Apps from "./Apps";
import Skills from "./Skills";
import Self from "./Self";
import Tasks from "./Tasks";
import Activity from "./Activity";
import Settings from "./Settings";
import WorkingDir from "./WorkingDir";
import Mail from "./Mail";
import Tools from "./Tools";
import Usage from "./Usage";
import About from "./About";
import "./SettingsHub.css";

/** The hub's own sections, and whether a view is one of them. Both now live in
 * `types.ts`, because the store needs them to collapse the whole hub onto a
 * single route tab and importing this module from the store would drag every
 * settings panel into its module graph. Re-exported under the names the rest
 * of the app already uses. */
export const HUB_TABS = HUB_SECTIONS;
export { isHubView } from "../lib/types";

/** The settings hub: everything that used to be its own rail entry (Models,
 * Runtime, Apps, Self, Settings) now lives behind the header cog, with its own
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

  // `SHL-16` is withdrawn: the hub owns its own section navigation, always,
  // whatever the Rail is doing. Moving it into the Rail made the sections read
  // as another top-level place to be rather than as the inside of Settings,
  // and it took the conversation list away to do it. This small inline column
  // is the settings navigation.
  return (
    <div className="settings-hub">
      <nav className="settings-hub-nav" aria-label="Settings sections">
        {HUB_TABS.map((t) => (
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
        {view === "workingdir" && <WorkingDir />}
        {view === "mail" && <Mail />}
        {view === "tools" && <Tools />}
        {view === "models" && <Models />}
        {view === "providers" && <Providers />}
        {view === "runtime" && <Runtime />}
        {view === "apps" && <Apps />}
        {view === "skills" && <Skills />}
        {view === "self" && <Self />}
        {view === "tasks" && <Tasks />}
        {view === "activity" && <Activity />}
        {view === "usage" && <Usage />}
        {view === "about" && <About />}
      </div>
    </div>
  );
}
