import { useState } from "react";

/**
 * Marks where the model stops seeing turns verbatim (CTX-UI-2). Everything
 * above it is still here and still yours — it just reaches the model as the
 * summary this divider reveals.
 */
export default function CompactDivider({ summary }: { summary: string }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="compact-divider-wrap">
      <button
        className="compact-divider"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        title="Show what the model sees in place of these turns"
      >
        · · · earlier turns are summarized for the model · · ·
      </button>
      {open && <pre className="compact-summary">{summary}</pre>}
    </div>
  );
}
