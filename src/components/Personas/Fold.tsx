import { useState, type ReactNode } from "react";
import { ChevronIcon } from "../Icons/Icons";

/**
 * One folded section of a persona, of Poiesis or of a team: a line until it is opened. Several
 * may be open at once (`useFolds`), like the groups of an agent app in External agents.
 */
export default function Fold({
  title,
  open,
  onToggle,
  children,
}: {
  title: string;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <section className={`persona-fold${open ? " open" : ""}`}>
      <button className="persona-fold-head" aria-expanded={open} onClick={onToggle}>
        <ChevronIcon dir={open ? "down" : "right"} size={11} strokeWidth={1.6} />
        <span>{title}</span>
      </button>
      {open && <div className="persona-fold-body">{children}</div>}
    </section>
  );
}

/** Which folds are open: `is(id)`, `toggle(id)`, and `reset()` when someone else is shown. */
export function useFolds(first: string[] = []) {
  const [open, setOpen] = useState<Set<string>>(new Set(first));
  return {
    is: (id: string) => open.has(id),
    toggle: (id: string) =>
      setOpen((now) => {
        const next = new Set(now);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      }),
    reset: () => setOpen(new Set(first)),
  };
}
