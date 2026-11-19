import { useEffect, useState } from "react";
import { useAppStore } from "../../lib/store";
import * as api from "../../lib/api";
import { PoiesisGlyph } from "./PersonaGlyph";

/**
 * `TEAM-4`: Poiesis as the first entry of Personas & teams. It is fixed: it cannot be
 * renamed or deleted, and its instructions are the global system prompt (the block that
 * used to sit in General), edited through the same `setSystemPrompt` call.
 */
export default function PoiesisEntry() {
  const systemPrompt = useAppStore((s) => s.systemPrompt);
  const setSystemPrompt = useAppStore((s) => s.setSystemPrompt);
  const setView = useAppStore((s) => s.setView);
  const openSelf = useAppStore((s) => s.openSelf);
  const [draft, setDraft] = useState(systemPrompt);
  const [saved, setSaved] = useState(false);
  const [style, setStyle] = useState<string | null>(null);

  useEffect(() => setDraft(systemPrompt), [systemPrompt]);
  useEffect(() => {
    if (!api.inTauri()) return;
    api
      .waysView()
      .then((v) => setStyle(v.defaults.find((d) => d.scope === "global")?.preset_name ?? null))
      .catch(() => {});
  }, []);

  async function save() {
    await setSystemPrompt(draft);
    setSaved(true);
    setTimeout(() => setSaved(false), 1600);
  }

  return (
    <section className="self-card ways-detail persona-card" aria-label="Poiesis">
      <div className="ways-detail-head">
        <PoiesisGlyph size={30} />
        <div className="ways-detail-title">
          <h3 className="persona-head-name">Poiesis</h3>
          <p className="persona-head-line">Leads every chat, unless you hand it to someone else. It cannot be renamed or deleted.</p>
        </div>
      </div>

      <div className="persona-who">
        <label className="field-label" htmlFor="poiesis-prompt">
          Who Poiesis is
        </label>
        <p className="field-hint">The instructions for every chat Poiesis leads.</p>
        <textarea
          id="poiesis-prompt"
          className="system-prompt"
          aria-label="Who Poiesis is"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          rows={6}
          spellCheck={false}
        />
        <div className="setting-actions">
          <button className="btn-primary" onClick={save} disabled={draft === systemPrompt}>
            Save
          </button>
          {saved && <span className="saved-note">Saved</span>}
        </div>
      </div>

      <dl className="poiesis-lines">
        <dt>What Poiesis uses</dt>
        <dd>
          <button className="why-link" onClick={() => setView("models")}>
            Models
          </button>
          {" · "}
          <button className="why-link" onClick={() => setView("tools")}>
            Tools
          </button>
          {" · "}
          <button className="why-link" onClick={() => setView("skills")}>
            Skills
          </button>
          {" · "}
          <button className="why-link" onClick={() => setView("apps")}>
            Apps
          </button>
        </dd>
        <dt>How Poiesis works</dt>
        <dd>
          {style ? `Chats start in ${style}, unless a place or a persona says otherwise. ` : ""}
          <button className="why-link" onClick={() => openSelf("ways", "starts")}>
            Change it in How I work
          </button>
        </dd>
        <dt>What Poiesis knows</dt>
        <dd>
          What I remember about you and your work.{" "}
          <button className="why-link" onClick={() => openSelf("memory", "poiesis")}>
            Open Memory
          </button>
        </dd>
      </dl>
    </section>
  );
}
