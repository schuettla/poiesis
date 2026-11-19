import type { ReactNode } from "react";
import "../Self/Ways.css";

export interface PickCard {
  id: string;
  glyph: ReactNode;
  name: string;
  line: string;
  /** A small mark at the top right: a badge or a dot. */
  mark?: ReactNode;
}

/**
 * The personas or the teams as cards, the way How I work shows the styles: a glyph, the name and
 * one line, the picked one outlined, and a dashed card that makes a new one. The picked one is
 * shown in full under the cards.
 */
export default function PickCards({
  label,
  cards,
  selected,
  onPick,
  newCard,
}: {
  label: string;
  cards: PickCard[];
  selected: string | null;
  onPick: (id: string) => void;
  newCard: { name: string; line: string; onClick: () => void; on?: boolean };
}) {
  return (
    <div className="ways-styles pick-cards" role="radiogroup" aria-label={label}>
      {cards.map((c) => (
        <button
          key={c.id}
          role="radio"
          aria-checked={selected === c.id}
          className={`ways-style${selected === c.id ? " on" : ""}`}
          title={c.line || undefined}
          onClick={() => onPick(c.id)}
        >
          <span className="ways-style-top">
            {c.glyph}
            {c.mark}
          </span>
          <span className="ways-style-name">{c.name}</span>
          <span className="ways-style-line">{c.line}</span>
        </button>
      ))}
      <button className={`ways-style new${newCard.on ? " on" : ""}`} onClick={newCard.onClick}>
        <span className="ways-style-plus" aria-hidden="true">
          +
        </span>
        <span className="ways-style-name">{newCard.name}</span>
        <span className="ways-style-line">{newCard.line}</span>
      </button>
    </div>
  );
}
