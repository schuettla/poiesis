import { useEffect, useState } from "react";
import { inTauri, scanSkillText } from "../../lib/api";
import { useAppStore } from "../../lib/store";

/** `SKL-UI-2`: the install card's `TRU-1` risk line. Renders nothing for a
 * clean skill, which is nearly all of them — the line exists so a skill that
 * tries to redirect the agent says so before the user keeps it, not as a
 * standing decoration. Never blocks: SKL-4 leaves the decision with the user. */
function SkillRiskLine({ proposedText }: { proposedText: string }) {
  const [flags, setFlags] = useState<string[]>([]);

  useEffect(() => {
    if (!inTauri() || !proposedText) return;
    let live = true;
    scanSkillText(proposedText)
      .then((s) => {
        if (live && s.risk > 0) setFlags(s.flags);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [proposedText]);

  if (flags.length === 0) return null;
  return <p className="proposal-risk">◇ It reads like instructions to me: {flags.join(", ")}.</p>;
}

/** Fields parsed out of an `email` proposal's `proposed_text` — mirrors the
 * backend's `render_email_proposal`/`parse_email_proposal` in `agent/mail.rs`
 * exactly, so the card shows precisely what accepting will send. */
interface EmailFields {
  to: string;
  cc?: string;
  subject: string;
  body: string;
}

function parseEmailProposal(text: string): EmailFields | null {
  const split = text.indexOf("\n\n");
  if (split < 0) return null;
  const header = text.slice(0, split);
  const body = text.slice(split + 2);
  let to: string | undefined;
  let cc: string | undefined;
  let subject: string | undefined;
  for (const line of header.split("\n")) {
    const i = line.indexOf(": ");
    if (i < 0) continue;
    const key = line.slice(0, i);
    const value = line.slice(i + 2);
    if (key === "To") to = value;
    else if (key === "Cc") cc = value;
    else if (key === "Subject") subject = value;
  }
  if (to === undefined) return null;
  return { to, cc, subject: subject ?? "", body };
}

/** Rebuild the same header+body shape the backend parses, preserving To/Cc/
 * Subject/Account and swapping in the user's edited body. */
function rebuildEmailProposal(original: string, editedBody: string): string {
  const split = original.indexOf("\n\n");
  const header = split < 0 ? original : original.slice(0, split);
  return `${header}\n\n${editedBody}`;
}

function EmailProposal({ id, proposedText }: { id: string; proposedText: string }) {
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const updateText = useAppStore((s) => s.updateChangeProposalText);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const fields = parseEmailProposal(proposedText);
  if (!fields) return null;

  async function send() {
    if (editing) {
      await updateText(id, rebuildEmailProposal(proposedText, draft));
    }
    await resolve(id, true);
  }

  return (
    <div className="proposal-card wide">
      <p className="proposal-text">I'd like to send this on your behalf. I can't unsend it.</p>
      <div className="proposal-fields">
        <span className="proposal-field-label">To</span>
        <span className="proposal-field-value">{fields.to}</span>
        {fields.cc && (
          <>
            <span className="proposal-field-label">Cc</span>
            <span className="proposal-field-value">{fields.cc}</span>
          </>
        )}
        <span className="proposal-field-label">Subject</span>
        <span className="proposal-field-value">{fields.subject || "(no subject)"}</span>
      </div>
      {editing ? (
        <textarea
          className="proposal-edit-body"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          autoFocus
        />
      ) : (
        <p className="proposal-body-preview">{fields.body}</p>
      )}
      <div className="proposal-actions">
        <button className="btn-text" onClick={send}>
          Send
        </button>
        <button
          className="btn-text"
          onClick={() => {
            if (!editing) setDraft(fields.body);
            setEditing((v) => !v);
          }}
        >
          {editing ? "Preview" : "Edit"}
        </button>
        <button className="btn-text" onClick={() => resolve(id, false)}>
          Not now
        </button>
      </div>
    </div>
  );
}

/** `MCG-3`: a connected app's call that a run nobody was watching could not ask
 * about. Mirrors `render_app_proposal` in `agent/connector_gate.rs`: a few header
 * lines, a blank line, then the exact arguments it will run with. */
function parseAppProposal(text: string): { kind: string; summary: string; args: string } | null {
  const split = text.indexOf("\n\n");
  if (split < 0) return null;
  let kind = "";
  let summary = "";
  for (const line of text.slice(0, split).split("\n")) {
    const i = line.indexOf(": ");
    if (i < 0) continue;
    const key = line.slice(0, i);
    if (key === "Kind") kind = line.slice(i + 2);
    else if (key === "Summary") summary = line.slice(i + 2);
  }
  return summary ? { kind, summary, args: text.slice(split + 2) } : null;
}

function AppProposal({ id, proposedText }: { id: string; proposedText: string }) {
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const fields = parseAppProposal(proposedText);
  if (!fields) return null;
  const send = fields.kind === "send";
  return (
    <div className={`proposal-card wide ${send ? "outbound" : ""}`}>
      <p className="proposal-text">I couldn't ask you while I was working, so this is waiting. {fields.summary}</p>
      <pre className="proposal-preview">{fields.args}</pre>
      <div className="proposal-actions">
        <button className="btn-text" onClick={() => resolve(id, true)}>
          {send ? "Send it" : "Do it"}
        </button>
        <button className="btn-text" onClick={() => resolve(id, false)}>
          Not now
        </button>
      </div>
    </div>
  );
}

function SkillProposal({
  id,
  slug,
  rationale,
  proposedText,
}: {
  id: string;
  slug: string | null;
  rationale: string;
  proposedText: string;
}) {
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const setView = useAppStore((s) => s.setView);

  function review() {
    setView("skills");
  }

  return (
    <div className="proposal-card">
      <p className="proposal-text">
        I'd like to add the {slug ?? "untitled"} skill — {rationale}
      </p>
      <SkillRiskLine proposedText={proposedText} />
      <div className="proposal-actions">
        <button className="btn-text" onClick={review}>
          Review
        </button>
        <button className="btn-text" onClick={() => resolve(id, true)}>
          Keep it
        </button>
        <button className="btn-text" onClick={() => resolve(id, false)}>
          Not now
        </button>
      </div>
    </div>
  );
}

/** `SKC-4`: what `/init` ends in. The instructions are shown whole, because
 * these ride in every prompt of every session in the project — they are read
 * before they are accepted, not summarised. Accepting writes them through
 * `setProjectInstructions`, the one place that also keeps the project current. */
function ProjectInstructionsProposal({
  id,
  projectId,
  rationale,
  proposedText,
}: {
  id: string;
  projectId: string | null;
  rationale: string;
  proposedText: string;
}) {
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const setProjectInstructions = useAppStore((s) => s.setProjectInstructions);
  const projectName = useAppStore((s) => s.projects.find((p) => p.id === projectId)?.name);

  async function accept() {
    if (projectId) await setProjectInstructions(projectId, proposedText);
    await resolve(id, true);
  }

  return (
    <div className="proposal-card">
      <p className="proposal-text">
        I'd like to carry these instructions in every session{projectName ? ` of ${projectName}` : ""} —{" "}
        {rationale}
      </p>
      <pre className="proposal-preview">{proposedText}</pre>
      <div className="proposal-actions">
        <button className="btn-text" onClick={() => void accept()}>
          Use these
        </button>
        <button className="btn-text" onClick={() => resolve(id, false)}>
          Not now
        </button>
      </div>
    </div>
  );
}

/** `OUT-2`: a skill that's been failing tool calls asks to revise its own
 * copy. Its own `skill-revision` target, not a variant of the install card —
 * accepting means "replace what you use", not "add something new". */
function SkillRevisionProposal({ id, slug, rationale }: { id: string; slug: string | null; rationale: string }) {
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const setView = useAppStore((s) => s.setView);

  function review() {
    setView("skills");
  }

  return (
    <div className="proposal-card">
      <p className="proposal-text">
        {rationale} I'd like to revise my own copy — {slug ?? "untitled"}
      </p>
      <div className="proposal-actions">
        <button className="btn-text" onClick={review}>
          Review
        </button>
        <button className="btn-text" onClick={() => resolve(id, true)}>
          Keep it
        </button>
        <button className="btn-text" onClick={() => resolve(id, false)}>
          Not now
        </button>
      </div>
    </div>
  );
}

/** `RPT-2`: a lesson learned three times escalates to a standing-instruction
 * proposal — same `target: "soul"` and accept path as any other soul edit
 * (via `Review` → Personas' soul section), told apart by carrying a `slug`
 * (the lesson's name), which a plain soul proposal never does. */
function RecurrenceEscalationProposal({ id }: { id: string }) {
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const setView = useAppStore((s) => s.setView);

  function review() {
    setView("settings");
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        document
          .getElementById("settings-personas")
          ?.scrollIntoView({ behavior: "smooth", block: "start" })
      )
    );
  }

  return (
    <div className="proposal-card">
      <p className="proposal-text">
        I keep relearning this one — I'd like it to become a standing instruction.
      </p>
      <div className="proposal-actions">
        <button className="btn-text" onClick={review}>
          Review
        </button>
        <button className="btn-text" onClick={() => resolve(id, false)}>
          Not now
        </button>
      </div>
    </div>
  );
}

/** `MCPS-2`: something an agent app suggested keeping. The words are shown in
 * full before anything is saved, and saying yes writes them as a lesson. */
function EngineMemoryProposal({ id, rationale, proposedText }: { id: string; rationale: string; proposedText: string }) {
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const [error, setError] = useState<string | null>(null);

  async function keep() {
    setError(null);
    try {
      await resolve(id, true);
    } catch (err) {
      setError(String(err).replace(/^Error:\s*/, ""));
    }
  }

  return (
    <div className="proposal-card">
      <p className="proposal-text">{rationale} Keep this?</p>
      <pre className="proposal-preview">{proposedText}</pre>
      {error && <p className="proposal-risk">{error}</p>}
      <div className="proposal-actions">
        <button className="btn-text" onClick={() => void keep()}>
          Keep it
        </button>
        <button className="btn-text" onClick={() => resolve(id, false)}>
          Not now
        </button>
      </div>
    </div>
  );
}

/** What an `engine-rule` proposal would do, read back from its payload
 * (`engines/learn.rs`): `{ engine_id, action, rule_id?, kind?, text? }`. */
export interface EngineRuleChange {
  engine_id: string;
  action: "edit" | "add" | "off";
  rule_id?: string;
  kind?: "use" | "avoid";
  text?: string;
}

export function parseRuleChange(text: string): EngineRuleChange | null {
  try {
    const v = JSON.parse(text) as EngineRuleChange;
    return v && typeof v.engine_id === "string" && ["edit", "add", "off"].includes(v.action) ? v : null;
  } catch {
    return null;
  }
}

/** `RTE-6`: a change to the rules for when I hand work to an agent app. Shown at
 * the top of that agent's rules, and never applied without this click. */
function EngineRuleProposal({ id, rationale, proposedText }: { id: string; rationale: string; proposedText: string }) {
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const loadEngines = useAppStore((s) => s.loadEngines);
  const [error, setError] = useState<string | null>(null);
  const change = parseRuleChange(proposedText);
  if (!change) return null;
  const yes = change.action === "off" ? "Turn it off" : change.action === "edit" ? "Change it" : "Add it";

  async function accept() {
    setError(null);
    try {
      await resolve(id, true);
      await loadEngines();
    } catch (err) {
      setError(String(err).replace(/^Error:\s*/, ""));
    }
  }

  return (
    <div className="proposal-card" role="group" aria-label="Suggested change to a rule">
      <p className="proposal-text">{rationale}</p>
      {error && <p className="proposal-risk">{error}</p>}
      <div className="proposal-actions">
        <button className="btn-text" onClick={() => void accept()}>
          {yes}
        </button>
        <button className="btn-text" onClick={() => resolve(id, false)}>
          Not now
        </button>
      </div>
    </div>
  );
}

/**
 * A change the agent would like to make to itself, shown where it was raised
 * (SOUL-UI-2). Inline and quiet — the same visual register as a permission
 * request, never a modal. Nothing has changed when this appears; accepting
 * takes an explicit act. `email`, `skill`, `skill-revision` and the `soul`+
 * `slug` recurrence escalation get a dedicated shape (MAIL-UI-2, SKL-UI-2,
 * OUT-2, RPT-2); everything else gets the original generic card.
 */
export default function ProposalCard({ id }: { id: string }) {
  const proposal = useAppStore((s) => s.changeProposals.find((p) => p.id === id));
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const setView = useAppStore((s) => s.setView);

  // Answered elsewhere (or already resolved): the card is done.
  if (!proposal) return null;

  if (proposal.target === "email") {
    return <EmailProposal id={id} proposedText={proposal.proposed_text} />;
  }
  if (proposal.target === "app") {
    return <AppProposal id={id} proposedText={proposal.proposed_text} />;
  }
  if (proposal.target === "engine-memory") {
    return <EngineMemoryProposal id={id} rationale={proposal.rationale} proposedText={proposal.proposed_text} />;
  }
  if (proposal.target === "engine-rule") {
    return <EngineRuleProposal id={id} rationale={proposal.rationale} proposedText={proposal.proposed_text} />;
  }
  if (proposal.target === "project_instructions") {
    return (
      <ProjectInstructionsProposal
        id={id}
        projectId={proposal.slug}
        rationale={proposal.rationale}
        proposedText={proposal.proposed_text}
      />
    );
  }
  if (proposal.target === "skill-revision") {
    return <SkillRevisionProposal id={id} slug={proposal.slug} rationale={proposal.rationale} />;
  }
  // `SKL-5`: "recipe" is only ever a proposal that predates skills and was
  // never answered. Accepting it writes a skill, so it says so.
  if (proposal.target === "skill" || proposal.target === "recipe") {
    return (
      <SkillProposal
        id={id}
        slug={proposal.slug}
        rationale={proposal.rationale}
        proposedText={proposal.proposed_text}
      />
    );
  }
  // `RPT-2`: a soul proposal carrying a lesson slug is the recurrence
  // escalation, not an ordinary standing-instruction edit (which always has
  // `slug: null` — see `propose_soul_edit` in `agent/memory_skill.rs`).
  if (proposal.target === "soul" && proposal.slug) {
    return <RecurrenceEscalationProposal id={id} />;
  }

  const lead =
    proposal.target === "lesson-critic"
      ? "I nearly learned this, but I wasn't sure enough — should I keep it?"
      : proposal.target === "lesson"
        ? "Poiesis wants to remember a lesson"
        : "Poiesis suggests a standing instruction";

  function review() {
    setView("settings");
    // The section renders on the next frame; two frames is enough to be safe.
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        document
          .getElementById("settings-personas")
          ?.scrollIntoView({ behavior: "smooth", block: "start" })
      )
    );
  }

  return (
    <div className="proposal-card">
      <p className="proposal-text">
        {lead} — {proposal.rationale}
      </p>
      <div className="proposal-actions">
        <button className="btn-text" onClick={review}>
          Review
        </button>
        <button className="btn-text" onClick={() => resolve(id, false)}>
          Dismiss
        </button>
      </div>
    </div>
  );
}
