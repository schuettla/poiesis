import { Component, Fragment, useMemo, useState } from "react";
import type { ReactNode } from "react";
import type { BlockView } from "../../lib/types";
import { useAppStore } from "../../lib/store";
import type { Diagnostic } from "../../lib/api";
import DiagnosticsList, { locateDiagnostic } from "../Conversation/Diagnostics";
import ConnectBlock from "./ConnectBlock";
import Frame from "./Frame";
import "./blocks.css";

/** Loose object access for lenient, model-provided block data. */
type Obj = Record<string, any>;
const asObj = (x: unknown): Obj => (x && typeof x === "object" ? (x as Obj) : {});
const asArr = (x: unknown): Obj[] => (Array.isArray(x) ? (x as Obj[]) : []);

/** A typed workspace block (Generative UI). Renders the right instrument for the
 * kind, styled as paper machinery; falls back to a raw-JSON dump if anything is
 * malformed so nothing is ever lost. */
export default function BlockRenderer({ block }: { block: BlockView }) {
  return (
    <BlockErrorBoundary block={block}>
      <BlockFrame block={block} />
    </BlockErrorBoundary>
  );
}

function BlockFrame({ block }: { block: BlockView }) {
  switch (block.kind) {
    case "connect":
      return <ConnectBlock block={block} />;
    case "comparison":
      return <ComparisonBlock block={block} />;
    case "collection":
      return <CollectionBlock block={block} />;
    case "plan":
      return <PlanBlock block={block} />;
    case "form":
      return <FormBlock block={block} />;
    case "progress":
      return <ProgressBlock block={block} />;
    case "document":
      return <DocumentBlock block={block} />;
    case "table":
      return <TableBlock block={block} />;
    case "diagnostics":
      return <DiagnosticsBlock block={block} />;
    default:
      return <RawBlock block={block} />;
  }
}

// ---- diagnostics (COD-UI-3) ----

/** The model hands over a build's findings as one block instead of describing
 * them in prose. Lenient on shape: severity words the model reaches for are
 * mapped onto the three the list draws. */
function DiagnosticsBlock({ block }: { block: BlockView }) {
  const openItem = useAppStore((s) => s.openItem);
  const root = useAppStore(
    (s) => s.conversations.find((c) => c.id === s.activeConversationId)?.folderPath ?? null
  );
  const items: Diagnostic[] = asArr(asObj(block.data).items).map((d) => {
    const sev = String(d.severity ?? "error").toLowerCase();
    return {
      file: String(d.file ?? ""),
      line: typeof d.line === "number" ? d.line : null,
      col: typeof d.col === "number" ? d.col : null,
      severity: sev.startsWith("warn") ? "warning" : sev.startsWith("fail") ? "failure" : "error",
      message: String(d.message ?? ""),
      code: d.code != null ? String(d.code) : null,
    };
  });
  const errors = items.filter((d) => d.severity !== "warning").length;
  const warnings = items.length - errors;
  const footer = [errors && `${errors} error${errors === 1 ? "" : "s"}`, warnings && `${warnings} warning${warnings === 1 ? "" : "s"}`]
    .filter(Boolean)
    .join(" · ");
  return (
    <Frame title={block.title} kind="diagnostics" footer={footer || undefined}>
      {items.length === 0 ? (
        <p className="block-empty">Nothing to report.</p>
      ) : (
        <DiagnosticsList
          items={items}
          onOpen={
            root
              ? (d) => {
                  const path = locateDiagnostic(d.file, root);
                  if (path) openItem({ kind: "file", id: path, line: d.line ?? undefined });
                }
              : undefined
          }
        />
      )}
    </Frame>
  );
}

// ---- comparison ----

