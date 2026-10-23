import type { ReactNode } from "react";

/** One line of an agent's settings: what it is on the left, the control on the
 * right. A `label` when the whole line is the control (a switch), a `div` when
 * it holds buttons or a menu. Styled in `routes/Engines.css`. */
export default function Opt({
  title,
  help,
  children,
  as: Tag = "div",
  className = "",
  hint,
}: {
  title: string;
  help?: ReactNode;
  children: ReactNode;
  as?: "div" | "label";
  className?: string;
  /** A tooltip for the whole line. */
  hint?: string;
}) {
  return (
    <Tag className={`engines-opt ${className}`} title={hint}>
      <span className="engines-opt-text">
        <span className="engines-opt-title">{title}</span>
        {help && <span className="engines-opt-help">{help}</span>}
      </span>
      <span className="engines-opt-control">{children}</span>
    </Tag>
  );
}
