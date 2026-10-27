import { useEffect, useState } from "react";
import { mailCategories, setMailCategories, type MailCategories } from "../../lib/api";
import "./MailInbox.css";

/** The most kinds I sort into, and the longest name. The backend cuts to the same. */
export const MAX_KINDS = 8;
const MAX_NAME = 24;

const clean = (e: unknown) => String(e).replace(/^Error:\s*/, "");
const sameList = (a: string[], b: string[]) => a.length === b.length && a.every((k, i) => k === b[i]);

/**
 * `USE-UI-4`: the kinds mail is sorted into, on the Accounts tab under the account
 * list. They are the person's own: add one, take one away, or go back to the
 * default list. `unsure` is always there and is not on the list.
 */
export default function KindEditor() {
  const [state, setState] = useState<MailCategories | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    mailCategories()
      .then((c) => live && setState(c))
      .catch((e) => live && setError(clean(e)));
    return () => {
      live = false;
    };
  }, []);

  async function save(list: string[]) {
    setError(null);
    try {
      setState(await setMailCategories(list));
    } catch (e) {
      setError(clean(e));
    }
  }

  if (!state) return error ? <p className="hw-note error">{error}</p> : null;

  const name = draft.trim().toLowerCase();
  const exists = state.categories.includes(name) || name === "unsure";
  const full = state.categories.length >= MAX_KINDS;

  function add() {
    if (!name || exists || full || !state) return;
    setDraft("");
    void save([...state.categories, name]);
  }

  return (
    <section className="mail-kinds" aria-label="Kinds of mail">
      <h2 className="mail-kinds-title">Kinds of mail</h2>
      <p className="field-hint">
        I sort new mail into these, and into "unsure" when none fits. Changing the list does not change mail that is
        already tagged.
      </p>
      <ul className="mail-kind-list">
        {state.categories.map((kind) => (
          <li key={kind} className="mail-kind-chip">
            <span>{kind}</span>
            <button
              className="mail-kind-remove"
              aria-label={`Remove ${kind}`}
              disabled={state.categories.length === 1}
              onClick={() => void save(state.categories.filter((k) => k !== kind))}
            >
              ×
            </button>
          </li>
        ))}
      </ul>
      <div className="mail-kind-add">
        <input
          className="field-input"
          placeholder={full ? `Up to ${MAX_KINDS} kinds` : "Add a kind"}
          aria-label="Add a kind"
          value={draft}
          maxLength={MAX_NAME}
          disabled={full}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") add();
          }}
        />
        <button className="btn-secondary" onClick={add} disabled={!name || exists || full}>
          Add
        </button>
      </div>
      {!sameList(state.categories, state.defaults) && (
        <button className="link-button inline" onClick={() => void save([])}>
          Back to the default list
        </button>
      )}
      {error && <p className="hw-note error">{error}</p>}
    </section>
  );
}
