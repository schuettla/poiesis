import { useCallback, useEffect, useState } from "react";
import {
  decideCloudCatalog,
  decideCloudProbe,
  decideCloudSelect,
  decideCloudStatus,
  inTauri,
  setDecideBackend,
  type CloudDecisionCatalog,
  type CloudDecisionModel,
  type CloudDecisionStatus,
} from "../../lib/api";
import { useAppStore } from "../../lib/store";
import "./DecisionModel.css";

/** Tokens as the catalog screens write them: 16k, 64k, 1.1M. 0 is "not listed". */
export function contextText(tokens: number): string {
  if (tokens <= 0) return "context not listed";
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
  return `${Math.round(tokens / 1024)}k`;
}

/** The price of the input, which is all there is to pay (`DEC-5a`: output is free). */
export function priceText(m: Pick<CloudDecisionModel, "free" | "price_per_mtok">): string {
  if (m.free) return "free";
  if (m.price_per_mtok === null) return "price not listed";
  return `$${m.price_per_mtok.toFixed(2)} / M`;
}

/** About what 1,000 decisions of roughly 1,000 tokens each cost. */
export function costPer1000(m: Pick<CloudDecisionModel, "free" | "price_per_mtok"> | undefined): string | null {
  if (!m || m.price_per_mtok === null) return null;
  if (m.free) return "free";
  return `~$${m.price_per_mtok.toFixed(2)} per 1,000 decisions`;
}

/** What goes in: text, or text and pictures. */
export function inputText(m: Pick<CloudDecisionModel, "images">): string {
  return m.images ? "text + images" : "text";
}

/** Whether a vendor's own picture input is one the screen should call experimental.
 * Jev's pictures are, by its maker's word (`DEC-8`). */
export function imagesExperimental(m: Pick<CloudDecisionModel, "id" | "images">): boolean {
  return m.images && m.id.startsWith("typesafe/");
}

/** What the last Test found about pictures, or what the catalog promises. */
export function visionText(m: CloudDecisionModel): string | null {
  if (!m.images) return null;
  if (m.probe?.ok && m.probe.vision === true) return "images ✓";
  if (m.probe?.ok && m.probe.vision === false) return "images: not accepted via OpenRouter";
  return "images listed, not tested";
}

/** `HH:MM`, in the person's own clock. */
function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** `DEC-UI-2`'s cloud line: a tone and the words for it. */
export function cloudLine(
  s: CloudDecisionStatus | null
): { tone: "ok" | "warn" | "bad" | "idle"; text: string } | null {
  if (!s || s.backend !== "cloud") return null;
  switch (s.state) {
    case "no-key":
      return { tone: "bad", text: "No OpenRouter key. Using this computer." };
    case "key":
      return { tone: "bad", text: "OpenRouter key rejected. Using this computer." };
    case "credits":
      return { tone: "warn", text: "Out of OpenRouter credits, using this computer." };
    case "overloaded":
      return {
        tone: "warn",
        text: `Cloud unavailable (overloaded), using this computer${s.until ? ` until ${clock(s.until)}` : ""}.`,
      };
    case "no-endpoint":
      return { tone: "warn", text: "No provider of this model keeps nothing, using this computer." };
    default:
      return { tone: "ok", text: `Ready${s.median_ms !== null ? ` · ${s.median_ms} ms` : ""}` };
  }
}

/** The cloud's state, re-read every few seconds while it is the chosen backend. */
export function useCloudStatus() {
  const [status, setStatus] = useState<CloudDecisionStatus | null>(null);
  const refresh = useCallback(async () => {
    if (!inTauri()) return;
    try {
      setStatus(await decideCloudStatus());
    } catch {
      /* the line stays as it was */
    }
  }, []);
  useEffect(() => {
    void refresh();
    const id = window.setInterval(refresh, 5000);
    return () => window.clearInterval(id);
  }, [refresh]);
  return { status, refresh };
}

/** `DEC-UI-1`: the cloud models in the Decision model tab. "Use this one" is the
 * whole choice: it picks the model and runs quick decisions in the cloud with it.
 * There is no second switch anywhere else. Going back to this computer is "Use this
 * one" on a local model. Only a click here sends anything, and only to providers
 * that keep nothing. */
