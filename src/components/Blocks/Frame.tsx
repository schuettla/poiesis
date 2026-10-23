import type { ReactNode } from "react";

/** Shared frame: mono header (title + kind) and an optional footer summary. */
export default function Frame({
  title,
  kind,
  footer,
  children,
}: {
  title: string;
  kind: string;
  footer?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="block" aria-label={`${title} — ${kind} block`}>
      <div className="block-head">
        <span className="block-title">{title}</span>
        <span className="block-kind">{kind}</span>
      </div>
      <div className="block-body">{children}</div>
      {footer != null && <div className="block-foot">{footer}</div>}
    </section>
  );
}
