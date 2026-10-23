import { useEffect } from "react";
import type { CommandMenuModel, MenuLine } from "./useCommandInput";
import type { LiveControl } from "../../lib/commands";
import { useAppStore } from "../../lib/store";
import "./CommandMenu.css";

type RowLine = Extract<MenuLine, { kind: "command" | "recent" }>;

/**
 * The `/` menu (`CMP-4`), as a dialog the width of the box it opens from.
 *
 * The first page is a short list of groups; a group opens its commands in
 * place. A command that is really a setting carries it on the row: a switch you
 * flip, a few choices you tap, or the current pick, which opens its list. All of
 * it draws a model that `useCommandInput` owns — nothing here decides what a
 * key or a click does.
 *
 * Class names keep the older `composer-menu` family so the drop-up sits and
 * animates exactly as the `+` menu always has.
 */
export default function CommandMenu({
  model,
  onHover,
  onChoose,
  onControl,
  onBack,
  onFilter,
}: {
  model: CommandMenuModel;
  onHover: (index: number) => void;
  onChoose: (index: number) => void;
  /** A switch flipped, or a choice tapped on a row. */
  onControl: (index: number, value?: string) => void;
  onBack: () => void;
  /** The popover's own filter field; absent when the input itself is the query. */
  onFilter?: (text: string) => void;
}) {
  const openSelf = useAppStore((s) => s.openSelf);

  // Keep the highlighted row in view while arrowing through a long list.
  useEffect(() => {
    document
      .getElementById(`${model.listId}-opt-${model.index}`)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [model.index, model.listId]);

  if (!model.open) {
    return <LiveRegion text={model.announcement} />;
  }

  const optionProps = (index: number) => ({
    role: "option" as const,
    id: `${model.listId}-opt-${index}`,
    "aria-selected": index === model.index,
    // The input's blur would fire before a click lands and close the menu out
    // from under the pointer.
    onMouseDown: (e: React.MouseEvent) => e.preventDefault(),
    onMouseEnter: () => onHover(index),
    onClick: () => onChoose(index),
  });

  // The recent buttons sit together on one strip, wherever the first of them is.
  const blocks: Array<MenuLine | { kind: "recents"; key: string; items: RowLine[] }> = [];
  for (const line of model.lines) {
    const last = blocks[blocks.length - 1];
    if (line.kind === "recent" && last?.kind === "recents") last.items.push(line);
    else if (line.kind === "recent") blocks.push({ kind: "recents", key: "recents", items: [line] });
    else blocks.push(line);
  }

  return (
    <div
      className={`composer-menu composer-slash-menu command-menu ${model.popover ? "popover" : ""}`}
      role="listbox"
      id={model.listId}
      aria-label="Commands"
    >
      {model.popover && onFilter && (
        <input
          className="command-menu-filter"
          autoFocus
          aria-label="Filter commands"
          placeholder="Find something to do"
          value={model.filter}
          onChange={(e) => onFilter(e.target.value)}
        />
      )}
      {model.panel && (
        <button
          className="command-menu-back"
          type="button"
          aria-label={`Back from ${model.panel.title}`}
          onMouseDown={(e) => e.preventDefault()}
          onClick={onBack}
        >
          <span aria-hidden="true">‹</span>
          {model.panel.title}
        </button>
      )}
      <div className="command-menu-body">
        {blocks.map((line) => {
          if (line.kind === "header") {
            return (
              <div className="command-menu-header" role="presentation" key={line.key}>
                {line.label}
              </div>
            );
          }
          if (line.kind === "note") {
            return (
              <div className="command-menu-note" role="presentation" key={line.key}>
                {line.text}
              </div>
            );
          }
          if (line.kind === "recents") {
            return (
              <div className="command-menu-recents" role="presentation" key={line.key}>
                <span className="command-menu-recents-label">Recent</span>
                {line.items.map((item) => {
                  const v = item.row.view;
                  const chipActive = item.index === model.index;
                  return (
                    <button
                      type="button"
                      className={`command-menu-chip ${chipActive ? "active" : ""} ${v.disabledReason ? "disabled" : ""}`}
                      data-command={v.name}
                      key={item.key}
                      title={v.disabledReason ?? v.summary}
                      {...optionProps(item.index)}
                    >
                      {v.title}
                      {item.control?.kind === "switch" && (
                        <span className="command-menu-chip-state">{item.control.on ? "on" : "off"}</span>
                      )}
                    </button>
                  );
                })}
              </div>
            );
          }
          const active = line.index === model.index;
          if (line.kind === "category") {
            return (
              <div
                className={`composer-menu-item command-menu-row command-menu-category ${active ? "active" : ""}`}
                data-section={line.section}
                key={line.key}
                {...optionProps(line.index)}
              >
                <span className="mi-body">
                  <span className="command-menu-title">{line.label}</span>
                </span>
                <span className="command-menu-hint">{line.hint}</span>
                <span className="command-menu-chev" aria-hidden="true">›</span>
              </div>
            );
          }
          if (line.kind === "choice") {
            const off = !!line.choice.disabledReason;
            return (
              <div
                className={`composer-menu-item command-menu-row ${active ? "active" : ""} ${off ? "disabled" : ""}`}
                aria-disabled={off || undefined}
                key={line.key}
                {...optionProps(line.index)}
              >
                <span className="mi-body">
                  <span className="command-menu-title command-menu-choice">{line.choice.label}</span>
                  {(line.choice.disabledReason ?? line.choice.hint) && (
                    <span className="mi-hint">{line.choice.disabledReason ?? line.choice.hint}</span>
                  )}
                </span>
                {line.checked && (
                  <span className="command-menu-check" aria-label="Chosen">
                    ✓
                  </span>
                )}
              </div>
            );
          }
          const v = line.row.view;
          const disabled = !!v.disabledReason;
          return (
            <div
              className={`composer-menu-item command-menu-row ${active ? "active" : ""} ${disabled ? "disabled" : ""}`}
              aria-disabled={disabled}
              data-command={v.name}
              key={line.key}
              {...optionProps(line.index)}
            >
              <span className="mi-body">
                {/* The "I can do this on my own" mark sits right after the name. */}
                <span className="command-menu-name-line">
                  <span className="command-menu-title">
                    <Marked text={v.title} query={line.kind === "command" ? line.query : undefined} />
                  </span>
                  {v.who === "both" && (
                    <span
                      className="command-menu-glyph"
                      role="button"
                      tabIndex={-1}
                      title="I can do this on my own as well — you decide how much in my Self panel"
                      aria-label="I can do this on my own as well"
                      onMouseDown={(e) => e.stopPropagation()}
                      onClick={(e) => {
                        e.stopPropagation();
                        openSelf("autonomy", v.agentClass);
                      }}
                    >
                      ◆
                    </span>
                  )}
                  {/* While typing, the name to type, so a title that has drifted
                      from it never hides what the box is matching. */}
                  {line.kind === "command" && line.query !== undefined && v.title !== v.name && (
                    <span className="command-menu-cmd">
                      /<Marked text={v.name} query={line.query} />
                    </span>
                  )}
                </span>
                <span className="mi-hint">{disabled ? v.disabledReason : v.summary}</span>
              </span>
              {line.kind === "command" && line.sectionHint && !disabled && (
                <span className="command-menu-hint">{line.sectionHint}</span>
              )}
              {line.control && (
                <Control
                  control={line.control}
                  label={v.title}
                  onChange={(value) => onControl(line.index, value)}
                />
              )}
            </div>
          );
        })}
      </div>
      <div className="command-menu-foot" role="presentation">
        <span>↑↓ to move · Enter to choose · Esc {model.panel ? "to go back" : "to close"}</span>
      </div>
      <LiveRegion text={model.announcement} />
    </div>
  );
}

/** Where `query` appears in `text`, the way the ranking finds it: as a run,
 * wherever it sits. Character indexes; `null` when it does not appear (a match
 * through the summary or an alias marks nothing). */
function matchedChars(text: string, query: string): Set<number> | null {
  const q = query.trim().toLowerCase();
  if (!q) return null;
  const at = text.toLowerCase().indexOf(q);
  return at < 0 ? null : new Set(Array.from({ length: q.length }, (_, i) => at + i));
}

/** `text` with the part that matched what was typed marked. */
function Marked({ text, query }: { text: string; query?: string }) {
  const hit = query ? matchedChars(text, query) : null;
  if (!hit) return <>{text}</>;
  const parts: Array<{ text: string; on: boolean }> = [];
  [...text].forEach((ch, i) => {
    const on = hit.has(i);
    const last = parts[parts.length - 1];
    if (last && last.on === on) last.text += ch;
    else parts.push({ text: ch, on });
  });
  return (
    <>
      {parts.map((p, i) =>
        p.on ? (
          <mark className="command-menu-match" key={i}>
            {p.text}
          </mark>
        ) : (
          p.text
        )
      )}
    </>
  );
}

/** The setting on a row. A tap here is the row's own business, not "run this". */
function Control({
  control,
  label,
  onChange,
}: {
  control: LiveControl;
  label: string;
  onChange: (value?: string) => void;
}) {
  // Taps must not bubble to the row (which would also "choose" it), and must not
  // pull focus out of the message box.
  const own = {
    onMouseDown: (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
    },
  };
  if (control.kind === "switch") {
    return (
      <button
        type="button"
        role="switch"
        aria-checked={control.on}
        aria-label={label}
        tabIndex={-1}
        className={`command-menu-switch ${control.on ? "on" : ""}`}
        {...own}
        onClick={(e) => {
          e.stopPropagation();
          onChange();
        }}
      >
        <span className="command-menu-knob" />
      </button>
    );
  }
  if (control.kind === "segments") {
    return (
      <span className="command-menu-segments" role="radiogroup" aria-label={label}>
        {control.options.map((o) => (
          <button
            type="button"
            role="radio"
            aria-checked={o.value === control.value}
            tabIndex={-1}
            key={o.value}
            title={o.hint}
            className={`command-menu-seg ${o.value === control.value ? "on" : ""}`}
            {...own}
            onClick={(e) => {
              e.stopPropagation();
              onChange(o.value);
            }}
          >
            {o.label}
          </button>
        ))}
      </span>
    );
  }
  return (
    <span className="command-menu-value">
      {control.current}
      <span className="command-menu-chev" aria-hidden="true">›</span>
    </span>
  );
}

/** What Enter on a disabled row says, out loud. */
function LiveRegion({ text }: { text: string }) {
  return (
    <div className="command-menu-live" role="status" aria-live="polite">
      {text}
    </div>
  );
}
