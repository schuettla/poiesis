import { useLayoutEffect, useRef, useState } from "react";
import { readPhoto } from "../photo";
import type { Photo } from "../store";
import { words } from "../link";
import { CloseIcon, ImageIcon, MicIcon, SendIcon, StopIcon } from "./Icons";

/** Tallest the field grows before it scrolls. */
const MAX_FIELD = 140;

/** The floating box at the bottom of a chat: photo, text, and one button on the right that changes with what makes
 * sense: talk when empty, send when there is something, stop while Poiesis works. */
export default function Composer({
  disabled,
  reason,
  busy,
  onSend,
  onStop,
  onVoice,
}: {
  disabled: boolean;
  reason: string;
  busy: boolean;
  onSend: (text: string, photos: Photo[]) => void;
  onStop: () => void;
  /** Start talking. */
  onVoice: () => void;
}) {
  const [text, setText] = useState("");
  const [photos, setPhotos] = useState<Photo[]>([]);
  const [problem, setProblem] = useState<string | null>(null);
  const file = useRef<HTMLInputElement>(null);
  const field = useRef<HTMLTextAreaElement>(null);

  // The field grows with what is typed, up to a few lines.
  useLayoutEffect(() => {
    const el = field.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, MAX_FIELD)}px`;
  }, [text]);

  async function picked(list: FileList | null) {
    if (!list?.length) return;
    setProblem(null);
    try {
      const next = await Promise.all(Array.from(list).slice(0, 4 - photos.length).map(readPhoto));
      setPhotos((p) => [...p, ...next].slice(0, 4));
    } catch (e) {
      setProblem(words(e));
    }
    if (file.current) file.current.value = "";
  }

  const filled = !!text.trim() || photos.length > 0;

  function send() {
    if (disabled || !filled) return;
    onSend(text, photos);
    setText("");
    setPhotos([]);
  }

  return (
    <footer className="composer-dock">
      {(disabled || problem) && <p className="composer-reason">{disabled ? reason : problem}</p>}
      <div className={`composer${disabled ? " is-off" : ""}`}>
        {photos.length > 0 && (
          <ul className="composer-photos">
            {photos.map((p, i) => (
              <li key={p.name + i}>
                <img src={p.dataUri} alt={p.name} />
                <button aria-label={`Remove ${p.name}`} onClick={() => setPhotos((all) => all.filter((_, j) => j !== i))}>
                  <CloseIcon size={12} />
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="composer-row">
          <input ref={file} type="file" accept="image/*" multiple hidden onChange={(e) => void picked(e.target.files)} />
          <button
            className="round-btn quiet"
            aria-label="Add a photo"
            disabled={disabled || photos.length >= 4}
            onClick={() => file.current?.click()}
          >
            <ImageIcon size={22} />
          </button>
          <textarea
            ref={field}
            value={text}
            rows={1}
            placeholder={busy ? "Add to what Poiesis is doing" : "Ask Poiesis"}
            disabled={disabled}
            onChange={(e) => setText(e.target.value)}
          />
          {filled ? (
            <button className="round-btn solid" aria-label={busy ? "Add to the running task" : "Send"} disabled={disabled} onClick={send}>
              <SendIcon size={20} />
            </button>
          ) : busy ? (
            <button className="round-btn outline" aria-label="Stop" disabled={disabled} onClick={onStop}>
              <StopIcon size={18} />
            </button>
          ) : (
            <button className="round-btn solid" aria-label="Talk to Poiesis" disabled={disabled} onClick={onVoice}>
              <MicIcon size={22} />
            </button>
          )}
        </div>
      </div>
    </footer>
  );
}
