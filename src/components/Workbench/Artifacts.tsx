import { useState } from "react";
import { useAppStore } from "../../lib/store";
import type { Artifact } from "../../lib/api";
import { extFor, slugify } from "./artifactFiles";

/**
 * Things the agent made in this chat.
 *
 * These are not files — they live with the conversation until you save one,
 * which writes it into the working folder and moves it into the tree above.
 * Keeping them in their own section says that plainly instead of dressing them
 * up as something that's already on disk.
 */
export default function Artifacts({ artifacts, canSave }: { artifacts: Artifact[]; canSave: boolean }) {
  return (
    <div className="wb-artifacts" role="list">
      {artifacts.map((a) => (
        <Row key={a.id} artifact={a} canSave={canSave} />
      ))}
    </div>
  );
}

function Row({ artifact, canSave }: { artifact: Artifact; canSave: boolean }) {
  const selected = useAppStore((s) => s.selected);
  const selectNode = useAppStore((s) => s.selectNode);
  const saveArtifactToFolder = useAppStore((s) => s.saveArtifactToFolder);
  const [name, setName] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const isActive = selected?.kind === "artifact" && selected.id === artifact.id;

  const commit = async (value: string) => {
    setError(null);
    try {
      await saveArtifactToFolder(artifact.id, value);
      setName(null);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <>
      <div
        className={`wb-row wb-artifact ${isActive ? "active" : ""}`}
        role="listitem"
        tabIndex={0}
        onClick={() => selectNode({ kind: "artifact", id: artifact.id })}
        onKeyDown={(e) => e.key === "Enter" && selectNode({ kind: "artifact", id: artifact.id })}
        style={{ paddingLeft: 10 }}
        title={artifact.title}
      >
        <span className="wb-row-caret" aria-hidden="true" />
        <span className="wb-row-name">{artifact.title}</span>
        <span className="wb-kind">{artifact.kind}</span>
        {canSave && (
          <button
            className="wb-save"
            title="Write this into the working folder"
            onClick={(e) => {
              e.stopPropagation();
              setName(`${slugify(artifact.title)}.${extFor(artifact)}`);
            }}
          >
            Save
          </button>
        )}
      </div>
      {name !== null && (
        <div className="wb-save-row" onClick={(e) => e.stopPropagation()}>
          <input
            className="wb-save-input"
            autoFocus
            value={name}
            aria-label="File name"
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit(name);
              if (e.key === "Escape") setName(null);
            }}
          />
          <button className="wb-link" onClick={() => commit(name)}>
            Save
          </button>
          <button className="wb-link" onClick={() => setName(null)}>
            Cancel
          </button>
        </div>
      )}
      {error && <p className="wb-error">{error}</p>}
    </>
  );
}
