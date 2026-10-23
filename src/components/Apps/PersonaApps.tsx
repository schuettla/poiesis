import { useEffect, useState } from "react";
import { composioStatus, getPersonaAppGrants, type ComposioStatus, type PersonaAppGrant } from "../../lib/api";

type Mode = "all" | "only";

/** No rows is "every connected app". The one row for no app at all is "no apps". */
function modeOf(grants: PersonaAppGrant[]): Mode {
  return grants.length === 0 ? "all" : "only";
}

function appsOf(grants: PersonaAppGrant[]): string[] {
  return grants.filter((g) => g.app !== "").map((g) => g.app);
}

/**
 * `CA-UI-6`: which connected apps one persona may reach. The choice is held by
 * the editor and saved with its Save, like tools and skills (`CAP-C-F10`).
 * `value` is `null` until the person changes something; the persona's stored
 * grants show until then.
 */
export default function PersonaApps({
  personaId,
  value,
  onChange,
}: {
  personaId: string | null;
  value: PersonaAppGrant[] | null;
  onChange: (grants: PersonaAppGrant[]) => void;
}) {
  const [status, setStatus] = useState<ComposioStatus | null>(null);
  const [stored, setStored] = useState<PersonaAppGrant[]>([]);
  const grants = value ?? stored;

  useEffect(() => {
    let current = true;
    composioStatus()
      .then((s) => current && setStatus(s))
      .catch(() => {});
    if (personaId) {
      getPersonaAppGrants(personaId)
        .then((g) => current && setStored(g))
        .catch(() => {});
    }
    return () => {
      current = false;
    };
  }, [personaId]);

  // Nothing to choose from until a key is saved and an app is signed in.
  if (!status?.configured) return null;
  const connected = [...new Map(status.accounts.filter((a) => a.status === "ACTIVE").map((a) => [a.app, a.app_name]))];
  if (connected.length === 0 || !status.connector_id) return null;
  const connectorId = status.connector_id;

  const mode = modeOf(grants);
  const chosen = appsOf(grants);

  const write = (next: PersonaAppGrant[]) => onChange(next);

  /** Grants for exactly these apps; none at all is the explicit "no apps". */
  const forApps = (apps: string[]): PersonaAppGrant[] =>
    apps.length === 0
      ? [{ connector_id: connectorId, app: "", tools: "[]" }]
      : apps.map((app) => ({ connector_id: connectorId, app, tools: "*" }));

  const group = personaId ?? "new";
  return (
    <div className="persona-tools">
      <label className="field-label">Apps this persona may use</label>
      <div className="persona-tools-list">
        <label className="persona-tools-row">
          <input type="radio" name={`apps-${group}`} checked={mode === "all"} onChange={() => write([])} />
          <span className="persona-tools-name">Every app I'm connected to</span>
        </label>
        <label className="persona-tools-row">
          <input
            type="radio"
            name={`apps-${group}`}
            checked={mode === "only"}
            onChange={() => write(forApps(connected.map(([app]) => app)))}
          />
          <span className="persona-tools-name">Only these</span>
        </label>
        {mode === "only" &&
          connected.map(([app, name]) => (
            <label className="persona-tools-row" key={app} style={{ marginLeft: 22 }}>
              <input
                type="checkbox"
                checked={chosen.includes(app)}
                onChange={() =>
                  write(forApps(chosen.includes(app) ? chosen.filter((a) => a !== app) : [...chosen, app]))
                }
              />
              <span className="persona-tools-name">{name}</span>
            </label>
          ))}
        {mode === "only" && chosen.length === 0 && <p className="persona-tools-note">No apps.</p>}
      </div>
    </div>
  );
}
