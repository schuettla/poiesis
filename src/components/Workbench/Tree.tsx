import { useAppStore } from "../../lib/store";
import type { FileNode } from "../../lib/api";

/** "2m", "3h" — enough to place a change in time without a clock. */
function ago(ts: number): string {
  const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

function FolderIcon({ open }: { open: boolean }) {
  return (
    <svg width="13" height="13" viewBox="0 0 20 20" fill="none" aria-hidden="true">
      <path
        d={
          open
            ? "M2.5 15V6A1.5 1.5 0 0 1 4 4.5h3.2l1.4 1.8H15A1.5 1.5 0 0 1 16.5 8H5.6L2.5 15z"
            : "M2.5 6A1.5 1.5 0 0 1 4 4.5h3.2l1.4 1.8H16A1.5 1.5 0 0 1 17.5 8v6A1.5 1.5 0 0 1 16 15.5H4A1.5 1.5 0 0 1 2.5 14z"
        }
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function FileIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 20 20" fill="none" aria-hidden="true">
      <path
        d="M5 3.5h6L15 7.5v9H5z"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
      <path d="M11 3.5v4h4" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
    </svg>
  );
}

/** The working folder, one level at a time. Folders sort first, then files. */
export default function Tree({ filter }: { filter: string }) {
  const root = useAppStore((s) => {
    const conv = s.conversations.find((c) => c.id === s.activeConversationId);
    return conv?.folderPath ?? null;
  });
  if (!root) return null;
  return (
    <div className="wb-tree" role="tree" aria-label="Files">
      <Branch path={root} depth={0} needle={filter.trim().toLowerCase()} />
    </div>
  );
}

function Branch({ path, depth, needle }: { path: string; depth: number; needle: string }) {
  const children = useAppStore((s) => s.folderTree[path]);
  if (!children) return depth === 0 ? <p className="wb-hint">Reading the folder…</p> : null;

  const visible = needle
    ? children.filter((c) => c.is_dir || c.name.toLowerCase().includes(needle))
    : children;

  if (visible.length === 0) {
    return depth === 0 ? <p className="wb-hint">This folder is empty.</p> : null;
  }
  return (
    <>
      {visible.map((node) => (
        <Row key={node.path} node={node} depth={depth} needle={needle} />
      ))}
    </>
  );
}

function Row({ node, depth, needle }: { node: FileNode; depth: number; needle: string }) {
  const expanded = useAppStore((s) => s.expandedDirs.includes(node.path));
  const toggleDir = useAppStore((s) => s.toggleDir);
  const selectNode = useAppStore((s) => s.selectNode);
  const openInSystem = useAppStore((s) => s.openInSystem);
  const revealInSystem = useAppStore((s) => s.revealInSystem);
  const selected = useAppStore((s) => s.selected);
  const touched = useAppStore((s) => s.touchedFiles[node.path]);
  const isActive = selected?.kind === "file" && selected.id === node.path;

  const activate = () => {
    if (node.is_dir) toggleDir(node.path);
    else selectNode({ kind: "file", id: node.path });
  };

  return (
    <>
      <div
        className={`wb-row ${isActive ? "active" : ""} ${node.is_dir ? "is-dir" : ""}`}
        role="treeitem"
        aria-selected={isActive}
        aria-expanded={node.is_dir ? expanded : undefined}
        tabIndex={0}
        style={{ paddingLeft: 10 + depth * 13 }}
        onClick={activate}
        onDoubleClick={() => !node.is_dir && openInSystem(node.path)}
        onKeyDown={(e) => e.key === "Enter" && activate()}
        onContextMenu={(e) => {
          e.preventDefault();
          revealInSystem(node.path);
        }}
        title={node.path}
      >
        <span className="wb-row-caret" aria-hidden="true">
          {node.is_dir ? (expanded ? "▾" : "▸") : ""}
        </span>
        <span className="wb-row-icon">{node.is_dir ? <FolderIcon open={expanded} /> : <FileIcon />}</span>
        <span className="wb-row-name">{node.name}</span>
        {touched && (
          <span className="wb-touched" title="Changed in this session">
            ● {ago(touched)}
          </span>
        )}
      </div>
      {node.is_dir && expanded && <Branch path={node.path} depth={depth + 1} needle={needle} />}
    </>
  );
}
