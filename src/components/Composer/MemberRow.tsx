import { whoIs } from "../../lib/teamLive";
import { useAddressing } from "../../lib/members";
import { useAppStore } from "../../lib/store";
import MemberGlyph from "../Personas/MemberGlyph";
import "./MemberRow.css";

/**
 * `TEAM-17`: who the next message goes to, in a session a team leads (`TEAMS_PLAN` II.5). Shown only
 * when the person has a line to a member besides the coordinator, so a Hub shows nothing. The
 * coordinator is chosen to begin with; pressing a face addresses the next message to that member, and
 * after the message is sent it is the coordinator again (the choice is a modifier, spent by the send).
 */
export default function MemberRow() {
  const { team, members, selected } = useAddressing();
  const personas = useAppStore((s) => s.personas);
  const set = useAppStore((s) => s.setTurnModifier);
  const clear = useAppStore((s) => s.clearTurnModifier);
  if (!team || members.length === 0) return null;

  const lead = whoIs(team, "coordinator", personas);
  const places = [
    { role: "coordinator", name: lead.name ?? "Poiesis", who: lead },
    ...members.map((m) => ({ role: m.role, name: m.name, who: whoIs(team, m.role, personas) })),
  ];
  const current = selected?.role ?? "coordinator";
  return (
    <div className="member-row" role="group" aria-label="Who to speak to">
      <span className="member-row-label">To:</span>
      {places.map((p) => (
        <button
          key={p.role}
          className={`member-row-pick${current === p.role ? " on" : ""}`}
          aria-pressed={current === p.role}
          title={p.role === "coordinator" ? `${p.name}, who leads` : `${p.name}, ${p.role}`}
          onClick={() => (p.role === "coordinator" ? clear("addressed") : set({ addressed: p.role }))}
        >
          <MemberGlyph who={p.who} size={16} />
          <span>{p.name}</span>
        </button>
      ))}
    </div>
  );
}
