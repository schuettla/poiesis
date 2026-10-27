import { useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { classifyCorrect, inTauri, saveArtifactFile } from "../../lib/api";
import {
  arrange,
  csvName,
  lowCount,
  nextSort,
  readClassify,
  readCorrections,
  shown,
  toCsv,
  type ClassifyRow,
  type Filter,
  type Sort,
  type SortKey,
} from "../../lib/classify";
import { useAppStore } from "../../lib/store";
import type { BlockView } from "../../lib/types";
import Select from "../Select/Select";
import Frame from "./Frame";

/** More answers than this are picked from a list; fewer are shown as buttons. */
const CHIPS_UP_TO = 6;

const clean = (e: unknown) => String(e).replace(/^Error:\s*/, "");

const ARROW = { asc: "↑", desc: "↓" } as const;

/**
 * `USE-UI-5`: the table the `classify` tool hands the user. Every item with the
 * answer the quick model gave and how sure it was, sortable, with the rows it was
 * not sure about one click away, an export, and a way to say what a row should
 * have been, which is the label the model's probabilities are later checked with.
 */
export default function ClassifyBlock({ block }: { block: BlockView }) {
  const data = useMemo(() => readClassify(block.data), [block.data]);
  const state = (block.state && typeof block.state === "object" ? block.state : {}) as Record<string, unknown>;
  const corrections = readCorrections(block.state);
  const setBlockState = useAppStore((s) => s.setBlockState);

  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const low = lowCount(data, corrections);
  const rows = arrange(data, corrections, filter, sort);
  const unsorted = data.rows.filter((r) => r.note).length;

  async function pick(row: ClassifyRow, value: string) {
    setEditing(null);
    setError(null);
    setBlockState(block.id, { ...state, corrections: { ...corrections, [row.index]: value } });
    if (!inTauri() || !row.decision) return;
    try {
      await classifyCorrect(row.decision, value);
    } catch (e) {
      // The label was not kept, so the table does not claim it was.
      setBlockState(block.id, { ...state, corrections });
      setError(clean(e));
    }
  }

  async function exportCsv() {
    setError(null);
    const csv = toCsv(data, corrections);
    const name = csvName(data.question);
    try {
      if (!inTauri()) {
        const url = URL.createObjectURL(new Blob([csv], { type: "text/csv" }));
        const a = document.createElement("a");
        a.href = url;
        a.download = name;
        a.click();
        URL.revokeObjectURL(url);
        return;
      }
      const dest = await save({ defaultPath: name });
      if (dest) await saveArtifactFile(dest, "csv", csv);
    } catch (e) {
      setError(clean(e));
    }
  }

  const head = (key: SortKey, label: string, hint?: string) => (
    <th
      className="tbl-label"
      aria-sort={sort?.key === key ? (sort.dir === "asc" ? "ascending" : "descending") : "none"}
      title={hint}
    >
      <button className="cls-sort" onClick={() => setSort(nextSort(sort, key))}>
        {label}
        {sort?.key === key && <span aria-hidden="true">{ARROW[sort.dir]}</span>}
      </button>
    </th>
  );

  const footer = [
    `${data.rows.length} row${data.rows.length === 1 ? "" : "s"}`,
    low > 0 && `${low} low confidence`,
    unsorted > 0 && `${unsorted} not sorted`,
    data.notReached > 0 && `${data.notReached} not reached${data.stopped ? ` (${data.stopped})` : ""}`,
  ]
    .filter(Boolean)
    .join(" · ");

  const confidenceHint = `How sure the quick model is of its answer. Under ${data.bar.toFixed(2)} a row is marked low.${
    data.calibrated ? "" : " These numbers are the model's own and have not been checked against examples yet."
  }`;

  return (
    <Frame title={block.title} kind="classify" footer={footer}>
      <div className="cls-tools">
        <button className={`col-chip${filter === "all" ? " on" : ""}`} aria-pressed={filter === "all"} onClick={() => setFilter("all")}>
          All ({data.rows.length})
        </button>
        <button className={`col-chip${filter === "low" ? " on" : ""}`} aria-pressed={filter === "low"} onClick={() => setFilter("low")}>
          Low confidence ({low})
        </button>
        <button className="btn-text" onClick={() => void exportCsv()}>
          Export CSV
        </button>
      </div>
      {error && (
        <p className="cls-error" role="alert">
          {error}
        </p>
      )}
      {rows.length === 0 ? (
        <p className="block-empty">{filter === "low" ? "No row is low confidence." : "Nothing was sorted."}</p>
      ) : (
        <div className="tbl-scroll">
          <table className="tbl-table">
            <thead>
              <tr>
                {head("item", "Item")}
                {head("answer", "Answer")}
                {head("confidence", "Confidence", confidenceHint)}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const s = shown(row, data, corrections);
                const canEdit = !!row.decision && data.choices.length > 0;
                return (
                  <tr key={row.index} className={`tbl-row${s.low ? " cls-low" : ""}`}>
                    <td className="cls-item" title={row.path ?? row.item}>
                      {row.thumb && <img className="cls-thumb" src={row.thumb} alt="" />}
                      {row.item}
                    </td>
                    {row.note ? (
                      <td className="cls-note" colSpan={2}>
                        Not sorted: {row.note}
                      </td>
                    ) : (
                      <>
                        <td className="tbl-cell">
                          {editing === row.index && canEdit ? (
                            <Choices
                              row={row}
                              choices={data.choices}
                              current={corrections[String(row.index)] ?? row.value ?? ""}
                              onPick={(v) => void pick(row, v)}
                              onClose={() => setEditing(null)}
                            />
                          ) : canEdit ? (
                            <button
                              className="cls-answer"
                              aria-label={`${s.answer ?? "No answer"}. Change the answer for ${row.item}`}
                              onClick={() => setEditing(row.index)}
                            >
                              {s.answer}
                              {s.corrected && <span className="cls-you">{" · you"}</span>}
                            </button>
                          ) : (
                            s.answer
                          )}
                        </td>
                        <td className="tbl-cell" title={s.confidence === undefined ? undefined : `Judged ${s.answer} (${s.confidence.toFixed(2)})`}>
                          {s.confidence === undefined ? (
                            <span className="cls-num">{s.corrected ? "you chose it" : "not judged"}</span>
                          ) : (
                            <>
                              <span className="cls-bar" aria-hidden="true">
                                <i style={{ width: `${Math.round(s.confidence * 100)}%` }} />
                              </span>
                              <span className="cls-num">{s.confidence.toFixed(2)}</span>
                              {s.low && <span className="cls-flag"> low</span>}
                            </>
                          )}
                        </td>
                      </>
                    )}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </Frame>
  );
}

/** What a row can be changed to. A few answers are buttons; many are a list. */
function Choices({
  row,
  choices,
  current,
  onPick,
  onClose,
}: {
  row: ClassifyRow;
  choices: { value: string; label: string }[];
  current: string;
  onPick: (value: string) => void;
  onClose: () => void;
}) {
  if (choices.length > CHIPS_UP_TO) {
    return (
      <Select
        label={`Answer for ${row.item}`}
        value={current}
        options={choices}
        onChange={onPick}
      />
    );
  }
  return (
    <div
      className="cls-choices"
      role="group"
      aria-label={`Answer for ${row.item}`}
      onKeyDown={(e) => e.key === "Escape" && onClose()}
    >
      {choices.map((c) => (
        <button key={c.value} className={`col-chip${c.value === current ? " on" : ""}`} aria-pressed={c.value === current} onClick={() => onPick(c.value)}>
          {c.label}
        </button>
      ))}
    </div>
  );
}
