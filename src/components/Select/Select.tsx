import { useEffect, useId, useRef, useState } from "react";
import { ChevronIcon } from "../Icons/Icons";
import "./Select.css";

export interface SelectOption {
  value: string;
  label: string;
  /** A quieter second line or trailing note, such as a count. */
  hint?: string;
}

/**
 * A dropdown in the app's own look, in place of the operating system's
 * combobox (see `EffortPicker` for why). A button and a list: arrow keys move,
 * Enter picks, Escape or a click elsewhere closes. It opens downward; a list
 * longer than about ten rows scrolls.
 */
export default function Select({
  value,
  options,
  onChange,
  label,
  id,
}: {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  /** Read out by screen readers; there is no visible label on the control itself. */
  label: string;
  id?: string;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const listId = useId();

  const index = Math.max(0, options.findIndex((o) => o.value === value));
  const current = options[index];

  useEffect(() => {
    if (!open) return;
    function onDoc(e: MouseEvent) {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  // Keep the row under the keyboard in view.
  useEffect(() => {
    if (!open) return;
    list.current?.querySelector<HTMLElement>(`[data-i="${active}"]`)?.scrollIntoView?.({ block: "nearest" });
  }, [open, active]);

  function openList() {
    setActive(index);
    setOpen(true);
  }

  function pick(i: number) {
    const option = options[i];
    setOpen(false);
    if (option && option.value !== value) onChange(option.value);
    root.current?.querySelector("button")?.focus();
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape" && open) {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) return openList();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setActive((a) => (a + step + options.length) % options.length);
    } else if ((e.key === "Enter" || e.key === " ") && open) {
      e.preventDefault();
      pick(active);
    }
  }

  return (
    <div className="select" ref={root} onKeyDown={onKeyDown}>
      <button
        id={id}
        type="button"
        className="select-trigger"
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        aria-label={label}
        onClick={() => (open ? setOpen(false) : openList())}
      >
        <span className="select-value">{current?.label ?? ""}</span>
        <span className="select-caret" aria-hidden="true">
          <ChevronIcon dir="down" size={12} strokeWidth={1.8} />
        </span>
      </button>
      {open && (
        <div className="select-list" role="listbox" id={listId} aria-label={label} ref={list}>
          {options.map((o, i) => (
            <div
              key={o.value}
              data-i={i}
              role="option"
              aria-selected={o.value === value}
              className={`select-option${o.value === value ? " selected" : ""}${i === active ? " active" : ""}`}
              onMouseEnter={() => setActive(i)}
              onClick={() => pick(i)}
            >
              <span className="select-option-label">{o.label}</span>
              {o.hint && <span className="select-option-hint">{o.hint}</span>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
