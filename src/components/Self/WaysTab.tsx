import { useCallback, useEffect, useState } from "react";
import * as api from "../../lib/api";
import { useAppStore } from "../../lib/store";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import { KebabIcon } from "../Icons/Icons";
import StyleGlyph from "../Mark/StyleGlyph";
import Select from "../Select/Select";
import Toggle from "../Toggle/Toggle";
import PresetHistory from "./PresetHistory";
import { ordinal, shortDate } from "./waysFormat";
import "./Ways.css";

/** What a rule says about where it came from, in a plain line. */
function note(w: api.WayView): string | null {
  if (w.status === "always" || w.origin === "shipped") return null;
  if (w.status === "trying" && w.trial) {
    const left = w.trial.days_left === 1 ? "1 day" : `${w.trial.days_left} days`;
    return `Trying it for the next ${w.trial.tasks} tasks of its kind, ${w.trial.done} so far, at most ${left} more.`;
  }
  if (w.origin === "you") return w.since ? `Your own, added ${w.since}.` : "Your own.";
  return w.since ? `From ${w.origin}, ${w.since}.` : `From ${w.origin}.`;
}

/**
 * `WRK-16`, `WRK-17`: Self → How I work, in two words. A **style** is how I organise a
 * task (Lean, Swarm, one of your own); a **rule** is one sentence, "When …, I …", made
 * of two fixed lists so nothing can be written that the loop cannot do.
 *
 * The page reads top to bottom the way a person asks: what is waiting for me, which
 * styles are there (cards, one picked), what does the picked one do (its rules, where it
 * starts, whether it may change), what holds in every style, and, folded away, the rest
 * (where each place starts, deleted styles, going back to a day). Every change is a
 * plain file in `memory/ways/`.
 */
