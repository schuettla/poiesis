import { useEffect, useState } from "react";
import PersonaEditor from "../components/Personas/PersonaEditor";
import TeamEditor from "../components/Personas/TeamEditor";
import { useAppStore } from "../lib/store";

type Tab = "personas" | "teams";

/**
 * `TEAM-4`: Settings → Personas & teams. Who Poiesis can speak as, and what each of them
 * is; `TEAM-9`: and the teams they work in, the second tab.
 *
 * Both tabs stay mounted, so a team being edited is still there after a look at a persona
 * (adding a member can start a new persona). A link that names a team (`team:<slug>`) lands on
 * the Teams tab, any other on the Personas tab.
 *
 * The Greek line is muted and can be folded away with the other first-time explanations
 * ("Show first-time explanations again" brings it back).
 */
export default function Personas() {
  const folded = useAppStore((s) => s.firstTimeFlags.koryphaios === true);
  const dismiss = useAppStore((s) => s.dismissExplanation);
  const focus = useAppStore((s) => s.personasFocus);
  const [tab, setTab] = useState<Tab>(focus?.startsWith("team:") ? "teams" : "personas");
  useEffect(() => {
    if (focus) setTab(focus.startsWith("team:") ? "teams" : "personas");
  }, [focus]);

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Personas &amp; teams</h1>
        <p className="lede">
          A persona is someone Poiesis speaks as: it can lead a chat or work in a team. A team is several
          personas with lines that say who works with whom.
        </p>
        {!folded && (
          <p className="setting-help">
            In Greek theater the chorus spoke through masks, and one of them, the koryphaios, spoke for all.{" "}
            <button className="link-btn" onClick={() => dismiss("koryphaios")}>
              Hide
            </button>
          </p>
        )}
        <div className="self-toolbar">
          <div className="model-tabs" role="tablist" aria-label="Personas and teams">
            {(["personas", "teams"] as const).map((t) => (
              <button key={t} role="tab" aria-selected={tab === t} className={`model-tab ${tab === t ? "on" : ""}`} onClick={() => setTab(t)}>
                {t === "personas" ? "Personas" : "Teams"}
              </button>
            ))}
          </div>
        </div>
        <div role="tabpanel" hidden={tab !== "personas"}>
          <PersonaEditor onOpenTeam={(slug) => useAppStore.setState({ personasFocus: `team:${slug}` })} />
        </div>
        <div role="tabpanel" hidden={tab !== "teams"}>
          <TeamEditor />
        </div>
      </div>
    </div>
  );
}
