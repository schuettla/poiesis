import { useCallback, useEffect, useState } from "react";
import {
  listConnectors,
  addConnector,
  testConnector,
  setConnectorEnabled,
  deleteConnector,
  exportConnectors,
  importConnectors,
  inTauri,
  type ConnectorView,
  type ConnectorStatus,
} from "../lib/api";
import "./Surface.css";
import "./Apps.css";

export default function Apps() {
  const [connectors, setConnectors] = useState<ConnectorView[]>([]);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const [transport, setTransport] = useState<"http" | "stdio">("http");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<Record<string, ConnectorStatus>>({});
  const [busyId, setBusyId] = useState<string | null>(null);
  const [bundleText, setBundleText] = useState("");
  const [bundleMode, setBundleMode] = useState<"export" | "import" | null>(null);
  const [bundleNote, setBundleNote] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!inTauri()) return;
    try {
      setConnectors(await listConnectors());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function connect() {
    const n = name.trim();
    const u = url.trim();
    if (!n || !u) return;
    setConnecting(true);
    setError(null);
    try {
      await addConnector(n, u, token.trim() || undefined, transport);
      setName("");
      setUrl("");
      setToken("");
      setShowAdvanced(false);
      await refresh();
    } catch (e) {
      setError(`Couldn't connect: ${e}`);
    } finally {
      setConnecting(false);
    }
  }

  async function openExport() {
    setBundleNote(null);
    try {
      setBundleText(await exportConnectors());
      setBundleMode("export");
    } catch (e) {
      setError(String(e));
    }
  }

  async function runImport() {
    setBundleNote(null);
    try {
      const count = await importConnectors(bundleText);
      setBundleNote(`Imported ${count} connector${count === 1 ? "" : "s"}. Open each and Test to load its tools.`);
      setBundleText("");
      setBundleMode(null);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function toggle(c: ConnectorView) {
    setBusyId(c.id);
    await setConnectorEnabled(c.id, !c.enabled).catch((e) => setError(String(e)));
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
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  async function remove(c: ConnectorView) {
    setBusyId(c.id);
    await deleteConnector(c.id).catch((e) => setError(String(e)));
    setStatuses((s) => {
      const next = { ...s };
      delete next[c.id];
      return next;
    });
    await refresh();
    setBusyId(null);
  }

  if (!inTauri()) {
    return (
      <div className="surface">
        <div className="surface-inner">
          <h1>Apps</h1>
          <p className="lede">Connectors run in the desktop app.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Apps</h1>
        <p className="lede">
          Connect apps and services so Nexus can act on your behalf. Nexus speaks the open{" "}
          <strong>Model Context Protocol</strong> — paste a server link to connect, and its tools
          become available to the assistant when you turn tools on in a chat.
        </p>

        {error && <p className="hw-note error">{error}</p>}

        {/* Connect a new server */}
        <section className="connect-card">
          <h2 className="section-title">Connect an app</h2>
          <div className="transport-toggle" role="group" aria-label="Connection type">
            <button
              className={`seg ${transport === "http" ? "on" : ""}`}
              aria-pressed={transport === "http"}
              onClick={() => setTransport("http")}
            >
              Remote link
            </button>
            <button
              className={`seg ${transport === "stdio" ? "on" : ""}`}
              aria-pressed={transport === "stdio"}
              onClick={() => setTransport("stdio")}
            >
              Local command
            </button>
          </div>
          <div className="connect-fields">
            <label className="field">
              <span className="field-label">Name</span>
              <input
                className="field-input"
                placeholder="e.g. My Notes"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            <label className="field">
              <span className="field-label">
                {transport === "stdio" ? "Server command" : "Server link"}
              </span>
              <input
                className="field-input"
                placeholder={
                  transport === "stdio"
                    ? "npx -y @modelcontextprotocol/server-filesystem C:/Users/you/docs"
                    : "https://example.com/mcp"
                }
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && connect()}
              />
            </label>
          </div>
          {transport === "stdio" && (
            <p className="field-hint">
              Runs a local MCP server program on your PC and talks to it over stdin/stdout. No token
              needed; it runs with your permissions.
            </p>
          )}

          {transport === "http" && (
            <>
              <button className="link-button" onClick={() => setShowAdvanced((v) => !v)}>
                {showAdvanced ? "Hide advanced setup" : "Advanced setup"}
              </button>
              {showAdvanced && (
                <label className="field">
                  <span className="field-label">
                    Access token <span className="field-hint">(optional — stored in Windows Credential Manager, never in a file)</span>
                  </span>
                  <input
                    className="field-input"
                    type="password"
                    placeholder="Paste a bearer token if the server needs one"
                    value={token}
                    onChange={(e) => setToken(e.target.value)}
                  />
                </label>
              )}
            </>
          )}

          <div className="connect-actions">
            <button
              className="btn-primary"
              onClick={connect}
              disabled={connecting || !name.trim() || !url.trim()}
            >
              {connecting ? "Connecting…" : "Connect"}
            </button>
          </div>
        </section>

        {/* Connected servers */}
        <section className="model-section">
          <h2 className="section-title">Connected</h2>
          {connectors.length === 0 ? (
            <p className="placeholder-note">
              No apps connected yet. Add a Model Context Protocol server above to give the assistant
              new abilities.
            </p>
          ) : (
            <div className="connector-list">
              {connectors.map((c) => {
                const status = statuses[c.id];
                return (
                  <div className={`connector-card ${c.enabled ? "" : "disabled"}`} key={c.id}>
                    <div className="connector-head">
                      <div className="connector-title">
                        <span className={`status-dot ${c.enabled ? "on" : "off"}`} aria-hidden="true" />
                        <span className="connector-name">{c.name}</span>
                        {c.has_auth && <span className="lock" title="Uses a saved access token">🔒</span>}
                      </div>
                      <label className="switch" title={c.enabled ? "Enabled" : "Disabled"}>
                        <input
                          type="checkbox"
                          checked={c.enabled}
                          disabled={busyId === c.id}
                          onChange={() => toggle(c)}
                        />
                        <span className="switch-label">{c.enabled ? "On" : "Off"}</span>
                      </label>
                    </div>

                    <div className="connector-url">{c.url}</div>

                    <div className="connector-tools">
                      {c.tools.length === 0 ? (
                        <span className="tool-empty">No tools discovered</span>
                      ) : (
                        <>
                          <span className="tool-count">
                            {c.tools.length} {c.tools.length === 1 ? "tool" : "tools"}:
                          </span>
                          {c.tools.slice(0, 8).map((t) => (
                            <span className="tool-chip" key={t.name} title={t.description}>
                              {t.name}
                            </span>
                          ))}
                          {c.tools.length > 8 && <span className="tool-chip">+{c.tools.length - 8}</span>}
                        </>
                      )}
                    </div>

                    {status && (
                      <div className={`connector-status ${status.ok ? "ok" : "err"}`}>
                        {status.ok
                          ? `Connected — ${status.tool_count} ${status.tool_count === 1 ? "tool" : "tools"} available`
                          : `Couldn’t connect: ${status.error}`}
                      </div>
                    )}

                    <div className="connector-actions">
                      <button className="btn-secondary" onClick={() => test(c)} disabled={busyId === c.id}>
                        {busyId === c.id ? "Checking…" : "Test connection"}
                      </button>
                      <button className="btn-text danger" onClick={() => remove(c)} disabled={busyId === c.id}>
                        Remove
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {/* Config import / export (MCP-5) */}
          <div className="bundle-bar">
            <button className="btn-text" onClick={openExport} disabled={connectors.length === 0}>
              Export config
            </button>
            <button
              className="btn-text"
              onClick={() => {
                setBundleMode("import");
                setBundleText("");
                setBundleNote(null);
              }}
            >
              Import config
            </button>
          </div>
          {bundleNote && <p className="bundle-note">{bundleNote}</p>}
          {bundleMode && (
            <div className="bundle-panel">
              <p className="field-hint">
                {bundleMode === "export"
                  ? "Copy this bundle to share your connector setup. Tokens are never included."
                  : "Paste a connector bundle. Existing connectors (same link) are skipped; tokens must be re-entered."}
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
                  <button className="btn-primary" onClick={runImport} disabled={!bundleText.trim()}>
                    Import
                  </button>
                )}
                <button className="btn-secondary" onClick={() => setBundleMode(null)}>
                  Close
                </button>
              </div>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