export function CloudModels() {
  const setView = useAppStore((s) => s.setView);
  const cloud = useCloudStatus();
  const [cat, setCat] = useState<CloudDecisionCatalog | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (refresh = false) => {
    if (!inTauri()) return;
    try {
      setCat(await decideCloudCatalog(refresh));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function test(slug: string) {
    setBusy(`test:${slug}`);
    setError(null);
    try {
      await decideCloudProbe(slug);
    } catch (e) {
      setError(String(e));
    } finally {
      await load();
      setBusy(null);
    }
  }

  async function pick(m: CloudDecisionModel) {
    setBusy(`pick:${m.slug}`);
    setError(null);
    try {
      await decideCloudSelect(m.slug);
      await setDecideBackend("cloud");
      await load();
      // The first time a model is chosen it is tested, so the row says what it can do.
      if (!m.probe) await decideCloudProbe(m.slug);
    } catch (e) {
      setError(String(e));
    } finally {
      await load();
      await cloud.refresh();
      setBusy(null);
    }
  }

  if (!inTauri() || !cat) return null;

  if (!cat.key_set) {
    return (
      <div className="decide-cloud">
        <h2 className="section-title">
          <span className="provenance-dot cloud" aria-hidden="true" /> Via OpenRouter
        </h2>
        <p className="add-help">
          Add an OpenRouter key in Providers to use cloud decision models.{" "}
          <button className="link-button inline" onClick={() => setView("providers")}>
            Open Providers →
          </button>
        </p>
      </div>
    );
  }

  const inCloud = cat.backend === "cloud";
  const shown = showAll ? cat.models : cat.models.filter((m) => m.featured || m.selected);
  const hidden = cat.models.length - shown.length;
  const line = cloudLine(cloud.status);
  const active = cat.models.find((m) => m.selected);

  return (
    <div className="decide-cloud">
      <div className="section-head">
        <h2 className="section-title">
          <span className="provenance-dot cloud" aria-hidden="true" /> Via OpenRouter
        </h2>
        <span className="disk-total">
          {(showAll || hidden > 0) && (
            <button className="link-button inline" aria-expanded={showAll} onClick={() => setShowAll((a) => !a)}>
              {showAll ? "Show fewer" : `Show all ${cat.models.length}`}
            </button>
          )}{" "}
          <button
            className="btn-text"
            disabled={busy !== null}
            title="Read OpenRouter's list again"
            onClick={() => void load(true)}
          >
            ↻ {cat.fetched_at ? `read ${new Date(cat.fetched_at).toLocaleDateString()}` : "read now"}
          </button>
        </span>
      </div>
      <div className="model-rows">
        {shown.map((m) => {
          const vision = visionText(m);
          const blocked = !m.zdr;
          const inUse = m.selected && inCloud;
          return (
            <div className="model-row-wrap" key={m.slug}>
              <div className={`model-row ${inUse ? "in-use" : ""} ${blocked ? "unavailable" : ""}`}>
                <span className="provenance-dot cloud" aria-hidden="true" />
                <span className="model-row-name" title={m.slug}>
                  {m.name}
                </span>
                <span className="model-row-chips">
                  <span className="cap-chip">{inputText(m)}</span>
                  <span className="cap-chip">{contextText(m.context)}</span>
                  <span className="cap-chip">{priceText(m)}</span>
                  {imagesExperimental(m) && <span className="cap-chip">images: experimental</span>}
                </span>
                <span className="model-row-cost">
                  {blocked
                    ? "no zero-retention provider"
                    : [m.same_as_local ? "same as local ✓" : null, vision].filter(Boolean).join(" · ")}
                </span>
                <span className="model-row-actions">
                  {inUse ? (
                    <span className="default-tag">In use</span>
                  ) : (
                    <button className="btn-text" disabled={busy !== null || blocked} onClick={() => void pick(m)}>
                      {busy === `pick:${m.slug}` ? "Testing…" : "Use this one"}
                    </button>
                  )}
                  {m.selected && (
                    <button className="btn-text" disabled={busy !== null} onClick={() => void test(m.slug)}>
                      {busy === `test:${m.slug}` ? "Testing…" : "Test"}
                    </button>
                  )}
                </span>
              </div>
              {m.selected && m.probe && (
                <p className={`decide-probe ${m.probe.ok ? "" : "bad"}`}>
                  {m.probe.ok
                    ? `Tested · answered in ${m.probe.ms} ms`
                    : `Test failed: ${m.probe.error ?? "no answer"}`}
                </p>
              )}
            </div>
          );
        })}
      </div>
      {hidden > 0 && <p className="add-help">{hidden} more are hidden.</p>}
      {inCloud && line && <p className={`decide-cloud-line ${line.tone}`}>{line.text}</p>}
      <p className="add-help">
        {inCloud
          ? `Quick decisions send the question and a short excerpt to OpenRouter. ${costPer1000(active) ?? ""} `
          : ""}
        Every question goes only to providers that keep nothing. Questions that run on their own, like
        mail and reflection, stay on this computer either way.
      </p>
      {cat.error && <p className="hw-note error">{cat.error}</p>}
      {error && <p className="hw-note error">{error}</p>}
    </div>
  );
}
