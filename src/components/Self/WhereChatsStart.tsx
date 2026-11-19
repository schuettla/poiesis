import { useState } from "react";
import * as api from "../../lib/api";
import { useAppStore } from "../../lib/store";
import Select from "../Select/Select";

type Act = (fn: () => Promise<unknown>, after?: () => void) => Promise<void>;

/**
 * `TEAM-5b`: the one place that answers "what happens when a new chat starts here". Each
 * row is a place (everywhere, a project, a kind of work, a persona's chats), who leads
 * there and in which style. Either may be left alone: a row can set only who, only how, or
 * both. The most particular place wins, as it always did.
 *
 * A leader links to its entry in Personas & teams.
 */
export default function WhereChatsStart({ view, act }: { view: api.WaysView; act: Act }) {
  const personas = useAppStore((s) => s.personas);
  const teams = useAppStore((s) => s.teams);
  const openPersonas = useAppStore((s) => s.openPersonas);

  const leaders = [
    { value: "", label: "Not set" },
    { value: "poiesis", label: "Poiesis" },
    ...personas.map((p) => ({ value: `persona:${p.id}`, label: p.name })),
    ...teams.map((t) => ({ value: `team:${t.slug}`, label: t.name })),
  ];
  const styles = view.presets.map((p) => ({ value: p.slug, label: p.name }));
  const styleOptions = (leader: string | null | undefined) => [
    { value: "", label: leader ? "The leader's style" : "Not set" },
    ...styles,
  ];

  const taken = new Set(view.starts.map((s) => s.scope));
  // A persona's own chats are set on its page: there they are led by it already.
  const places = view.scopes.filter((s) => s.value !== "everywhere" && !s.value.startsWith("persona:") && !taken.has(s.value));
  const [scope, setScope] = useState("");
  const [leader, setLeader] = useState("");
  const [style, setStyle] = useState("");
  const place = places.some((p) => p.value === scope) ? scope : (places[0]?.value ?? "");

  const leaderOf = (s: api.StartView) => (s.scope.startsWith("persona:") ? s.scope : (s.leader ?? ""));
  const open = (who: string) => openPersonas(who.startsWith("persona:") ? who.slice(8) : who.startsWith("team:") ? who : "poiesis");
  const nameOf = (who: string) => leaders.find((l) => l.value === who)?.label ?? "this one";

  return (
    <section className="ways-section" id="ways-starts">
      <div className="ways-heading">
        <h3>Where chats start</h3>
        <p className="ways-hint">
          Who leads a new chat, and in which style, depending on where it starts. The most particular place wins.
        </p>
      </div>

      {view.starts.map((s) => {
        const who = leaderOf(s);
        const own = s.scope.startsWith("persona:");
        return (
          <div className="ways-row ways-start" key={s.scope}>
            <div className="ways-text">{s.label}</div>
            {own ? (
              <span className="ways-start-fixed">Led by {nameOf(who)}</span>
            ) : (
              <Select
                label={`Who leads at ${s.label}`}
                value={who}
                options={leaders}
                onChange={(v) => act(() => api.setLeaderDefault(s.scope, v || null))}
              />
            )}
            {who && (
              <button className="why-link" aria-label={`Open ${nameOf(who)} in Personas & teams`} onClick={() => open(who)}>
                Open
              </button>
            )}
            <Select
              label={`Style at ${s.label}`}
              value={s.style ?? ""}
              options={styleOptions(who)}
              onChange={(v) => act(() => api.setStyleDefault(s.scope, v || null))}
            />
            {s.scope !== "global" ? (
              <button
                className="why-link"
                onClick={() =>
                  act(async () => {
                    if (!own) await api.setLeaderDefault(s.scope, null);
                    await api.setStyleDefault(s.scope, null);
                  })
                }
              >
                Remove
              </button>
            ) : (
              <span className="ways-start-gap" />
            )}
          </div>
        );
      })}

      {places.length > 0 && (
        <div className="ways-addrow">
          <span className="ways-word">For</span>
          <Select label="For" value={place} options={places} onChange={setScope} />
          <span className="ways-word">led by</span>
          <Select label="Led by" value={leader} options={leaders} onChange={setLeader} />
          <span className="ways-word">start in</span>
          <Select label="Start in" value={style} options={styleOptions(leader)} onChange={setStyle} />
          <span className="ways-addrow-actions">
            <button
              className="btn-secondary"
              disabled={!leader && !style}
              onClick={() =>
                act(async () => {
                  if (leader) await api.setLeaderDefault(place, leader);
                  if (style) await api.setStyleDefault(place, style);
                  setLeader("");
                  setStyle("");
                })
              }
            >
              Set
            </button>
          </span>
        </div>
      )}
    </section>
  );
}
