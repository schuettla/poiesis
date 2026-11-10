import { Fragment, useEffect } from "react";
import type { MentionMenuModel } from "./useMentionInput";
import "./CommandMenu.css";

/**
 * The `@` menu: files from the working folder, then things from the library. It
 * draws a model that `useMentionInput` owns, in the same dialog the `/` menu
 * uses, so it sits and animates the same way.
 */
export default function MentionMenu({
  model,
  onHover,
  onChoose,
}: {
  model: MentionMenuModel;
  onHover: (index: number) => void;
  onChoose: (index: number) => void;
}) {
  // Keep the highlighted row in view while arrowing through the list.
  useEffect(() => {
    document.getElementById(`${model.listId}-opt-${model.index}`)?.scrollIntoView?.({ block: "nearest" });
  }, [model.index, model.listId]);

  if (!model.open) return null;

  const files = model.items.filter((i) => i.kind === "file");
  const library = model.items.filter((i) => i.kind === "library");
  const empty = model.items.length === 0;

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

  return (
    <div
      className="composer-menu composer-slash-menu command-menu mention-menu"
      role="listbox"
      id={model.listId}
      aria-label="Files and library"
    >
      <div className="command-menu-body">
        {files.length > 0 && (
          <div className="command-menu-header" role="presentation">
            In this folder
          </div>
        )}
        {model.items.map((item, index) => {
          const header =
            item.kind === "library" && item === library[0] ? (
              <div className="command-menu-header" role="presentation">
                From my library
              </div>
            ) : null;
          const active = index === model.index;
          if (item.kind === "file") {
            const at = item.path.lastIndexOf("/");
            const name = item.path.slice(at + 1);
            const dir = at < 0 ? "" : item.path.slice(0, at);
            return (
              <div
                className={`composer-menu-item command-menu-row ${active ? "active" : ""}`}
                key={item.key}
                {...optionProps(index)}
              >
                <span className="mi-body">
                  <span className="command-menu-title">{name}</span>
                </span>
                {dir && <span className="command-menu-hint">{dir}</span>}
              </div>
            );
          }
          return (
            <Fragment key={item.key}>
              {header}
              <div
                className={`composer-menu-item command-menu-row ${active ? "active" : ""}`}
                {...optionProps(index)}
              >
                <span className="mi-body">
                  <span className="command-menu-title">{item.artifact.title}</span>
                </span>
                <span className="command-menu-hint">{item.artifact.kind}</span>
              </div>
            </Fragment>
          );
        })}
        {empty && (
          <div className="command-menu-note" role="presentation">
            {model.query
              ? `Nothing matches "${model.query}"`
              : model.hasFolder
                ? "This folder has no files I can list"
                : "Attach a folder with + to point at its files"}
          </div>
        )}
      </div>
      <div className="command-menu-foot" role="presentation">
        <span>↑↓ to move · Enter to choose · Esc to close</span>
      </div>
    </div>
  );
}
