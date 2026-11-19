import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import { useAppStore } from "../../lib/store";

/**
 * `TEAM-2`: who leads this chat, for the composer's button and the "Who leads" row.
 *
 * Somebody's choice for this chat is read from the chat itself (`leader`, or the
 * `personaId` a chat had before leaders existed). A chat nobody led yet starts where
 * its place says (`TEAM-1`, `defaults.md`), asked of the backend so the button says
 * who will answer the next message. A persona that no longer exists leads nothing.
 *
 * `change` opens the `/lead` menu, which is where it is changed.
 */
export function useLeaderState() {
  const convId = useAppStore((s) => s.activeConversationId);
  const own = useAppStore((s) => {
    const c = s.conversations.find((x) => x.id === s.activeConversationId);
    return c?.leader ?? (c?.personaId ? `persona:${c.personaId}` : null);
  });
  const personas = useAppStore((s) => s.personas);
  const teams = useAppStore((s) => s.teams);
  const requestComposer = useAppStore((s) => s.requestComposer);

  const [started, setStarted] = useState<string | null>(null);
  useEffect(() => {
    if (!convId || own || !api.inTauri()) {
      setStarted(null);
      return;
    }
    let cancelled = false;
    api
      .conversationLeader(convId)
      .then((l) => !cancelled && setStarted(l?.source === "default" ? l.leader : null))
      .catch(() => !cancelled && setStarted(null));
    return () => {
      cancelled = true;
    };
  }, [convId, own, personas, teams]);

  const leader = own ?? started ?? "poiesis";
  const persona = leader.startsWith("persona:") ? (personas.find((p) => p.id === leader.slice("persona:".length)) ?? null) : null;
  const team = leader.startsWith("team:") ? (teams.find((t) => t.slug === leader.slice("team:".length)) ?? null) : null;
  // A team is drawn and named by its coordinator (Part II.2).
  const coordinator = team?.coordinator.startsWith("persona:")
    ? (personas.find((p) => `persona:${p.id}` === team.coordinator) ?? null)
    : null;
  const teamPersonas = team ? team.members.filter((m) => m.persona.startsWith("persona:")).length : 0;
  return {
    convId,
    leader: persona || team ? leader : "poiesis",
    persona,
    team,
    coordinator,
    name: persona?.name ?? team?.name ?? "Poiesis",
    /** The rest of the "Led by" line: the persona's own line, or who coordinates a team. */
    about: persona
      ? (persona.line ?? "")
      : team
        ? `coordinator ${coordinator?.name ?? "Poiesis"}, ${teamPersonas} persona${teamPersonas === 1 ? "" : "s"}`
        : "",
    /** Somebody other than Poiesis leads: the button says so. */
    led: !!persona || !!team,
    /** Where it was decided, for the hover. */
    where: own ? "Your choice for this chat" : started ? "Where chats like this start" : "Poiesis unless you say otherwise",
    change: () => requestComposer("/lead "),
  };
}
