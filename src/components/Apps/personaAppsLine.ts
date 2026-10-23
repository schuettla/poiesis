import { useEffect, useState } from "react";
import { getPersonaAppGrants, inTauri, type PersonaAppGrant } from "../../lib/api";
import { useAppStore } from "../../lib/store";

/**
 * `CA-UI-6`: the apps a narrowed persona may use, as one line for a tooltip:
 * "Apps: Notion, Linear". A persona that is not narrowed has no line at all,
 * since "every app" is what a chat without a persona has too.
 */
export function appsLine(grants: PersonaAppGrant[], nameOf: (slug: string) => string): string | null {
  if (grants.length === 0) return null;
  const apps = grants.filter((g) => g.app !== "").map((g) => nameOf(g.app));
  return apps.length > 0 ? `Apps: ${apps.join(", ")}` : "Apps: none";
}

// One read per persona, shared by every chip and Fleet row that shows it, and
// read again once the persona's apps are saved.
const asked = new Map<string, Promise<PersonaAppGrant[]>>();
const listeners = new Set<() => void>();

/** The persona editor saved this persona's apps: read them again. */
export function forgetPersonaApps(personaId?: string) {
  if (personaId) asked.delete(personaId);
  else asked.clear();
  listeners.forEach((l) => l());
}

function grantsOf(personaId: string): Promise<PersonaAppGrant[]> {
  let p = asked.get(personaId);
  if (!p) {
    p = getPersonaAppGrants(personaId).catch(() => []);
    asked.set(personaId, p);
  }
  return p;
}

export function usePersonaAppsLine(personaId: string | null | undefined): string | null {
  const accounts = useAppStore((s) => s.appAccounts);
  const [grants, setGrants] = useState<PersonaAppGrant[] | null>(null);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    const bump = () => setVersion((v) => v + 1);
    listeners.add(bump);
    return () => {
      listeners.delete(bump);
    };
  }, []);

  useEffect(() => {
    if (!personaId || !inTauri()) {
      setGrants(null);
      return;
    }
    let current = true;
    void grantsOf(personaId).then((g) => current && setGrants(g));
    return () => {
      current = false;
    };
  }, [personaId, version]);

  if (!grants) return null;
  return appsLine(grants, (slug) => accounts.find((a) => a.app === slug)?.app_name ?? slug);
}
