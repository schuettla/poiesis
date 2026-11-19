import { useEffect, useState } from "react";
import { useAppStore } from "../../lib/store";
import { activeStackLabel } from "../Context/ContextChip";
import { ChevronIcon, CloseIcon, LayersIcon, LayoutIcon } from "../Icons/Icons";
import { usePersonaAppsLine } from "../Apps/personaAppsLine";
import StyleGlyph from "../Mark/StyleGlyph";
import PersonaGlyph, { PoiesisGlyph, TeamGlyph } from "../Personas/PersonaGlyph";
import { chipTitle, useModeChips, type ModeChip } from "./ModeChips";
import { useLeaderState } from "./leaderState";
import { useStyleState } from "./styleState";

/** The standing state that lives in this menu. What takes something away from me
 * (tools, tier, external agents) stays a chip under the composer instead. */
const IN_MENU: ModeChip["key"][] = ["workspace"];

/**
 * How this chat is set up, as one icon button under the composer, the same size
 * and shape as `+` and `/` beside it. It holds what used to be separate pieces of
 * text in the footer: who leads (`TEAM-2`), how I work (`WRK-3`), what I'm working
 * from (`WHY-4`), and the standing mode Workspace (`CMP-7`).
 *
 * The button draws the current way of working's glyph, or, when a persona leads, the
 * persona's letter with that glyph as a small badge. It is lit, like `+` with
 * something waiting, whenever anything here is not at its default, so the state
 * can be read without opening it. Everything is named in full on the hover.
 */
export default function ChatSetup() {
  const style = useStyleState();
  const leader = useLeaderState();
  const appsLine = usePersonaAppsLine(leader.persona?.id);
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
  const lit = style.chosen || leader.led || chips.length > 0;
  // Poiesis leading is the plain state and says nothing, so a chat with no persona reads as it always did.
  const summary = [
    ...(leader.led ? [`Led by ${leader.name}${leader.about ? ` · ${leader.about}` : ""}`] : []),
    `How I work: ${style.shown}`,
    `Working from: ${stack}`,
    ...chips.map((c) => c.label),
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
        {leader.persona ? (
          <PersonaGlyph name={leader.name} look={leader.persona.look} size={17} badge={<StyleGlyph glyph={style.glyph} branch={false} size={11} />} />
        ) : leader.team ? (
          <TeamGlyph coordinator={leader.coordinator} size={17} badge={<StyleGlyph glyph={style.glyph} branch={false} size={11} />} />
        ) : (
          <StyleGlyph glyph={style.glyph} branch={style.branch} size={17} />
        )}
      </button>
      {open && (
        <>
          <div className="composer-menu-backdrop" onClick={close} />
          <div className="composer-menu chat-setup-menu" role="menu" aria-label="This chat">
            <button
              className="composer-menu-item"
              role="menuitem"
              title={[leader.where, appsLine].filter(Boolean).join(" · ")}
              onClick={() => {
                close();
                leader.change();
              }}
            >
              <span className="mi-icon" aria-hidden="true">
                {leader.persona ? (
                  <PersonaGlyph name={leader.name} look={leader.persona.look} size={15} />
                ) : leader.team ? (
                  <TeamGlyph coordinator={leader.coordinator} size={15} />
                ) : (
                  <PoiesisGlyph size={15} />
                )}
              </span>
              <span className="mi-body">
                Who leads
                <span className="mi-hint">{leader.name}</span>
              </span>
              <span className="mi-more" aria-hidden="true">
                <ChevronIcon dir="right" size={11} strokeWidth={1.6} />
              </span>
            </button>
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
                        <LayoutIcon size={15} />
                      </span>
                      <span className="mi-body">
                        {c.label}
                        <span className="mi-hint">{c.open?.title}</span>
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
