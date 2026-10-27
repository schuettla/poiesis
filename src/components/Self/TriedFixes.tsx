import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import { formLabel, replaysOf } from "../../lib/learning";
import "../Memory/Memory.css";
import { useAppStore } from "../../lib/store";

/**
 * `IMP-UI-1`, IV.8: "I tried 3 fixes". One block on the card of a fix that was tried
 * on replayed chats: collapsed to its first line, it lists each fix with how it did.
 * The fix I am proposing is the one still waiting; the others are shown as tried.
 * "Show replays" lists the chats they were tried on.
 */
export default function TriedFixes({ proposal }: { proposal: api.ChangeProposal }) {
  const [group, setGroup] = useState<api.ChangeProposal[]>([]);
  const [open, setOpen] = useState(false);
  const [replays, setReplays] = useState(false);
  const conversations = useAppStore((s) => s.conversations);
  const setActiveConversation = useAppStore((s) => s.setActiveConversation);
  const setView = useAppStore((s) => s.setView);

  useEffect(() => {
    if (!proposal.group_id || !api.inTauri()) return;
    api.improvementGroup(proposal.group_id).then(setGroup).catch(() => setGroup([]));
  }, [proposal.group_id]);

  const tried = group.filter((r) => r.status !== "kept_base");
  const mine = replaysOf(proposal.rehearsal_json);
  if (!proposal.group_id) return null;
  if (!mine.rehearsed) {
    // Nothing to replay: say so in one line and show no numbers.
    return <p className="tried-fixes-head">I had no chat to try this on, so it is untried.</p>;
  }
  const head = `${proposal.problem_cue ?? "This"} came up again. I tried ${tried.length} ${tried.length === 1 ? "fix" : "fixes"} on the chats where it happened.`;
  return (
    <div className="tried-fixes" aria-label="Fixes I tried">
      <button className="btn-text tried-fixes-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        {head}
      </button>
      {open && (
        <>
          <ul>
            {tried.map((r) => {
              const x = replaysOf(r.rehearsal_json);
              return (
                <li key={r.id}>
                  {formLabel(r.form)}: right in {x.right} of {x.tries} replays
                </li>
              );
            })}
            <li>the setup as it is: right in {mine.baseRight} of {mine.tries} replays</li>
          </ul>
          <button className="btn-text" onClick={() => setReplays(!replays)}>
            {replays ? "Hide replays" : "Show replays"}
          </button>
          {replays && (
            <ul className="tried-fixes-replays">
              {mine.conversations.map((id) => {
                const c = conversations.find((x) => x.id === id);
                return (
                  <li key={id}>
                    {c ? (
                      <button
                        className="btn-text"
                        onClick={() => {
                          setActiveConversation(id);
                          setView("chat");
                        }}
                      >
                        {c.title || "a chat"}
                      </button>
                    ) : (
                      "a chat that is gone"
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </>
      )}
    </div>
  );
}
