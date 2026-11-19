import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import type { Persona } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import { MenuItem, RowMenu } from "../Self/WaysTab";

/**
 * `TEAM-5`: what a persona knows. "Remember across chats" decides whether it learns
 * anything for itself; the entries below are what it did learn, each of which has a ⋯ menu:
 * share it with everyone (the tag is cleared) or forget it (set aside in memory's trash).
 *
 * Both act at once, like the entries do, so they are not part of the page's Save.
 */
export default function PersonaKnows({ persona }: { persona: Persona }) {
  const updatePersona = useAppStore((s) => s.updatePersona);
  const openSelf = useAppStore((s) => s.openSelf);
  const [entries, setEntries] = useState<{ collection: "facts" | "lessons"; entry: api.Fact }[]>([]);
  const [error, setError] = useState<string | null>(null);
  const remembers = persona.remembers !== false;

  async function load() {
    if (!api.inTauri()) return;
    const [facts, lessons] = await Promise.all([
      api.listMemoryFacts().catch(() => [] as api.Fact[]),
      api.listLessons().catch(() => [] as api.Fact[]),
    ]);
    // An earlier version that a newer one replaced is not something it knows now.
    const own = (f: api.Fact) => f.persona === persona.id && f.status !== "superseded";
    setEntries([
      ...facts.filter(own).map((entry) => ({ collection: "facts" as const, entry })),
      ...lessons.filter(own).map((entry) => ({ collection: "lessons" as const, entry })),
    ]);
  }

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [persona.id]);

  async function act(run: () => Promise<unknown>) {
    setError(null);
    try {
      await run();
      await load();
      void useAppStore.getState().refreshMemoryContext();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  }

  return (
    <>
      <label className="persona-spawnable">
        <input
          type="checkbox"
          checked={remembers}
          onChange={(e) => void act(() => updatePersona({ ...persona, remembers: e.target.checked }))}
        />
        Remember across chats
      </label>
      <p className="field-hint">
        {remembers
          ? `What ${persona.name} learns in chats led by ${persona.name} is kept for next time. What is true of you everywhere stays shared.`
          : `${persona.name} keeps nothing and sees only what everyone knows.`}
      </p>

      {entries.length === 0 ? (
        <p className="field-hint">Nothing yet.</p>
      ) : (
        <ul className="persona-knows" aria-label={`What ${persona.name} knows`}>
          {entries.map(({ collection, entry }) => (
            <li key={`${collection}:${entry.name}`} className="persona-knows-row">
              <span className="persona-knows-text" title={entry.body}>
                {entry.description || entry.name}
                {collection === "lessons" && <span className="persona-knows-kind"> · lesson</span>}
              </span>
              <RowMenu label={`More for ${entry.description || entry.name}`}>
                {(close) => (
                  <>
                    <MenuItem
                      onClick={() => {
                        close();
                        void act(() => api.shareMemory(collection, entry.name));
                      }}
                    >
                      Share with everyone
                    </MenuItem>
                    <MenuItem
                      danger
                      onClick={() => {
                        close();
                        void act(() => (collection === "facts" ? api.forgetMemoryFact(entry.name) : api.forgetLesson(entry.name)));
                      }}
                    >
                      Forget
                    </MenuItem>
                  </>
                )}
              </RowMenu>
            </li>
          ))}
        </ul>
      )}
      {error && (
        <p className="persona-tools-note" role="alert">
          {error}
        </p>
      )}
      <p className="field-hint">
        <button className="link-btn" onClick={() => openSelf("memory", `persona:${persona.id}`)}>
          See it in Memory
        </button>
      </p>
    </>
  );
}
