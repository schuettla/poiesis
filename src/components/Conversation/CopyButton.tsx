import { useEffect, useState } from "react";
import { sayRefused } from "../../lib/store";
import { CheckIcon, CopyIcon } from "../Icons/Icons";

/** Takes a message's text, and says "Copied" for a moment. Used under an answer
 * and on your own message. */
export default function CopyButton({ text, tip }: { text: string; tip: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);
  return (
    <button
      className="turn-btn"
      aria-label="Copy"
      data-tip={copied ? "Copied" : tip}
      onClick={() => void navigator.clipboard.writeText(text).then(() => setCopied(true), sayRefused)}
    >
      {copied ? <CheckIcon size={14} strokeWidth={1.8} /> : <CopyIcon size={14} strokeWidth={1.5} />}
    </button>
  );
}
