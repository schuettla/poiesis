import { useState } from "react";
import {
  addConnector,
  deleteConnector,
  exportConnectors,
  importConnectors,
  setConnectorEnabled,
  testConnector,
  type ConnectorStatus,
  type ConnectorView,
} from "../../lib/api";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import { PlusIcon } from "../Icons/Icons";
import { ToolChips, WhatIMayDo } from "./ConnectorGate";
import "./McpServers.css";

const clean = (e: unknown) => String(e).replace(/^Error:\s*/, "");

/** Remote servers are reached by a link; local ones are programs run on this PC. */
const isLocal = (c: Pick<ConnectorView, "transport">) => c.transport === "stdio";

/**
 * The Apps page's MCP tab: servers the person added themselves, by link or by
 * command. One toolbar (count, Manage, Add server), the form only when asked
 * for, and one card per server.
 */
export default function McpServers({
  connectors,
  refresh,
  onError,
}: {
  connectors: ConnectorView[];
  refresh: () => Promise<void>;
  onError: (message: string) => void;
}) {
  const [adding, setAdding] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [statuses, setStatuses] = useState<Record<string, ConnectorStatus>>({});
  const [busyId, setBusyId] = useState<string | null>(null);
  const [removing, setRemoving] = useState<ConnectorView | null>(null);
  const [bundleText, setBundleText] = useState("");
  const [bundleMode, setBundleMode] = useState<"export" | "import" | null>(null);
  const [bundleNote, setBundleNote] = useState<string | null>(null);

  const on = connectors.filter((c) => c.enabled).length;
  const formOpen = adding || connectors.length === 0;

  async function openExport() {
    setMenuOpen(false);
    setBundleNote(null);
    try {
      setBundleText(await exportConnectors());
      setBundleMode("export");
    } catch (e) {
      onError(clean(e));
    }
  }

  async function runImport() {
    setBundleNote(null);
    try {
      const count = await importConnectors(bundleText);
      setBundleNote(`Imported ${count} server${count === 1 ? "" : "s"}. Test each one to load its tools.`);
      setBundleText("");
      setBundleMode(null);
      await refresh();
    } catch (e) {
      onError(clean(e));
    }
  }

  async function toggle(c: ConnectorView) {
    setBusyId(c.id);
    await setConnectorEnabled(c.id, !c.enabled).catch((e) => onError(clean(e)));
    await refresh();
    setBusyId(null);
  }

  async function test(c: ConnectorView) {
    setBusyId(c.id);
    try {
      const status = await testConnector(c.id);
      setStatuses((s) => ({ ...s, [c.id]: status }));
      await refresh();
    } catch (e) {
      onError(clean(e));
    } finally {
      setBusyId(null);
    }
  }

  async function remove(c: ConnectorView) {
    setRemoving(null);
    setBusyId(c.id);
    await deleteConnector(c.id).catch((e) => onError(clean(e)));
    setStatuses((s) => {
      const next = { ...s };
      delete next[c.id];
      return next;
    });
    await refresh();
    setBusyId(null);
  }

  return (
    <>
      <p className="apps-intro">
        Any server that speaks the open <strong>Model Context Protocol</strong> gives me new tools: a remote one by its
        link, or a program on this PC by its command. I read without asking and ask before I change or send anything;
        each server can be set differently.
      </p>

      <div className="mcp-toolbar">
        <span className="mcp-count">
          {connectors.length === 0
            ? "No servers yet"
            : `${connectors.length} ${connectors.length === 1 ? "server" : "servers"} · ${on} on`}
        </span>
        <div className="provider-manage mcp-manage">
          <button className="btn-text" aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => setMenuOpen((o) => !o)}>
            Manage ▾
          </button>
          {menuOpen && (
            <div className="provider-menu" role="menu" onMouseLeave={() => setMenuOpen(false)}>
              <button
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  setBundleMode("import");
                  setBundleText("");
                  setBundleNote(null);
                }}
              >
                Import servers
              </button>
              <button role="menuitem" disabled={connectors.length === 0} onClick={() => void openExport()}>
                Export servers
              </button>
            </div>
          )}
        </div>
        {!formOpen && (
          <button className="btn-primary mcp-add-button" onClick={() => setAdding(true)}>
            <PlusIcon /> Add server
          </button>
        )}
      </div>

      {bundleNote && <p className="bundle-note">{bundleNote}</p>}
      {bundleMode && (
        <section className="connect-card">
          <h2 className="mcp-card-title">{bundleMode === "export" ? "Export servers" : "Import servers"}</h2>
          <p className="field-hint">
            {bundleMode === "export"
              ? "Copy this to set up the same servers elsewhere. Tokens are never included."
              : "Paste servers exported from Poiesis. Ones with the same link are skipped; tokens are entered again."}
          </p>
          <textarea
            className="bundle-text"
            rows={8}
            spellCheck={false}
            readOnly={bundleMode === "export"}
            value={bundleText}
            onChange={(e) => setBundleText(e.target.value)}
            placeholder={bundleMode === "import" ? '{ "version": 1, "connectors": [ … ] }' : ""}
          />
          <div className="connect-actions">
            {bundleMode === "import" && (
              <button className="btn-primary" onClick={() => void runImport()} disabled={!bundleText.trim()}>
                Import
              </button>
            )}
            <button className="btn-secondary" onClick={() => setBundleMode(null)}>
              Close
            </button>
          </div>
        </section>
      )}

      {formOpen && (
        <AddServer
          canCancel={connectors.length > 0}
          onCancel={() => setAdding(false)}
          onAdded={async () => {
            setAdding(false);
            await refresh();
          }}
        />
      )}

      {connectors.length > 0 && (
        <div className="connector-list">
          {connectors.map((c) => {
            const status = statuses[c.id];
            const busy = busyId === c.id;
            return (
              <div className={`connector-card ${c.enabled ? "" : "disabled"}`} key={c.id}>
                <div className="connector-head">
                  <div className="connector-title">
                    <span className={`status-dot ${c.enabled ? "on" : "off"}`} aria-hidden="true" />
                    <span className="connector-name">{c.name}</span>
                    <span className="mcp-badge">{isLocal(c) ? "Local" : "Remote"}</span>
                    {c.has_auth && <span className="mcp-badge quiet">Token saved</span>}
                  </div>
                  <label className="switch" title={c.enabled ? "On" : "Off"}>
                    <input type="checkbox" checked={c.enabled} disabled={busy} onChange={() => void toggle(c)} />
                    <span className="switch-label">{c.enabled ? "On" : "Off"}</span>
                  </label>
                </div>

                <div className="connector-url">{c.url}</div>

                <div className="connector-tools">
                  <ToolChips connector={c} onChanged={refresh} onError={onError} />
                </div>

                <WhatIMayDo connector={c} onChanged={refresh} onError={onError} />

                {status && (
                  <div className={`connector-status ${status.ok ? "ok" : "err"}`} role={status.ok ? undefined : "alert"}>
                    {status.ok
                      ? `Connected · ${status.tool_count} ${status.tool_count === 1 ? "tool" : "tools"}`
                      : `Couldn't connect: ${status.error}`}
                  </div>
                )}

                <div className="connector-actions">
                  <button className="btn-secondary" onClick={() => void test(c)} disabled={busy}>
                    {busy ? "Checking…" : "Test connection"}
                  </button>
                  <button className="btn-text danger" onClick={() => setRemoving(c)} disabled={busy}>
                    Remove
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {removing && (
        <ConfirmDialog
          title={`Remove ${removing.name}?`}
          body={
            removing.has_auth
              ? "I stop using its tools, and its saved token is deleted. You can add it again later."
              : "I stop using its tools. You can add it again later."
          }
          confirmLabel="Remove"
          onConfirm={() => void remove(removing)}
          onCancel={() => setRemoving(null)}
        />
      )}
    </>
  );
}

/** The form for one new server. Its own state, so closing it forgets a half-typed one. */
function AddServer({
  canCancel,
  onCancel,
  onAdded,
}: {
  canCancel: boolean;
  onCancel: () => void;
  onAdded: () => Promise<void>;
}) {
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const [transport, setTransport] = useState<"http" | "stdio">("http");
  const [showToken, setShowToken] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  async function connect() {
    const n = name.trim();
    const u = url.trim();
    if (!n || !u) return;
    setConnecting(true);
    setProblem(null);
    try {
      await addConnector(n, u, token.trim() || undefined, transport);
      await onAdded();
    } catch (e) {
      // The fields keep their values so a typo can be fixed in place.
      setProblem(`Couldn't connect: ${clean(e)}`);
    } finally {
      setConnecting(false);
    }
  }

  return (
    <section className="connect-card mcp-add">
      <div className="mcp-add-head">
        <h2 className="mcp-card-title">Add a server</h2>
        <div className="segmented" role="group" aria-label="Connection type">
          <button
            className={`segmented-item ${transport === "http" ? "on" : ""}`}
            aria-pressed={transport === "http"}
            onClick={() => setTransport("http")}
          >
            Remote link
          </button>
          <button
            className={`segmented-item ${transport === "stdio" ? "on" : ""}`}
            aria-pressed={transport === "stdio"}
            onClick={() => setTransport("stdio")}
          >
            Local command
          </button>
        </div>
      </div>
      <div className="connect-fields">
        <label className="field mcp-field-name">
          <span className="field-label">Name</span>
          <input className="field-input" placeholder="My notes" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="field mcp-field-url">
          <span className="field-label">{transport === "stdio" ? "Command" : "Link"}</span>
          <input
            className="field-input"
            placeholder={
              transport === "stdio"
                ? "npx -y @modelcontextprotocol/server-filesystem C:/Users/you/docs"
                : "https://example.com/mcp"
            }
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void connect()}
          />
        </label>
      </div>
      <p className="field-hint mcp-hint">
        {transport === "stdio"
          ? "Runs the program on this PC, with your permissions, and talks to it directly. No token needed."
          : "The server's MCP address. If it needs a token, add one below."}
      </p>
      {transport === "http" &&
        (showToken ? (
          <label className="field">
            <span className="field-label">Access token</span>
            <input
              className="field-input"
              type="password"
              placeholder="Paste the token the server gave you"
              value={token}
              onChange={(e) => setToken(e.target.value)}
            />
            <span className="field-hint">Kept in Windows Credential Manager, never in a file.</span>
          </label>
        ) : (
          <button className="link-button" onClick={() => setShowToken(true)}>
            Add an access token
          </button>
        ))}
      {problem && (
        <p className="composio-problem" role="alert">
          {problem}
        </p>
      )}
      <div className="connect-actions">
        <button className="btn-primary" onClick={() => void connect()} disabled={connecting || !name.trim() || !url.trim()}>
          {connecting ? "Connecting…" : "Connect"}
        </button>
        {canCancel && (
          <button className="btn-text" onClick={onCancel}>
            Cancel
          </button>
        )}
      </div>
    </section>
  );
}
