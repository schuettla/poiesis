import { useCallback, useEffect, useState } from "react";
import {
  MAIL_INBOX_EVENT,
  inTauri,
  listenAppEvent,
  mailInbox,
  mailRetag,
  mailSyncNow,
  type InboxRow,
  type InboxView,
  type MailAccount,
} from "../../lib/api";
import { useAppStore } from "../../lib/store";
import { RefreshIcon } from "../Icons/Icons";
import Select from "../Select/Select";
import {
  countRows,
  filterRows,
  kindHint,
  replyHint,
  statusLine,
  urgencyHint,
  whenText,
  type InboxFilter,
} from "./inbox";
import "./MailInbox.css";

const clean = (e: unknown) => String(e).replace(/^Error:\s*/, "");
const rowKey = (r: Pick<InboxRow, "account_id" | "uid">) => `${r.account_id}:${r.uid}`;

/** Four bars, as many filled as the message is urgent. Not drawn at all when no
 * urgency was judged: a row that was not sure shows nothing, never a guess. */
function UrgencyMeter({ row }: { row: InboxRow }) {
  const hint = urgencyHint(row);
  if (row.urgency === null || hint === null) return <span className="mail-urgency empty" aria-hidden="true" />;
  return (
    <span className="mail-urgency" role="img" aria-label={`Urgency ${row.urgency} of 4`} title={hint}>
      {[1, 2, 3, 4].map((step) => (
        <span key={step} className={`mail-urgency-bar s${step} ${step <= row.urgency! ? "on" : ""}`} />
      ))}
    </span>
  );
}

/** The kind of a message, and the menu that changes it. The change is a label the
 * calibration reads, so a wrong tag corrected here makes the next one better. */
function KindTag({
  row,
  kinds,
  open,
  onOpen,
  onClose,
  onPick,
}: {
  row: InboxRow;
  kinds: string[];
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
  onPick: (kind: string | null) => void;
}) {
  // `unsure` is always allowed: it says "none of mine", where "No kind" says "none".
  const options = [...kinds, "unsure"];
  return (
    <span className="mail-kind-wrap">
      {row.category ? (
        <button
          className={`mail-kind ${row.retagged ? "mine" : ""}`}
          aria-haspopup="menu"
          aria-expanded={open}
          title={kindHint(row)}
          onClick={onOpen}
        >
          {row.category}
        </button>
      ) : (
        <button className="mail-kind none" aria-haspopup="menu" aria-expanded={open} onClick={onOpen}>
          Set kind
        </button>
      )}
      {open && (
        <>
          <div className="row-menu-backdrop" onClick={onClose} />
          <div className="row-menu mail-menu" role="menu" aria-label="Kind of this message">
            {options.map((kind) => (
              <button
                key={kind}
                className="row-menu-item"
                role="menuitemradio"
                aria-checked={row.category === kind}
                onClick={() => onPick(kind)}
              >
                <span className="row-menu-label">{kind}</span>
                {row.category === kind && <span aria-hidden="true">✓</span>}
              </button>
            ))}
            {row.category && (
              <button className="row-menu-item" role="menuitem" onClick={() => onPick(null)}>
                <span className="row-menu-label">No kind</span>
              </button>
            )}
          </div>
        </>
      )}
    </span>
  );
}

/**
 * `USE-UI-4`: the Inbox, a small list of the newest mail of each account with what
 * the check said about it. It shows and never acts: nothing here moves, answers or
 * deletes a message. A row opens nothing; it offers to start a chat about it.
 */
