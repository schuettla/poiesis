import { useCallback, useEffect, useState } from "react";
import { listConnectors, inTauri, type ConnectorView } from "../lib/api";
import { useAppStore } from "../lib/store";
import ComposioSection from "../components/Apps/ComposioSection";
import McpServers from "../components/Apps/McpServers";
import { useComposio } from "../components/Apps/useComposio";
import "./Surface.css";
import "./Models.css";
import "./Apps.css";

type AppsTab = "composio" | "mcp";

/**
 * Apps: two ways in, one tab each. Composio brings hundreds of apps through
 * one key; MCP servers are ones the person adds by link or command. They share
 * nothing but the gate (what I may do without asking), so they don't share a page.
 */
export default function Apps() {
  const [connectors, setConnectors] = useState<ConnectorView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<AppsTab | null>(null);
  const composio = useComposio(inTauri());
  const needsAttention = useAppStore((s) => s.appsNeedAttention);

  const refresh = useCallback(async () => {
    if (!inTauri()) return;
    try {
      // Composio has its own tab; it is not a server the person added.
      setConnectors((await listConnectors()).filter((c) => c.transport !== "composio"));
      // What a server offers as prompts follows what is connected and turned on.
      void useAppStore.getState().refreshMcpPrompts();
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

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

  // Until the person picks one: Composio, unless they only use servers of their own.
  const shown: AppsTab = tab ?? (!composio.status?.configured && connectors.length > 0 ? "mcp" : "composio");
  const appCount = new Set((composio.status?.accounts ?? []).filter((a) => a.status === "ACTIVE").map((a) => a.app)).size;

  const tabs: { id: AppsTab; label: string; count: number; dot?: boolean }[] = [
    { id: "mcp", label: "MCP servers", count: connectors.length },
    { id: "composio", label: "Composio", count: appCount, dot: needsAttention },
  ];

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Apps</h1>
        <p className="lede">
          Connect the apps you use and I can work in them for you. I read without asking, and I ask before I change or
          send anything.
        </p>

        <div className="model-tabs apps-tabs" role="tablist" aria-label="Kind of app">
          {tabs.map((t) => (
            <button
              key={t.id}
              className={`model-tab ${shown === t.id ? "on" : ""}`}
              role="tab"
              aria-selected={shown === t.id}
              onClick={() => {
                setTab(t.id);
                setError(null);
              }}
            >
              {t.label}
              {t.count > 0 && <span className="apps-tab-count">{t.count}</span>}
              {t.dot && <span className="apps-tab-dot" aria-label="needs attention" />}
            </button>
          ))}
        </div>

        {error && <p className="hw-note error">{error}</p>}

        {shown === "composio" ? (
          <div role="tabpanel" aria-label="Composio">
            {/* Without a key the card itself explains Composio; with one, a line says what this tab is. */}
            {composio.status?.configured && (
              <p className="apps-intro">
                Gmail, Slack, Notion, Linear and hundreds more, through one Composio key. Composio holds the sign-ins;
                you choose what I may do in each app.
              </p>
            )}
            <ComposioSection
              status={composio.status}
              reload={composio.reload}
              onError={setError}
              checkError={composio.checkError}
            />
          </div>
        ) : (
          <div role="tabpanel" aria-label="MCP servers">
            <McpServers connectors={connectors} refresh={refresh} onError={setError} />
          </div>
        )}
      </div>
    </div>
  );
}
