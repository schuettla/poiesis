import { useEffect, useMemo, useRef, useState } from "react";
import {
  accountWaiting,
  composioCatalog,
  composioConnect,
  type Catalog,
  type CatalogApp,
  type ComposioStatus,
} from "../../lib/api";
import Select, { type SelectOption } from "../Select/Select";
import AppLogo from "./AppLogo";
import "./ComposioApps.css";

/** How many cards are drawn at first, and added by each "Show more". */
const PAGE = 60;

const clean = (e: unknown) => String(e).replace(/^Error:\s*/, "");
const number = (n: number) => n.toLocaleString("en");

/**
 * `CA-UI-2`: every app I can reach through Composio, to search and connect from.
 * The list ships with the app, so it is whole with or without a key; the key
 * only decides whether Connect works.
 */
export default function BrowseApps({
  status,
  reload,
  onClose,
  focusApp,
  checkError,
}: {
  status: ComposioStatus | null;
  reload: () => void | Promise<void>;
  onClose: () => void;
  /** An app to open the label field for at once ("Add account"). */
  focusApp?: string;
  /** Why the last look at an open sign-in failed, if it did. */
  checkError?: string | null;
}) {
  const [query, setQuery] = useState(focusApp ?? "");
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [category, setCategory] = useState("");
  const [limit, setLimit] = useState(PAGE);
  const configured = status?.configured ?? false;
  const isConnect = status?.mode === "connect";
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    searchRef.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  // Typing searches after a short pause, so each key is not a request.
  useEffect(() => {
    let current = true;
    const timer = setTimeout(() => {
      composioCatalog(query.trim() || undefined)
        .then((c) => {
          if (!current) return;
          setCatalog(c);
          setFailed(null);
        })
        .catch((e) => current && setFailed(clean(e)));
    }, query ? 250 : 0);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [query, configured]);

  // A new search or category starts at the top of the list again.
  useEffect(() => setLimit(PAGE), [query, category]);

  const categories = useMemo(() => {
    const counts = new Map<string, number>();
    for (const app of catalog?.apps ?? []) for (const c of app.categories) counts.set(c, (counts.get(c) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  }, [catalog]);
  const options: SelectOption[] = [
    { value: "", label: "All categories", hint: number(catalog?.apps.length ?? 0) },
    ...categories.map(([name, n]) => ({ value: name, label: name, hint: number(n) })),
  ];

  const matching = (catalog?.apps ?? []).filter((a) => !category || a.categories.includes(category));
  const shown = matching.slice(0, limit);
  // A name the list doesn't know can still be tried: Composio says so if it
  // doesn't know it either.
  const typedSlug = query.trim().toLowerCase();
  const typed: CatalogApp | null =
    isConnect && configured && catalog !== null && matching.length === 0 && /^[a-z0-9_-]{2,64}$/.test(typedSlug)
      ? {
          slug: typedSlug,
          name: typedSlug,
          description: `Connect ${typedSlug}. If Composio doesn't know it, it says so.`,
          categories: [],
          managed: true,
        }
      : null;

  const total = catalog?.total ?? 0;

  return (
    <div className="confirm-backdrop" onClick={onClose}>
      <div className="browse-card" role="dialog" aria-modal="true" aria-label="Browse apps" onClick={(e) => e.stopPropagation()}>
        <div className="browse-head">
          <div className="browse-heading">
            <h2 className="browse-title">Browse apps</h2>
            <span className="browse-count" aria-live="polite">
              {catalog === null
                ? " "
                : matching.length === total
                  ? `${number(total)} apps`
                  : `${number(matching.length)} of ${number(total)} apps`}
            </span>
          </div>
          <button className="btn-text" onClick={onClose}>
            Close
          </button>
        </div>

        {!configured && (
          <p className="browse-key-note">
            You can look at every app now. To connect one, add your Composio key on the Apps page.
          </p>
        )}

        <div className="browse-filters">
          <input
            ref={searchRef}
            className="field-input browse-search"
            type="search"
            placeholder="Search apps"
            aria-label="Search apps"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          {categories.length > 0 && <Select label="Category" value={category} options={options} onChange={setCategory} />}
        </div>

        <div className="browse-grid">
          {failed && <p className="browse-empty">{failed}</p>}
          {!failed && catalog === null && <p className="browse-empty">Looking…</p>}
          {!failed && catalog !== null && matching.length === 0 && !typed && (
            <p className="browse-empty">I couldn't find an app called “{query}”.</p>
          )}
          {typed && (
            <AppCard
              key={`typed-${typed.slug}`}
              app={typed}
              status={status}
              reload={reload}
              canConnect={configured}
              startOpen={false}
              checkError={checkError}
            />
          )}
          {shown.map((app) => (
            <AppCard
              key={app.slug}
              app={app}
              status={status}
              reload={reload}
              canConnect={configured}
              startOpen={app.slug === focusApp}
              checkError={checkError}
            />
          ))}
          {matching.length > shown.length && (
            <div className="browse-more">
              <button className="btn-secondary" onClick={() => setLimit((n) => n + PAGE)}>
                Show more
              </button>
            </div>
          )}
        </div>
        {catalog?.partial && <p className="browse-foot">Search finds the rest.</p>}
      </div>
    </div>
  );
}

function AppCard({
  app,
  status,
  reload,
  canConnect,
  startOpen,
  checkError,
}: {
  app: CatalogApp;
  status: ComposioStatus | null;
  reload: () => void | Promise<void>;
  canConnect: boolean;
  startOpen: boolean;
  checkError?: string | null;
}) {
  const mine = (status?.accounts ?? []).filter((a) => a.app === app.slug && a.status !== "REVOKED");
  const active = mine.filter((a) => a.status === "ACTIVE");
  const waiting = mine.some(accountWaiting);
  const isConnect = status?.mode === "connect";
  // "Add account" opens the name field at once. With a Connect key that is only
  // for an app that already works: a first sign-in needs no name.
  const [asking, setAsking] = useState(startOpen && (isConnect ? active.length > 0 : mine.length > 0));
  const [label, setLabel] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function connect(withLabel?: string) {
    setBusy(true);
    setError(null);
    try {
      // A name is sent only for one more account (`CAP-C6`).
      await composioConnect(app.slug, withLabel?.trim() || undefined);
      setAsking(false);
      setLabel("");
      await reload();
    } catch (e) {
      setError(clean(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="browse-app">
      <AppLogo slug={app.slug} name={app.name} src={app.logo ?? null} />
      <div className="browse-app-main">
        <span className="browse-app-name">{app.name}</span>
        <span className="browse-app-blurb">{app.description || app.categories[0] || " "}</span>
        {!app.managed && (
          <span className="browse-app-note">Needs its own sign-in setup in your Composio project.</span>
        )}

        {waiting ? (
          <span className="browse-app-state">
            Waiting for you in the browser…{" "}
            <button className="link-button inline" disabled={busy} onClick={() => void connect()}>
              Open again
            </button>
            {checkError && (
              <span className="browse-app-error" role="alert">
                {" "}
                I couldn't check: {checkError}
              </span>
            )}
          </span>
        ) : asking ? (
          <span className="browse-app-label">
            <input
              className="field-input"
              placeholder="Call this account, like work"
              aria-label={`Name for the new ${app.name} account`}
              value={label}
              maxLength={64}
              autoFocus
              onChange={(e) => setLabel(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && label.trim() && void connect(label)}
            />
            <button className="btn-primary" disabled={busy || !label.trim()} onClick={() => void connect(label)}>
              Connect
            </button>
            <button className="btn-text" onClick={() => setAsking(false)}>
              Cancel
            </button>
          </span>
        ) : active.length > 0 ? (
          <span className="browse-app-state">
            {isConnect && active.length === 1
              ? "Connected"
              : `Connected · ${active.length} ${active.length === 1 ? "account" : "accounts"}`}{" "}
            <button className="link-button inline" disabled={!canConnect} onClick={() => setAsking(true)}>
              Add account
            </button>
          </span>
        ) : (
          <span>
            <button
              className="btn-secondary"
              disabled={!canConnect || busy}
              title={canConnect ? undefined : "Add your Composio key first"}
              onClick={() => (mine.length > 0 && !isConnect ? setAsking(true) : void connect())}
            >
              {busy ? "Opening…" : "Connect"}
            </button>
          </span>
        )}
        {error && (
          <span className="browse-app-error" role="alert">
            {error}
          </span>
        )}
      </div>
    </div>
  );
}