export default function WaysTab() {
  const presetsInStore = useAppStore((s) => s.presets);
  const loadPresets = useAppStore((s) => s.loadPresets);
  const openPresetDialog = useAppStore((s) => s.openPresetDialog);
  const setActiveConversation = useAppStore((s) => s.setActiveConversation);

  const [view, setView] = useState<api.WaysView | null>(null);
  const [shown, setShown] = useState<string | null>(null);
  const [problem, setProblem] = useState("");
  const [adding, setAdding] = useState(false);
  const [addingToStyle, setAddingToStyle] = useState(false);
  const [asking, setAsking] = useState<{ slug: string; name: string; usedBy: string[] } | null>(null);
  const [history, setHistory] = useState(false);
  /** After going back, the style that could be locked like this. */
  const [offerLock, setOfferLock] = useState<string | null>(null);

  const load = useCallback(async (preset?: string | null) => {
    if (!api.inTauri()) return;
    try {
      setView(await api.waysView(preset));
    } catch (e) {
      setProblem(String(e));
    }
  }, []);

  // Reload when the list of styles changes anywhere (the dialog, the menu).
  useEffect(() => {
    void load(shown);
  }, [load, shown, presetsInStore]);

  // A different style shown closes what was open for the last one.
  useEffect(() => {
    setAddingToStyle(false);
  }, [shown]);

  /** Do something, show the answer, say plainly when it did not work. */
  async function act(fn: () => Promise<unknown>, after?: () => void) {
    setProblem("");
    try {
      await fn();
      after?.();
      await loadPresets();
      await load(shown);
    } catch (e) {
      setProblem(String(e));
    }
  }

  if (!view) {
    return <p className="setting-help">{api.inTauri() ? "Loading…" : "I can only show how I work in the desktop app."}</p>;
  }

  const preset = view.presets.find((p) => p.slug === view.preset) ?? view.presets[0];
  const isAdHoc = preset?.slug === "ad-hoc";
  const everywhere = view.ways.filter((w) => w.status !== "retired");
  const retired = view.ways.filter((w) => w.status === "retired");
  const parent = preset?.parent ? view.presets.find((p) => p.slug === preset.parent) : undefined;
  const undoable = view.recent.filter((r) => r.undoable);

  const triggers = view.triggers.map((t) => ({ value: t.value, label: t.label }));
  const moves = view.moves.map((m) => ({ value: m.value, label: m.label }));

  async function remove(p: api.PresetView) {
    setProblem("");
    try {
      const r = await api.deletePreset(p.slug, false);
      if (r.deleted) {
        setShown(null);
        await loadPresets();
      } else {
        setAsking({ slug: p.slug, name: p.name, usedBy: r.used_by });
      }
    } catch (e) {
      setProblem(String(e));
    }
  }

  return (
    <div className="self-list ways">
      <div className="ways-intro">
        <p className="ways-lede">
          A <strong>style</strong> is how I organise a task. Each style is a few <strong>rules</strong>: when
          something happens, I do something about it. New chats start in your default style, and you can switch any
          chat with the style button under the message box.
        </p>
        <p className="ways-story">{view.narrative}</p>
      </div>

      {(view.proposals.length > 0 || undoable.length > 0) && (
        <Section title="Waiting for you">
          {view.proposals.map((p) => (
            <div className="ways-proposal" key={p.id} role="group" aria-label={p.lead}>
              <p className="ways-proposal-lead">{p.lead}</p>
              <p className="ways-proposal-quote">{p.quote}</p>
              <p className="ways-note">Why: {p.why}</p>
              <div className="ways-proposal-actions">
                {p.answers.map(([answer, label], i) => (
                  <button
                    key={answer}
                    className={i === 0 ? "btn-primary" : "btn-text"}
                    onClick={() => act(() => api.wayProposalAnswer(p.id, answer))}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
          ))}
          {undoable.map((r) => (
            <div className="ways-row" key={r.id}>
              <div className="ways-text">I {r.said}.</div>
              <button className="why-link" onClick={() => act(() => api.waysUndo(r.id))}>
                Undo
              </button>
            </div>
          ))}
        </Section>
      )}

      <Section title="Styles">
        <div className="ways-styles" role="radiogroup" aria-label="Styles">
          {view.presets.map((p) => {
            const on = p.slug === preset?.slug;
            return (
              <button
                key={p.slug}
                role="radio"
                aria-checked={on}
                className={`ways-style${on ? " on" : ""}`}
                onClick={() => setShown(p.slug)}
              >
                <span className="ways-style-top">
                  <StyleGlyph glyph={p.glyph} size={20} branch={!!p.parent} />
                  {view.global_default === p.slug && <span className="ways-badge">Default</span>}
                  {!p.built_in && view.global_default !== p.slug && <span className="ways-badge quiet">Yours</span>}
                </span>
                <span className="ways-style-name">{p.name}</span>
                <span className="ways-style-line">{p.line}</span>
              </button>
            );
          })}
          <button
            className="ways-style new"
            onClick={() => openPresetDialog({ mode: "new", from: isAdHoc ? null : (preset?.slug ?? null), onDone: setShown })}
          >
            <span className="ways-style-plus" aria-hidden="true">
              +
            </span>
            <span className="ways-style-name">New style</span>
            <span className="ways-style-line">From scratch or from one of these</span>
          </button>
        </div>
      </Section>

      {preset && (
        <section className="self-card ways-detail" aria-label={preset.name}>
          <div className="ways-detail-head">
            <StyleGlyph glyph={preset.glyph} size={26} branch={!!preset.parent} />
            <div className="ways-detail-title">
              <h3>{preset.name}</h3>
              <p>{preset.line}</p>
            </div>
            {!isAdHoc && (
              <RowMenu label={`More for ${preset.name}`}>
                {(close) => (
                  <>
                    {!preset.built_in && (
                      <MenuItem
                        onClick={() => {
                          close();
                          openPresetDialog({ mode: "rename", slug: preset.slug, name: preset.name, line: preset.line });
                        }}
                      >
                        Rename
                      </MenuItem>
                    )}
                    <MenuItem
                      onClick={() => {
                        close();
                        openPresetDialog({
                          mode: "new",
                          from: preset.slug,
                          name: preset.built_in ? "" : `${preset.name} copy`,
                          line: preset.built_in ? "" : preset.line,
                          onDone: setShown,
                        });
                      }}
                    >
                      {preset.built_in ? "Make my own from it…" : "Duplicate"}
                    </MenuItem>
                    {parent && (
                      <MenuItem
                        onClick={() => {
                          close();
                          void act(
                            () => api.presetMerge(preset.slug),
                            () => setShown(parent.slug)
                          );
                        }}
                      >
                        Bring back into {parent.name}
                      </MenuItem>
                    )}
                    {!preset.built_in && (
                      <MenuItem
                        danger
                        onClick={() => {
                          close();
                          void remove(preset);
                        }}
                      >
                        Delete
                      </MenuItem>
                    )}
                  </>
                )}
              </RowMenu>
            )}
          </div>

          <div className="ways-detail-switches">
            <label className="ways-switch">
              <Toggle
                checked={view.global_default === preset.slug}
                onChange={(on) => act(() => api.setStyleDefault("global", on ? preset.slug : null))}
                label="New chats start in this style"
              />
              <span>New chats start in this style</span>
            </label>
            {!isAdHoc && (
              <label className="ways-switch">
                <Toggle
                  checked={!preset.locked}
                  onChange={(on) => act(() => api.updatePreset(preset.slug, { locked: !on }))}
                  label="I may improve it"
                />
                <span>
                  I may improve it
                  <span className="ways-switch-help">
                    {preset.locked
                      ? "Locked: I never change its rules on my own."
                      : "From how chats go, I suggest changes to its rules."}
                  </span>
                </span>
              </label>
            )}
          </div>

          {!isAdHoc && (
            <p className="ways-version">
              {preset.name}, {ordinal(preset.version)} version
              {preset.lineage ? ` · ${preset.lineage}` : ""}
              {" · "}
              <button className="why-link" onClick={() => setHistory(true)}>
                History
              </button>
              {preset.version > 1 && (
                <>
                  {" · "}
                  <button
                    className="why-link"
                    onClick={() =>
                      act(
                        () => api.presetBack(preset.slug, null),
                        () => setOfferLock(preset.slug)
                      )
                    }
                  >
                    Back to original
                  </button>
                </>
              )}
            </p>
          )}
          {offerLock && preset.slug === offerLock && !preset.locked && (
            <p className="ways-offer">
              Lock it like this?{" "}
              <button
                className="why-link"
                onClick={() =>
                  act(
                    () => api.updatePreset(offerLock, { locked: true }),
                    () => setOfferLock(null)
                  )
                }
              >
                Lock
              </button>
            </p>
          )}

          {isAdHoc ? (
            <p className="ways-quiet">
              Ad hoc has no rules of its own. I just answer, and follow only the rules that hold in every style.
            </p>
          ) : (
            <div className="ways-rules">
              <h4 className="ways-subhead">Its rules</h4>
              {view.rules.length === 0 && !addingToStyle && (
                <p className="ways-quiet">No rules yet. Add the first one below.</p>
              )}
              {view.rules.map((r) => (
                <div className={`ways-row${r.on ? "" : " off"}`} key={`${r.when}-${r.move}`}>
                  <div className="ways-text">
                    <span className={r.on ? "" : "ways-struck"}>{r.text}</span>
                    {r.on && !r.original && <span className="ways-tag">added</span>}
                  </div>
                  <Toggle
                    checked={r.on}
                    onChange={(on) => act(() => api.setPresetRule(preset.slug, { when: r.when, move: r.move }, on))}
                    label={r.text}
                  />
                </div>
              ))}
              {addingToStyle ? (
                <AddRow
                  triggers={triggers}
                  moves={moves}
                  label={`Add to ${preset.name}`}
                  onAdd={(when, move) =>
                    act(() => api.setPresetRule(preset.slug, { when, move }, true), () => setAddingToStyle(false))
                  }
                  onCancel={() => setAddingToStyle(false)}
                />
              ) : (
                <button className="ways-add-btn" onClick={() => setAddingToStyle(true)}>
                  + Add a rule to {preset.name}
                </button>
              )}
            </div>
          )}
        </section>
      )}

      {problem && (
        <p className="ways-problem" role="alert">
          {problem}
        </p>
      )}

      <Section title="Rules in every style" hint="These hold whatever style a chat uses.">
        {everywhere.length === 0 && <p className="ways-quiet">None are on.</p>}
        {everywhere.map((w) => (
          <WayRow key={w.slug} way={w} view={view} act={act} openChat={setActiveConversation} />
        ))}
        {adding ? (
          <AddRow
            triggers={triggers}
            moves={moves}
            scopes={view.scopes}
            label="Add"
            onAdd={(when, move, scope) => act(() => api.addWay(when, move, scope ?? "everywhere"), () => setAdding(false))}
            onCancel={() => setAdding(false)}
          />
        ) : (
          <button className="ways-add-btn" onClick={() => setAdding(true)}>
            + Add a rule for every style
          </button>
        )}
        {retired.length > 0 && (
          <details className="ways-fold">
            <summary>Switched off ({retired.length})</summary>
            {retired.map((w) => (
              <WayRow key={w.slug} way={w} view={view} act={act} openChat={setActiveConversation} />
            ))}
          </details>
        )}
      </Section>

      <details className="ways-fold ways-advanced">
        <summary>More settings</summary>
        <Defaults view={view} act={act} />
        {view.deleted.length > 0 && (
          <Section title="Deleted styles">
            {view.deleted.map((d) => (
              <div className="ways-row" key={d.file}>
                <div className="ways-text">{d.name}</div>
                <button className="why-link" onClick={() => act(() => api.restorePreset(d.file))}>
                  Bring back
                </button>
              </div>
            ))}
          </Section>
        )}
        <GoBack view={view} act={act} />
      </details>

      {history && preset && (
        <PresetHistory
          slug={preset.slug}
          name={preset.name}
          glyph={preset.glyph}
          onClose={() => setHistory(false)}
          onChanged={() => {
            setOfferLock(preset.slug);
            void loadPresets().then(() => load(shown));
          }}
        />
      )}

      {asking && (
        <ConfirmDialog
          title={`Delete "${asking.name}"?`}
          body={`"${asking.name}" is the default for ${joinWords(asking.usedBy)}. Those go back to Ad hoc. You can bring it back afterwards.`}
          confirmLabel="Delete"
          onCancel={() => setAsking(null)}
          onConfirm={() => {
            const slug = asking.slug;
            setAsking(null);
            void act(() => api.deletePreset(slug, true), () => setShown(null));
          }}
        />
      )}
    </div>
  );
}

function joinWords(items: string[]): string {
  if (items.length <= 1) return items[0] ?? "";
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

function Section({ title, hint, children }: { title: string; hint?: string; children?: React.ReactNode }) {
  return (
    <section className="ways-section">
      <div className="ways-heading">
        <h3>{title}</h3>
        {hint && <p className="ways-hint">{hint}</p>}
      </div>
      {children}
    </section>
  );
}

function MenuItem({ onClick, danger, children }: { onClick: () => void; danger?: boolean; children: React.ReactNode }) {
  return (
    <button className={`row-menu-item${danger ? " danger" : ""}`} role="menuitem" onClick={onClick}>
      {children}
    </button>
  );
}

/** The ⋯ button and the small menu it opens; the shared row-menu look. */
function RowMenu({ label, children }: { label: string; children: (close: () => void) => React.ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <span className="ways-menu">
      <button
        className="ways-more"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        title="More"
        onClick={() => setOpen((o) => !o)}
      >
        <KebabIcon size={14} />
      </button>
      {open && (
        <>
          <div className="row-menu-backdrop" onClick={() => setOpen(false)} />
          <div className="row-menu" role="menu">
            {children(() => setOpen(false))}
          </div>
        </>
      )}
    </span>
  );
}

type Act = (fn: () => Promise<unknown>, after?: () => void) => Promise<void>;

/** The word for how a rule in every style stands, beside its sentence. */
function standing(w: api.WayView): string | null {
  if (w.status === "trying") return "trying out";
  // Your own say so in their note; what I learned says so here.
  if (w.status === "learned" && w.origin !== "you") return "learned";
  return null;
}

/** One rule that holds in every style: its sentence, a switch, and the ⋯ menu. */
function WayRow({
  way,
  view,
  act,
  openChat,
}: {
  way: api.WayView;
  view: api.WaysView;
  act: Act;
  openChat: (id: string) => Promise<void>;
}) {
  const [editing, setEditing] = useState(false);
  const base = way.status === "always";
  const on = way.status !== "retired";
  const line = note(way);
  const tag = standing(way);

  if (editing) {
    return (
      <div className="ways-row editing">
        <AddRow
          triggers={view.triggers}
          moves={view.moves}
          initial={{ when: way.when, move: way.move }}
          label="Save"
          onAdd={(when, move) => act(() => api.updateWay(way.slug, { when, move }), () => setEditing(false))}
          onCancel={() => setEditing(false)}
        />
      </div>
    );
  }

  return (
    <div className={`ways-row${on ? "" : " off"}`}>
      <div className="ways-text">
        <span className={on ? "" : "ways-struck"}>{way.text}</span>
        {tag && <span className="ways-tag">{tag}</span>}
        {way.scope !== "everywhere" && <span className="ways-tag">{way.scope_label}</span>}
        {line && <span className="ways-note">{line}</span>}
      </div>
      <Toggle
        checked={on}
        onChange={(next) => act(() => api.updateWay(way.slug, { status: next ? (base ? "always" : "learned") : "retired" }))}
        label={way.text}
      />
      <RowMenu label={`More for: ${way.text}`}>
        {(close) => (
          <WayMenu
            way={way}
            view={view}
            base={base}
            on={on}
            close={close}
            edit={() => setEditing(true)}
            act={act}
            openChat={openChat}
          />
        )}
      </RowMenu>
    </div>
  );
}

/**
 * "When [ … ▾ ], I [ … ▾ ]" and, for a rule in every style, "Only in [ … ▾ ]". Each
 * part is a select over the fixed lists, so a sentence the loop cannot carry out
 * cannot be made. The finished sentence is read back under it before it is added.
 */
function AddRow({
  triggers,
  moves,
  scopes,
  initial,
  label,
  onAdd,
  onCancel,
}: {
  triggers: api.WaysChoice[];
  moves: api.WaysChoice[];
  scopes?: api.WaysChoice[];
  initial?: { when: string; move: string };
  label: string;
  onAdd: (when: string, move: string, scope?: string) => void;
  onCancel: () => void;
}) {
  const [when, setWhen] = useState(initial?.when ?? triggers[0]?.value ?? "");
  const [move, setMove] = useState(initial?.move ?? moves[0]?.value ?? "");
  const [scope, setScope] = useState("everywhere");
  const whenWords = triggers.find((t) => t.value === when)?.label ?? "";
  const moveWords = moves.find((m) => m.value === move)?.label ?? "";
  return (
    <div className="ways-addrow">
      <span className="ways-word">When</span>
      <Select label="When" value={when} options={triggers} onChange={setWhen} />
      <span className="ways-word">then</span>
      <Select label="I" value={move} options={moves} onChange={setMove} />
      {scopes && (
        <>
          <span className="ways-word">Only in</span>
          <Select label="Only in" value={scope} options={scopes} onChange={setScope} />
        </>
      )}
      <p className="ways-readback">
        “When {whenWords}, {moveWords}.”
      </p>
      <span className="ways-addrow-actions">
        <button className="btn-secondary" onClick={onCancel}>
          Cancel
        </button>
        <button className="btn-primary" onClick={() => onAdd(when, move, scopes ? scope : undefined)}>
          {label}
        </button>
      </span>
    </div>
  );
}

/** `WRK-17`: which style a project, a persona or a kind of work starts in. Everywhere
 * else is the "New chats start in this style" switch on a style. */
function Defaults({ view, act }: { view: api.WaysView; act: Act }) {
  const places = view.scopes.filter((s) => s.value !== "everywhere");
  const [scope, setScope] = useState(places[0]?.value ?? "");
  const [slug, setSlug] = useState(view.presets.find((p) => p.slug === "lean")?.slug ?? view.presets[0]?.slug ?? "");
  const others = view.defaults.filter((d) => d.scope !== "global");
  if (places.length === 0 && others.length === 0) return null;
  return (
    <Section title="A different style for one place" hint="A project, a persona or a kind of work can start in its own style.">
      {others.map((d) => (
        <div className="ways-row" key={d.scope}>
          <div className="ways-text">
            {d.label}: {d.preset_name}
          </div>
          <button className="why-link" onClick={() => act(() => api.setStyleDefault(d.scope, null))}>
            Remove
          </button>
        </div>
      ))}
      {places.length > 0 && (
        <div className="ways-addrow">
          <span className="ways-word">For</span>
          <Select label="For" value={scope} options={places} onChange={setScope} />
          <span className="ways-word">start in</span>
          <Select
            label="Start in"
            value={slug}
            options={view.presets.map((p) => ({ value: p.slug, label: p.name }))}
            onChange={setSlug}
          />
          <span className="ways-addrow-actions">
            <button className="btn-secondary" onClick={() => act(() => api.setStyleDefault(scope, slug))}>
              Set
            </button>
          </span>
        </div>
      )}
    </Section>
  );
}

/** Going back to how I worked on an earlier day, after asking. */
function GoBack({ view, act }: { view: api.WaysView; act: Act }) {
  const [dayId, setDayId] = useState("");
  const [goingBack, setGoingBack] = useState<api.WayMoment | null>(null);
  if (view.moments.length <= 1) return null;
  const picked = dayId || view.moments[0].id;
  return (
    <Section title="Go back to an earlier day">
      <div className="ways-addrow">
        <span className="ways-word">Go back to how I worked on</span>
        <Select
          label="A day"
          value={picked}
          options={view.moments.map((m) => ({ value: m.id, label: `${shortDate(m.at)} · ${m.label}` }))}
          onChange={setDayId}
        />
        <span className="ways-addrow-actions">
          <button className="btn-secondary" onClick={() => setGoingBack(view.moments.find((m) => m.id === picked) ?? null)}>
            Go back
          </button>
        </span>
      </div>
      {goingBack && (
        <ConfirmDialog
          title={`Go back to how I worked on ${shortDate(goingBack.at)}?`}
          body="The rules, the styles and where each one starts go back to then. What I have now is kept first, so you can come back to it."
          confirmLabel="Go back"
          cancelLabel="Cancel"
          onCancel={() => setGoingBack(null)}
          onConfirm={() => {
            const day = goingBack;
            setGoingBack(null);
            void act(() => api.waysGoBack(day.id));
          }}
        />
      )}
    </Section>
  );
}

/** The ⋯ menu of one rule. Its own component so that it starts at the top each time
 * it opens. */
function WayMenu({
  way,
  view,
  base,
  on,
  close,
  edit,
  act,
  openChat,
}: {
  way: api.WayView;
  view: api.WaysView;
  base: boolean;
  on: boolean;
  close: () => void;
  edit: () => void;
  act: Act;
  openChat: (id: string) => Promise<void>;
}) {
  const [menu, setMenu] = useState<"root" | "scope">("root");
  const run = (fn: () => void) => () => {
    close();
    fn();
  };
  if (menu === "scope") {
    return (
      <>
        {view.scopes.map((s) => (
          <button
            className="row-menu-item"
            role="menuitem"
            key={s.value}
            onClick={run(() => void act(() => api.updateWay(way.slug, { scope: s.value })))}
          >
            <span className="row-menu-label">{s.label}</span>
            {s.value === way.scope && <span aria-label="Now">✓</span>}
          </button>
        ))}
      </>
    );
  }
  return (
    <>
      <button className="row-menu-item" role="menuitem" onClick={() => setMenu("scope")}>
        Only in…
      </button>
      {!base && <MenuItem onClick={run(edit)}>Change the wording</MenuItem>}
      {way.evidence.map((e) => (
        <button className="row-menu-item" role="menuitem" key={e.id} onClick={run(() => void openChat(e.id))}>
          <span className="row-menu-label">Where it came from: {e.title}</span>
        </button>
      ))}
      {way.status === "learned" && (
        <MenuItem onClick={run(() => void act(() => api.updateWay(way.slug, { status: "trying" })))}>Try it for a while</MenuItem>
      )}
      {way.status === "trying" && (
        <MenuItem onClick={run(() => void act(() => api.updateWay(way.slug, { status: "learned" })))}>Keep it</MenuItem>
      )}
      <MenuItem
        onClick={run(() => void act(() => api.updateWay(way.slug, { status: on ? "retired" : base ? "always" : "learned" })))}
      >
        {on ? "Switch off" : "Bring back"}
      </MenuItem>
    </>
  );
}
