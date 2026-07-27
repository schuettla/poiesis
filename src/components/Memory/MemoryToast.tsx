import { useEffect, useState } from "react";
import { useAppStore } from "../../lib/store";
import { setSetting } from "../../lib/api";
import "./Memory.css";

/** Auto-dismiss delay. Long enough to read and undo, short enough to stay quiet. */
const DWELL_MS = 6000;
const ONBOARDED_KEY = "memory.onboarded";

/** Past-tense receipt matching the op, so the toast never claims the wrong thing. */
function leadFor(op: string, collection: string): string {
  const lesson = collection === "lessons";
  switch (op) {
    case "forget":
      return lesson ? "I forgot a lesson" : "I forgot that";
    case "update":
      return lesson ? "I revised a lesson" : "I updated that";
    default:
      return lesson ? "I learned something" : "I'll remember that";
  }
}

/** The watchdog speaking for itself (HEAL-1). Same quiet shell as a memory
 * write, different glyph — self-repair is the same kind of event as
 * self-writing: something the organism did, said plainly, with no alarm. */
function HealToast() {
  const message = useAppStore((s) => s.healToast);
  const dismiss = useAppStore((s) => s.dismissHealToast);

  useEffect(() => {
    if (!message) return;
    const t = setTimeout(dismiss, DWELL_MS);
    return () => clearTimeout(t);
  }, [message, dismiss]);

  if (!message) return null;
  return (
    <div className="memory-toast" role="status">
      <div className="memory-toast-line">
        <span className="memory-toast-text">{message}</span>
      </div>
    </div>
  );
}

/**
 * A quiet, undoable notice that the agent just wrote to its durable self
 * (MEM-UI-3). Every self-write is visible — this is that promise, kept at the
 * moment it happens rather than buried in a log.
 */
export default function MemoryToast() {
  const toast = useAppStore((s) => s.memoryToast);
  const dismiss = useAppStore((s) => s.dismissMemoryToast);
  const undo = useAppStore((s) => s.undoMemoryWrite);
  const onboarded = useAppStore((s) => s.memoryOnboarded);
  // Latched per toast: marking the flag immediately would hide the explainer
  // on the very toast that's supposed to carry it.
  const [explain, setExplain] = useState(false);

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(dismiss, DWELL_MS);
    return () => clearTimeout(t);
  }, [toast, dismiss]);

  // The first time the agent ever writes, explain what just happened (MEM-UI-4).
  useEffect(() => {
    if (!toast) return;
    if (onboarded) {
      setExplain(false);
      return;
    }
    setExplain(true);
    useAppStore.setState({ memoryOnboarded: true });
    setSetting(ONBOARDED_KEY, "true").catch(() => {});
  }, [toast, onboarded]);

  if (!toast) return <HealToast />;

  const lead = leadFor(toast.op, toast.collection);
  // A save is undone by forgetting, a forget by restoring — both clean. An
  // update has no retained prior text to revert to, so it's a receipt only.
  const canUndo = toast.op === "save" || toast.op === "forget";

  return (
    <div className="memory-toast" role="status">
      <div className="memory-toast-line">
        <span className="memory-toast-mark" aria-hidden="true">
          ◆
        </span>
        <span className="memory-toast-text">
          {lead}: {toast.description || toast.name}
        </span>
        {canUndo && (
          <button className="memory-toast-undo" onClick={() => undo()}>
            Undo
          </button>
        )}
      </div>
      {explain && (
        <p className="memory-toast-explain">
          I keep a few markdown notes about your preferences on this device. You can
          review them — or stop me — in my Self panel.
        </p>
      )}
    </div>
  );
}
