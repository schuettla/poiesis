import { useEffect, useState } from "react";
import * as api from "./api";
import { leaderOf } from "./leaders";
import { useAppStore } from "./store";

/**
 * `TEAM-14`: the team that leads a chat (`team:<slug>`), read from its file. `slug` is null for a
 * chat nobody's team leads, and then `team` is null too and nothing is read. `team` is `undefined`
 * while the file is being read and null when it is gone, so a view can tell waiting from missing.
 */
export function useLedTeam(conversationId: string | null | undefined): { slug: string | null; team: api.Team | null | undefined } {
  const slug = useAppStore((s) => {
    const conv = conversationId ? s.conversations.find((c) => c.id === conversationId) : undefined;
    const leader = conv ? leaderOf(conv) : "";
    return leader.startsWith("team:") ? leader.slice(5) : null;
  });
  const [read, setRead] = useState<{ slug: string; team: api.Team | null } | null>(null);

  useEffect(() => {
    if (!slug) return;
    let live = true;
    api
      .teamGet(slug)
      .then((r) => live && setRead({ slug, team: r?.team ?? null }))
      .catch(() => live && setRead({ slug, team: null }));
    return () => {
      live = false;
    };
  }, [slug]);

  if (!slug) return { slug: null, team: null };
  return { slug, team: read?.slug === slug ? read.team : undefined };
}
