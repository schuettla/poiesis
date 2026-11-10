import { useEffect, useState } from "react";
import { useAppStore } from "../../lib/store";
import { activeStackLabel } from "../Context/ContextChip";
import { ChevronIcon, CloseIcon, LayersIcon, LayoutIcon, PersonIcon } from "../Icons/Icons";
import StyleGlyph from "../Mark/StyleGlyph";
import { chipTitle, useModeChips, type ModeChip } from "./ModeChips";
import { useStyleState } from "./styleState";

/** The standing state that lives in this menu. What takes something away from me
 * (tools, tier, external agents) stays a chip under the composer instead. */
const IN_MENU: ModeChip["key"][] = ["workspace", "persona"];

/**
 * How this chat is set up, as one icon button under the composer, the same size
 * and shape as `+` and `/` beside it. It holds three things that used to be three
 * separate pieces of text in the footer: how I work (`WRK-3`), what I'm working
 * from (`WHY-4`), and the standing modes Workspace and Persona (`CMP-7`).
 *
 * The button draws the current way of working's glyph. It is lit, like `+` with
 * something waiting, whenever anything here is not at its default, so the state
 * can be read without opening it. Everything is named in full on the hover.
 */
export default function ChatSetup() {
  const style = useStyleState();
  const chips = useModeChips().filter((c) => IN_MENU.includes(c.key));
  const stack = useAppStore(activeStackLabel);
  const openContextPanel = useAppStore((s) => s.openContextPanel);
  const requestComposer = useAppStore((s) => s.requestComposer);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  if (!style.convId) return null;
  const convId = style.convId;
  const lit = style.chosen || chips.length > 0;
  const summary = [
    `How I work: ${style.shown}`,
    `Working from: ${stack}`,
    ...chips.map((c) => (c.key === "persona" ? `Persona: ${c.label}` : c.label)),
  ];
  const close = () => setOpen(false);

  return (
    <div className="composer-menu-wrap">
      <button
        className={`icon-btn chat-setup-btn${open ? " on" : ""}${lit ? " lit" : ""}`}
        aria-label={`This chat. ${summary.join(". ")}`}
        aria-haspopup="menu"
        aria-expanded={open}
        title={summary.join("\n")}
        onClick={() => setOpen((v) => !v)}
      >
        <StyleGlyph glyph={style.glyph} branch={style.branch} size={17} />
      </button>
      {open && (
        <>
          <div className="composer-menu-backdrop" onClick={close} />
          <div className="composer-menu chat-setup-menu" role="menu" aria-label="This chat">
            <button
              className="composer-menu-item"
              role="menuitem"
              title={`${style.line} · ${style.where}`}
              onClick={() => {
                close();
                style.change();
              }}
            >
              <span className="mi-icon" aria-hidden="true">
                <StyleGlyph glyph={style.glyph} branch={style.branch} size={15} />
              </span>
              <span className="mi-body">
                How I work
                <span className="mi-hint">
                  {style.shown} · {style.line}
                </span>
              </span>
              <span className="mi-more" aria-hidden="true">
                <ChevronIcon dir="right" size={11} strokeWidth={1.6} />
              </span>
            </button>
            <button
              className="composer-menu-item"
              role="menuitem"
              title="See what I'm working from, layer by layer"
              onClick={() => {
                close();
                openContextPanel({ conversationId: convId });
              }}
            >
              <span className="mi-icon" aria-hidden="true">
                <LayersIcon size={15} />
              </span>
              <span className="mi-body">
                What I'm working from
                <span className="mi-hint">{stack}</span>
              </span>
              <span className="mi-more" aria-hidden="true">
                <ChevronIcon dir="right" size={11} strokeWidth={1.6} />
              </span>
            </button>
            {chips.length > 0 && (
              <>
                <div className="composer-menu-sep" />
                <div className="composer-menu-label">On in this chat</div>
                {chips.map((c) => (
                  <div className="chat-setup-row" key={c.key}>
                    <button
                      className="composer-menu-item"
                      role="menuitem"
                      title={chipTitle(c)}
                      onClick={() => {
                        close();
                        if (c.open) c.open.run();
                        else requestComposer(c.command);
                      }}
                    >
                      <span className="mi-icon" aria-hidden="true">
                        {c.key === "workspace" ? <LayoutIcon size={15} /> : <PersonIcon size={15} />}
                      </span>
                      <span className="mi-body">
                        {c.key === "persona" ? "Persona" : c.label}
                        <span className="mi-hint">
                          {c.key === "persona" ? [c.label, c.hint].filter(Boolean).join(" · ") : c.open?.title}
                        </span>
                      </span>
                    </button>
                    <button className="chat-setup-off" aria-label={c.offLabel} title={c.offLabel} onClick={c.off}>
                      <CloseIcon size={11} strokeWidth={1.6} />
                    </button>
                  </div>
                ))}
              </>
            )}
          </div>
        </>
      )}
    </div>
  );
}
