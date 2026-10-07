import { useEffect } from "react";
import type { CommandMenuModel } from "./useCommandInput";
import { useAppStore } from "../../lib/store";
import "./CommandMenu.css";

/**
 * The `/` menu (`CMP-4`): sections, recents, a reason on every row that can't
 * run, and a `◆` on the ones I can also do myself. It draws a model that
 * `useCommandInput` owns — nothing here decides what a key does.
 *
 * Class names keep the older `composer-menu` family so the drop-up sits and
 * animates exactly as the `+` menu always has.
 */
export default function CommandMenu({
  model,
  onHover,
  onChoose,
  onFilter,
}: {
  model: CommandMenuModel;
  onHover: (index: number) => void;
  onChoose: (index: number) => void;
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
          placeholder="Find a command"
          value={model.filter}
          onChange={(e) => onFilter(e.target.value)}
        />
      )}
      {model.lines.map((line) => {
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
        const active = line.index === model.index;
        if (line.kind === "choice") {
          return (
            <button
              className={`composer-menu-item command-menu-row ${active ? "active" : ""}`}
              role="option"
              id={`${model.listId}-opt-${line.index}`}
              aria-selected={active}
              key={line.key}
              onMouseDown={(e) => e.preventDefault()}
              onMouseEnter={() => onHover(line.index)}
              onClick={() => onChoose(line.index)}
            >
              <span className="mi-body">
                <span className="command-menu-line">
                  <span className="command-menu-choice">{line.choice.label}</span>
                </span>
                {line.choice.hint && <span className="mi-hint">{line.choice.hint}</span>}
              </span>
            </button>
          );
        }
        const v = line.row.view;
        const disabled = !!v.disabledReason;
        return (
          <button
            className={`composer-menu-item command-menu-row ${active ? "active" : ""} ${disabled ? "disabled" : ""}`}
            role="option"
            id={`${model.listId}-opt-${line.index}`}
            aria-selected={active}
            aria-disabled={disabled}
            key={line.key}
            // The input's blur would fire before a click lands and close the
            // menu out from under the pointer.
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => onHover(line.index)}
            onClick={() => onChoose(line.index)}
          >
            <span className="mi-body">
              <span className="command-menu-line">
                <span className="command-menu-name">{v.name}</span>
                {v.args.type !== "none" && v.args.hint && (
                  <span className="command-menu-arg">{v.args.hint}</span>
                )}
              </span>
              <span className="mi-hint">
                {disabled ? v.disabledReason : v.summary}
                {line.sectionHint && !disabled ? `  ·  ${line.sectionHint}` : ""}
              </span>
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
          </button>
        );
      })}
      <div className="command-menu-foot" role="presentation">
        <span>↑↓ move · ↵ run · tab complete · ⇧tab plan first · esc close</span>
        <span className="command-menu-foot-key">/help</span>
      </div>
      <LiveRegion text={model.announcement} />
    </div>
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
