import { useAppStore } from "../lib/store";
import { HUB_GROUPS, HUB_SECTIONS } from "../lib/types";
import type { View } from "../lib/types";
import PoiesisMark from "../components/Mark/PoiesisMark";
import { SectionIcon } from "../components/Icons/Icons";
import FirstSteps from "./FirstSteps";
import Models from "./Models";
import Providers from "./Providers";
import Engines from "./Engines";
import Runtime from "./Runtime";
import Apps from "./Apps";
import Skills from "./Skills";
import Self from "./Self";
import Personas from "./Personas";
import Tasks from "./Tasks";
import Activity from "./Activity";
import Settings from "./Settings";
import WorkingDir from "./WorkingDir";
import Mail from "./Mail";
import Remote from "./Remote";
import Tools from "./Tools";
import Usage from "./Usage";
import Experiments from "./Experiments";
import About from "./About";
import { updateIsWaiting } from "../lib/updates";
import { hasNewExperiment } from "../lib/experiments";
import { TEAM_TARGET } from "../lib/teamProposals";
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
  // `RTE-6`: a suggested change to an agent app's rules badges External agents,
  // where accepting it writes. It is not a change to me, so Self stays quiet.
  // `TEAM-23`: a change Poiesis would like to make to a team badges Personas & teams, where it is
  // answered and where it writes, not Self.
  const selfPending = useAppStore((s) =>
    s.changeProposals.some((p) => p.target !== "soul" && p.target !== "engine-rule" && p.target !== TEAM_TARGET && !isSkill(p.target))
  );
  const enginePending = useAppStore((s) => s.changeProposals.some((p) => p.target === "engine-rule"));
  const teamPending = useAppStore((s) => s.changeProposals.some((p) => p.target === TEAM_TARGET));
  const skillPending = useAppStore((s) => s.changeProposals.some((p) => isSkill(p.target)));
  const consolidationPending = useAppStore((s) => s.consolidationPending);
  // `UPD-UI-3`: a newer version of me is waiting (or already staged).
  const updateWaiting = useAppStore((s) => updateIsWaiting(s.updateState));
  // `CA-UI-3`: a connected app's sign-in has expired.
  const appsNeedAttention = useAppStore((s) => s.appsNeedAttention);
  // `CLI-2`: an agent app was found that I haven't shown yet.
  const enginesNewlyFound = useAppStore((s) => s.enginesNewlyFound);
  // `EXP-UI-1`: an experiment appeared that the screen has not shown yet.
  const experimentsNew = useAppStore((s) => hasNewExperiment(s));
  const badgeFor = (v: View) =>
    (v === "settings" && soulPending) ||
    (v === "self" && (selfPending || consolidationPending)) ||
    (v === "skills" && skillPending) ||
    (v === "personas" && teamPending) ||
    (v === "apps" && appsNeedAttention) ||
    (v === "engines" && (enginesNewlyFound || enginePending)) ||
    (v === "experiments" && experimentsNew) ||
    (v === "about" && updateWaiting);
  const badgeText = (v: View) =>
    v === "about"
      ? "An update is waiting"
      : v === "experiments"
        ? "A new experiment"
      : v === "personas"
        ? "A change to a team is waiting for you"
      : v === "apps"
        ? "A sign-in has expired"
        : v === "engines"
          ? enginePending
            ? "A change to the rules is waiting for you"
            : "I found an agent app"
          : "Changes waiting for review";

  const firstSteps = HUB_TABS.find((t) => t.view === "firststeps");
  const about = HUB_TABS.find((t) => t.view === "about");
  const tab = (t: (typeof HUB_TABS)[number]) => (
    <button
      key={t.view}
      className={`settings-hub-tab ${view === t.view ? "active" : ""}`}
      onClick={() => setView(t.view)}
    >
      <span className="sht-icon" aria-hidden="true">
        {t.view === "self" ? <PoiesisMark size={15} /> : <SectionIcon view={t.view} size={16} />}
      </span>
      <span className="sht-label">{t.label}</span>
      {badgeFor(t.view) && (
        <span
          className="sht-badge"
          role="img"
          aria-label={badgeText(t.view)}
          title={badgeText(t.view)}
        />
      )}
    </button>
  );

  // `SHL-16` is withdrawn: the hub owns its own section navigation, always,
  // whatever the Rail is doing. Moving it into the Rail made the sections read
  // as another top-level place to be rather than as the inside of Settings,
  // and it took the conversation list away to do it. This small inline column
  // is the settings navigation.
  return (
    <div className="settings-hub">
      <nav className="settings-hub-nav" aria-label="Settings sections">
        {HUB_GROUPS.map((g) => (
          <div
            className="settings-hub-group"
            role="group"
            aria-labelledby={g.label ? `shg-${g.id}` : undefined}
            key={g.id}
          >
            {g.label && (
              <div className="settings-hub-group-label" id={`shg-${g.id}`}>
                {g.label}
              </div>
            )}
            {HUB_TABS.filter((t) => t.group === g.id).map(tab)}
          </div>
        ))}
        {about && <div className="settings-hub-group">{tab(about)}</div>}
        {firstSteps && (
          <div className="settings-hub-first">
            {tab(firstSteps)}
          </div>
        )}
      </nav>
      <div className="settings-hub-content">
        {view === "firststeps" && <FirstSteps />}
        {view === "settings" && <Settings />}
        {view === "workingdir" && <WorkingDir />}
        {view === "mail" && <Mail />}
        {view === "remote" && <Remote />}
        {view === "tools" && <Tools />}
        {view === "models" && <Models />}
        {view === "providers" && <Providers />}
        {view === "engines" && <Engines />}
        {view === "runtime" && <Runtime />}
        {view === "apps" && <Apps />}
        {view === "skills" && <Skills />}
        {view === "self" && <Self />}
        {view === "personas" && <Personas />}
        {view === "tasks" && <Tasks />}
        {view === "activity" && <Activity />}
        {view === "usage" && <Usage />}
        {view === "experiments" && <Experiments />}
        {view === "about" && <About />}
      </div>
    </div>
  );
}
