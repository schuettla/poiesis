import { useEffect, useState } from "react";
import {
  inTauri,
  listToolsets,
  setToolsetEnabled,
  getToolStats,
  listIndexRoots,
  formatDiskSize,
  type ToolsetInfo,
  type ToolsetReliability,
  type IndexRootView,
} from "../lib/api";
import { useAppStore } from "../lib/store";
import "./Surface.css";
import "./Settings.css";

export default function Tools() {
  const [toolsets, setToolsets] = useState<ToolsetInfo[]>([]);
  const [reliability, setReliability] = useState<ToolsetReliability[]>([]);
  const [indexRoots, setIndexRoots] = useState<IndexRootView[]>([]);
  const forgetFolderIndex = useAppStore((s) => s.forgetFolderIndex);

  useEffect(() => {
    if (!inTauri()) return;
    listToolsets().then(setToolsets).catch(() => {});
    getToolStats().then(setReliability).catch(() => {});
    listIndexRoots().then(setIndexRoots).catch(() => {});
  }, []);

  async function toggleToolset(id: string, enabled: boolean) {
    // Optimistic; revert on failure.
    setToolsets((list) => list.map((s) => (s.id === id ? { ...s, enabled } : s)));
    try {
      await setToolsetEnabled(id, enabled);
    } catch {
      setToolsets((list) => list.map((s) => (s.id === id ? { ...s, enabled: !enabled } : s)));
    }
  }

  async function forgetIndexRoot(path: string) {
    await forgetFolderIndex(path);
    setIndexRoots((list) => list.filter((r) => r.path !== path));
  }

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Tools</h1>
        <p className="lede">
          What Poiesis Agent can do beyond chatting, when tools are turned on in a chat. Each one is
          opt-in; those that leave your device or run code are marked.
        </p>

        {inTauri() && (
          <section className="setting-block">
            {toolsets.map((s) => {
              const rel = reliability.find((r) => r.skill_id === s.id);
              return (
                <div key={s.id} className="toolset-item">
                  <label className="toggle-line toolset-line">
                    <input
                      type="checkbox"
                      checked={s.enabled}
                      onChange={(e) => toggleToolset(s.id, e.target.checked)}
                    />
                    <span className="toolset-text">
                      <span className="toolset-label">
                        {s.label}
                        {s.sensitive && <span className="toolset-flag">leaves device / runs code</span>}
                      </span>
                      <span className="toolset-desc">{s.description}</span>
                      {rel && (
                        <span className="toolset-reliability">
                          {rel.ok_percent}% ok over {rel.calls} call{rel.calls === 1 ? "" : "s"} this
                          week
                        </span>
                      )}
                    </span>
                  </label>
                  {/* IDX-UI-4: the indexed folders this tool has built, wherever
                      they were attached from — with the one undo that matters. */}
                  {s.id === "indexing" && indexRoots.length > 0 && (
                    <ul className="toolset-subitems">
                      {indexRoots.map((r) => (
                        <li key={r.path} className="toolset-subitem">
                          <span className="toolset-subitem-path" title={r.path}>
                            {r.path}
                          </span>
                          <span className="toolset-subitem-meta">{formatDiskSize(r.size_bytes)}</span>
                          <button className="link-button" onClick={() => forgetIndexRoot(r.path)}>
                            Forget this folder
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              );
            })}
          </section>
        )}
      </div>
    </div>
  );
}