function ComparisonBlock({ block }: { block: BlockView }) {
  const data = asObj(block.data);
  const columns = asArr(data.columns);
  const options = asArr(data.options);
  const recommendedId = data.recommended_id as string | undefined;
  const state = asObj(block.state);
  const pinned = (state.pinned as string[]) ?? [];
  const busy = useAppStore((s) => s.busy);
  const sendBlockAction = useAppStore((s) => s.sendBlockAction);
  const setBlockState = useAppStore((s) => s.setBlockState);
  const [expanded, setExpanded] = useState<string | null>(null);

  const togglePin = (opt: Obj) => {
    const id = String(opt.id);
    const isPinned = pinned.includes(id);
    const next = isPinned ? pinned.filter((x) => x !== id) : [...pinned, id];
    setBlockState(block.id, { ...state, pinned: next });
    if (!isPinned) {
      sendBlockAction(block.id, `Pinned “${opt.label ?? id}” in ${block.title}`, {
        a: "block_action",
        block: block.id,
        kind: "comparison",
        action: "pin",
        option: id,
        label: opt.label ?? id,
      });
    }
  };

  const pinnedLabels = options
    .filter((o) => pinned.includes(String(o.id)))
    .map((o) => o.label ?? o.id);

  return (
    <Frame
      title={block.title}
      kind="comparison"
      footer={pinnedLabels.length ? `pinned: ${pinnedLabels.join(", ")}` : undefined}
    >
      <div className="cmp-scroll">
        <table className="cmp-table">
          <thead>
            <tr>
              <th className="cmp-optcol" />
              {columns.map((c) => (
                <th key={String(c.id)} className="cmp-label">
                  {c.label ?? c.id}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {options.map((o) => {
              const id = String(o.id);
              const isRec = recommendedId === id;
              const isPinned = pinned.includes(id);
              const values = asObj(o.values);
              return (
                <Fragment key={id}>
                  <tr
                    className={`cmp-row${isRec ? " recommended" : ""}`}
                    onClick={() => setExpanded(expanded === id ? null : id)}
                  >
                    <td className="cmp-opt">
                      <button
                        className={`cmp-pin${isPinned ? " on" : ""}`}
                        disabled={busy}
                        aria-label={isPinned ? `Unpin ${o.label ?? id}` : `Pin ${o.label ?? id}`}
                        aria-pressed={isPinned}
                        onClick={(e) => {
                          e.stopPropagation();
                          togglePin(o);
                        }}
                      >
                        {isPinned ? "●" : "○"}
                      </button>
                      <span className="cmp-optname">{o.label ?? id}</span>
                      {isRec && (
                        <span className="cmp-rec" title="recommended">
                          ◆ recommended
                        </span>
                      )}
                    </td>
                    {columns.map((c) => (
                      <td key={String(c.id)} className="cmp-val">
                        {formatCell(values[String(c.id)])}
                      </td>
                    ))}
                  </tr>
                  {expanded === id && (o.pros || o.cons) && (
                    <tr className="cmp-detail-row">
                      <td colSpan={columns.length + 1}>
                        {o.pros && (
                          <div className="cmp-detail">
                            pros: {asArr2(o.pros).join(", ")}
                          </div>
                        )}
                        {o.cons && (
                          <div className="cmp-detail">
                            cons: {asArr2(o.cons).join(", ")}
                          </div>
                        )}
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      </div>
    </Frame>
  );
}

// ---- collection ----

function CollectionBlock({ block }: { block: BlockView }) {
  const data = asObj(block.data);
  const items = asArr(data.items);
  const state = asObj(block.state);
  const activeTags = (state.tags as string[]) ?? [];
  const setBlockState = useAppStore((s) => s.setBlockState);
  const busy = useAppStore((s) => s.busy);
  const sendBlockAction = useAppStore((s) => s.sendBlockAction);

  const allTags = useMemo(() => {
    const set = new Set<string>();
    for (const it of items) for (const t of asArr2(it.tags)) set.add(t);
    return [...set];
  }, [items]);

  const shown = activeTags.length
    ? items.filter((it) => activeTags.every((t) => asArr2(it.tags).includes(t)))
    : items;

  const toggleTag = (t: string) => {
    const next = activeTags.includes(t) ? activeTags.filter((x) => x !== t) : [...activeTags, t];
    setBlockState(block.id, { ...state, tags: next });
  };

  return (
    <Frame title={block.title} kind="collection" footer={`${shown.length} of ${items.length} shown`}>
      {allTags.length > 0 && (
        <div className="col-filters" role="group" aria-label="Filter">
          {allTags.map((t) => (
            <button
              key={t}
              className={`col-chip${activeTags.includes(t) ? " on" : ""}`}
              aria-pressed={activeTags.includes(t)}
              onClick={() => toggleTag(t)}
            >
              {t}
            </button>
          ))}
        </div>
      )}
      <ul className="col-list">
        {shown.map((it) => {
          const id = String(it.id);
          return (
            <li key={id} className="col-item">
              <div className="col-main">
                <button
                  className="col-choose"
                  disabled={busy}
                  onClick={() =>
                    sendBlockAction(block.id, `Chose “${it.title ?? id}” from ${block.title}`, {
                      a: "block_action",
                      block: block.id,
                      kind: "collection",
                      action: "select",
                      item: id,
                      title: it.title ?? id,
                    })
                  }
                >
                  {it.title ?? id}
                </button>
                {it.subtitle && <span className="col-sub">{it.subtitle}</span>}
                {it.url && (
                  <a className="col-link" href={String(it.url)} target="_blank" rel="noreferrer" aria-label="Open link">
                    ↗
                  </a>
                )}
              </div>
              {it.tags && <div className="col-tags">{asArr2(it.tags).join(" · ")}</div>}
            </li>
          );
        })}
      </ul>
    </Frame>
  );
}

// ---- plan ----

const STATUS_GLYPH: Record<string, string> = {
  done: "●",
  doing: "◐",
  todo: "○",
  blocked: "⊘",
};

function PlanBlock({ block }: { block: BlockView }) {
  const data = asObj(block.data);
  const steps = asArr(data.steps);
  const state = asObj(block.state);
  const checked = asObj(state.checked);
  const busy = useAppStore((s) => s.busy);
  const sendBlockAction = useAppStore((s) => s.sendBlockAction);
  const setBlockState = useAppStore((s) => s.setBlockState);

  // W2: the user's local check-offs overlay the model-provided status, so a
  // click lands instantly without a model round-trip.
  const statusOf = (step: Obj) => String(checked[String(step.id)] ?? step.status ?? "todo");
  const doneCount = steps.filter((s) => statusOf(s) === "done").length;

  const check = (step: Obj) => {
    const id = String(step.id);
    setBlockState(block.id, { ...state, checked: { ...checked, [id]: "done" } });
    sendBlockAction(block.id, `Marked “${step.label ?? id}” done in ${block.title}`, {
      a: "block_action",
      block: block.id,
      kind: "plan",
      action: "set_step",
      step: id,
      status: "done",
    });
  };

  return (
    <Frame
      title={block.title}
      kind="plan"
      footer={steps.length ? `${doneCount} of ${steps.length} done` : undefined}
    >
      <ul className="plan-list" role="list">
        {steps.map((step) => {
          const id = String(step.id);
          const status = statusOf(step);
          const label = `${status}: ${step.label ?? id}`;
          const clickable = !busy && (status === "todo" || status === "doing");
          return (
            <li key={id} className={`plan-step ${status}`} aria-label={label}>
              <button
                className="plan-dot"
                disabled={!clickable}
                aria-label={clickable ? `Mark ${step.label ?? id} done` : label}
                onClick={() => clickable && check(step)}
              >
                {STATUS_GLYPH[status] ?? "○"}
              </button>
              <span className="plan-label">{step.label ?? id}</span>
              {(step.detail || status === "blocked") && (
                <span className="plan-detail">
                  {status === "blocked" ? `blocked: ${step.detail ?? "—"}` : step.detail}
                </span>
              )}
            </li>
          );
        })}
      </ul>
    </Frame>
  );
}

// ---- form ----

function FormBlock({ block }: { block: BlockView }) {
  const data = asObj(block.data);
  const fields = asArr(data.fields);
  const state = asObj(block.state);
  const submitted = !!state.submitted;
  const setBlockState = useAppStore((s) => s.setBlockState);
  const busy = useAppStore((s) => s.busy);
  const sendBlockAction = useAppStore((s) => s.sendBlockAction);
  const [values, setValues] = useState<Obj>(() => asObj(state.values));

  const setField = (id: string, v: unknown) => {
    const next = { ...values, [id]: v };
    setValues(next);
    setBlockState(block.id, { ...state, values: next });
  };

  const missingRequired = fields.some(
    (f) => f.required && (values[String(f.id)] === undefined || values[String(f.id)] === "")
  );

  const submit = () => {
    setBlockState(block.id, { ...state, values, submitted: true });
    const summary = fields
      .map((f) => `${f.label ?? f.id}: ${formatCell(values[String(f.id)])}`)
      .filter((s) => !s.endsWith(": —"))
      .join("; ");
    sendBlockAction(block.id, `Submitted the form “${block.title}” · ${summary}`, {
      a: "block_action",
      block: block.id,
      kind: "form",
      action: "submit",
      data: values,
    });
  };

  return (
    <Frame title={block.title} kind="form" footer={submitted ? "submitted" : undefined}>
      <div className="form-fields">
        {fields.map((f) => {
          const id = String(f.id);
          const type = String(f.type ?? "text");
          const options = optionsOf(f.options);
          return (
            <div className="form-field" key={id}>
              <label className="form-label" htmlFor={`${block.id}-${id}`}>
                {f.label ?? id}
                {f.required && <span className="form-req" aria-hidden="true"> *</span>}
              </label>
              {submitted ? (
                <span className="form-frozen">{formatCell(shownValue(values[id], options))}</span>
              ) : (
                <FieldInput
                  fieldId={`${block.id}-${id}`}
                  type={type}
                  options={options}
                  value={values[id]}
                  onChange={(v) => setField(id, v)}
                />
              )}
            </div>
          );
        })}
      </div>
      {!submitted && (
        <div className="form-actions">
          <button className="btn-primary" disabled={busy || missingRequired} onClick={submit}>
            {data.submit_label ?? "Send"}
          </button>
        </div>
      )}
    </Frame>
  );
}

function FieldInput({
  fieldId,
  type,
  options,
  value,
  onChange,
}: {
  fieldId: string;
  type: string;
  options: FormOption[];
  value: unknown;
  onChange: (v: unknown) => void;
}) {
  if (type === "toggle") {
    return (
      <input
        id={fieldId}
        type="checkbox"
        className="form-toggle"
        checked={!!value}
        onChange={(e) => onChange(e.target.checked)}
      />
    );
  }
  if (type === "select") {
    return (
      <select id={fieldId} className="field-input form-select" value={String(value ?? "")} onChange={(e) => onChange(e.target.value)}>
        <option value="" />
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    );
  }
  if (type === "multiselect") {
    const arr = Array.isArray(value) ? (value as string[]) : [];
    return (
      <div className="form-multi">
        {options.map((o) => (
          <label key={o.value} className="form-multi-opt">
            <input
              type="checkbox"
              checked={arr.includes(o.value)}
              onChange={(e) => onChange(e.target.checked ? [...arr, o.value] : arr.filter((x) => x !== o.value))}
            />
            {o.label}
          </label>
        ))}
      </div>
    );
  }
  return (
    <input
      id={fieldId}
      type={type === "number" ? "number" : "text"}
      className="field-input form-input"
      value={String(value ?? "")}
      onChange={(e) => onChange(type === "number" ? e.target.valueAsNumber : e.target.value)}
    />
  );
}

// ---- progress ----

function ProgressBlock({ block }: { block: BlockView }) {
  const data = asObj(block.data);
  const current = Number(data.current ?? 0);
  const total = Number(data.total ?? 0);
  const status = String(data.status ?? "running");
  const unit = data.unit ? ` ${data.unit}` : "";
  const pct = total > 0 ? Math.max(0, Math.min(1, current / total)) : 0;

  if (status === "done") {
    return (
      <Frame title={block.title} kind="progress">
        <div className="prog-done">✓ {data.label ?? `Done — ${current}${unit}`}</div>
      </Frame>
    );
  }
  return (
    <Frame
      title={block.title}
      kind="progress"
      footer={status === "error" ? <span className="prog-error">{data.note ?? "error"}</span> : data.note}
    >
      <div className="prog-row" aria-label={`${data.label ?? "Progress"}: ${current} of ${total}`}>
        <div className="prog-track" aria-hidden="true">
          <div className="prog-fill" style={{ width: `${pct * 100}%` }} />
        </div>
        <span className="prog-count">
          {current} / {total}
          {unit}
        </span>
      </div>
    </Frame>
  );
}

// ---- document ----

function DocumentBlock({ block }: { block: BlockView }) {
  const data = asObj(block.data);
  const artifactId = data.artifact_id as string | undefined;
  const openArtifact = useAppStore((s) => s.openArtifact);
  const artifact = useAppStore((s) => {
    const convId = s.activeConversationId;
    if (!convId || !artifactId) return undefined;
    return (s.artifacts[convId] ?? []).find((a) => a.id === artifactId);
  });

  const desc = artifact?.content?.split("\n").find((l) => l.trim()) ?? block.title;
  return (
    <Frame title={block.title} kind="document">
      <div className="doc-row">
        <span className="doc-desc">{desc}</span>
        {artifactId && (
          <button className="doc-open" onClick={() => openArtifact(artifactId)}>
            Open →
          </button>
        )}
      </div>
    </Frame>
  );
}

// ---- table (DAT-3) ----

function TableBlock({ block }: { block: BlockView }) {
  const data = asObj(block.data);
  const columns = Array.isArray(data.columns) ? (data.columns as unknown[]) : [];
  const rows = Array.isArray(data.rows) ? (data.rows as unknown[][]) : [];

  return (
    <Frame title={block.title} kind="table" footer={`${rows.length} row${rows.length === 1 ? "" : "s"}`}>
      <div className="tbl-scroll">
        <table className="tbl-table">
          <thead>
            <tr>
              {columns.map((c, i) => (
                <th key={i} className="tbl-label">
                  {String(c)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <tr key={i} className="tbl-row">
                {row.map((cell, j) => (
                  <td key={j} className="tbl-cell">
                    {String(cell)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Frame>
  );
}

// ---- fallback ----

function RawBlock({ block }: { block: BlockView }) {
  return (
    <Frame title={block.title || "Block"} kind={block.kind || "unknown"}>
      <details className="block-raw">
        <summary>Unrenderable block — raw data</summary>
        <pre>{JSON.stringify(block.data, null, 2)}</pre>
      </details>
    </Frame>
  );
}

class BlockErrorBoundary extends Component<
  { block: BlockView; children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  render() {
    if (this.state.failed) return <RawBlock block={this.props.block} />;
    return this.props.children;
  }
}

// ---- helpers ----

/** Array-of-strings from a possibly-loose value. */
function asArr2(x: unknown): string[] {
  if (Array.isArray(x)) return x.map((v) => String(v));
  if (x == null) return [];
  return [String(x)];
}

/** A select option as the model may write it: a bare string, or an object with
 * `value` / `id` and `label` / `title` / `name`. */
interface FormOption {
  value: string;
  label: string;
}

function optionsOf(x: unknown): FormOption[] {
  if (!Array.isArray(x)) return [];
  return x.map((o) => {
    if (o && typeof o === "object") {
      const n = o as Obj;
      const value = String(n.value ?? n.id ?? n.label ?? n.title ?? n.name ?? "");
      return { value, label: String(n.label ?? n.title ?? n.name ?? value) };
    }
    return { value: String(o), label: String(o) };
  });
}

/** A submitted value as the person saw it: an option's label, not its id. */
function shownValue(v: unknown, options: FormOption[]): unknown {
  const label = (x: unknown) => options.find((o) => o.value === x)?.label ?? x;
  return Array.isArray(v) ? v.map(label) : label(v);
}

function formatCell(v: unknown): string {
  if (v === undefined || v === null || v === "") return "—";
  if (Array.isArray(v) && v.length === 0) return "—";
  if (typeof v === "boolean") return v ? "yes" : "no";
  if (Array.isArray(v)) return v.map(String).join(", ");
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}