export default function MailInbox({
  accounts,
  onGoToAccounts,
}: {
  accounts: MailAccount[];
  onGoToAccounts: () => void;
}) {
  const [accountId, setAccountId] = useState("");
  const [filter, setFilter] = useState<InboxFilter>("all");
  const [view, setView] = useState<InboxView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [looking, setLooking] = useState(false);
  const [menu, setMenu] = useState<string | null>(null);
  const setViewTo = useAppStore((s) => s.setView);
  const askAboutMail = useAppStore((s) => s.askAboutMail);

  const hasAccount = accounts.some((a) => a.enabled);

  const load = useCallback(async () => {
    if (!inTauri()) return;
    try {
      setView(await mailInbox(accountId || null));
    } catch (e) {
      setError(clean(e));
    }
  }, [accountId]);

  useEffect(() => {
    if (!hasAccount) return;
    void load();
    // A look that found mail, or a check that wrote a tag, says so.
    return listenAppEvent(MAIL_INBOX_EVENT, () => void load());
  }, [load, hasAccount]);

  async function lookNow() {
    setLooking(true);
    setError(null);
    try {
      await mailSyncNow();
      await load();
    } catch (e) {
      setError(clean(e));
    } finally {
      setLooking(false);
    }
  }

  async function retag(row: InboxRow, kind: string | null) {
    setMenu(null);
    setError(null);
    try {
      const changed = await mailRetag(row.account_id, row.uid, kind);
      setView((v) => (v ? { ...v, rows: v.rows.map((r) => (rowKey(r) === rowKey(changed) ? changed : r)) } : v));
    } catch (e) {
      setError(clean(e));
    }
  }

  if (!hasAccount) {
    return (
      <p className="placeholder-note">
        Add a mail account first.{" "}
        <button className="link-button inline" onClick={onGoToAccounts}>
          Go to Accounts
        </button>
      </p>
    );
  }

  const rows = view?.rows ?? [];
  const counts = countRows(rows);
  const shown = filterRows(rows, filter);
  const now = Date.now();
  const filters: { id: InboxFilter; label: string; count: number | null }[] = [
    { id: "all", label: "All", count: null },
    { id: "reply", label: "Needs reply", count: counts.reply },
    { id: "urgent", label: "Urgent", count: counts.urgent },
  ];

  return (
    <div role="tabpanel" aria-label="Inbox">
      <div className="mail-head">
        <div className="transport-toggle" role="group" aria-label="Show">
          {filters.map((f) => (
            <button
              key={f.id}
              className={`seg ${filter === f.id ? "on" : ""}`}
              aria-pressed={filter === f.id}
              onClick={() => setFilter(f.id)}
            >
              {f.label}
              {f.count !== null && f.count > 0 && <span className="mail-count">{f.count}</span>}
            </button>
          ))}
        </div>
        {accounts.length > 1 && (
          <Select
            label="Account"
            value={accountId}
            onChange={setAccountId}
            options={[
              { value: "", label: "All accounts" },
              ...accounts.map((a) => ({ value: a.id, label: a.label })),
            ]}
          />
        )}
      </div>

      <p className="mail-status">
        {statusLine(view?.last ?? null, view?.every_minutes ?? 15, now)}
        <button
          className="mail-refresh"
          onClick={lookNow}
          disabled={looking || view?.syncing === true}
          aria-label="Check for new mail now"
          title="Check for new mail now"
        >
          <RefreshIcon />
        </button>
      </p>
      {view?.last?.errors.map((line) => (
        <p key={line} className="hw-note error">
          {line}
        </p>
      ))}
      {view && !view.decision_model && (
        <p className="hw-note">
          Mail triage needs the decision model on this computer, so nothing gets a tag yet.{" "}
          <button className="link-button inline" onClick={() => setViewTo("models")}>
            Set it up in Models
          </button>
        </p>
      )}
      {error && <p className="hw-note error">{error}</p>}

      {view && rows.length === 0 ? (
        <p className="placeholder-note">
          Nothing fetched yet. The first look comes a minute or two after Poiesis starts, or you can look now.
        </p>
      ) : (
        <>
          <p className="mail-total" aria-live="polite">
            {shown.length === rows.length
              ? `${rows.length} ${rows.length === 1 ? "message" : "messages"}`
              : `${shown.length} of ${rows.length} messages`}
          </p>
          <ul className="mail-list" aria-label="Inbox messages">
            {shown.map((row) => {
              const key = rowKey(row);
              const account = accounts.length > 1 ? row.account_label : undefined;
              return (
                <li key={key} className={`mail-row ${row.unread ? "unread" : ""}`}>
                  <UrgencyMeter row={row} />
                  <span className="mail-from">{row.from || "(unknown sender)"}</span>
                  <span className="mail-subject">
                    <span className="mail-subject-text">{row.subject || "(no subject)"}</span>
                    {row.reply !== null && (
                      <span className="mail-reply" title={replyHint(row)}>
                        Reply
                      </span>
                    )}
                  </span>
                  <KindTag
                    row={row}
                    kinds={view?.categories ?? []}
                    open={menu === key}
                    onOpen={() => setMenu(menu === key ? null : key)}
                    onClose={() => setMenu(null)}
                    onPick={(kind) => void retag(row, kind)}
                  />
                  <time className="mail-when">{whenText(row.sent_at, now)}</time>
                  <button
                    className="btn-text mail-ask"
                    onClick={() => void askAboutMail(row.from, row.subject, account)}
                  >
                    Ask about this
                  </button>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
}
