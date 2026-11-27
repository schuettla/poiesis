import { useRef } from "react";
import Ticker from "./Ticker";
import { useThrottled } from "../../lib/useThrottled";

/** Longer than one flap (`--dur`), so each digit lands before it turns again. */
export const ROLL_STEP_MS = 300;

/** A number, grouping and decimal marks included: "5", "1,247", "0.04". An ordinary space
 * ends it, so "2 of 5" is two numbers. */
const NUMBER = /(\d+(?:[.,'’  ]\d+)*)/;

/** One cell to a character. Cells are keyed from the right, so the units stay the units
 * when the number gets longer. */
function Digits({ text, quiet }: { text: string; quiet: boolean }) {
  const chars = [...text];
  return (
    <span className="roll" role="img" aria-label={text}>
      {chars.map((c, i) => (
        <span className="roll-cell" key={chars.length - i}>
          <Ticker text={c} quiet={quiet} />
        </span>
      ))}
    </span>
  );
}

/** A line with numbers in it: each digit that changes flaps over, like a departure board.
 * The words stand still, and so does a line that has not changed since it was drawn. */
export function RollingText({ text }: { text: string }) {
  const first = useRef(text);
  const changed = useRef(false);
  if (text !== first.current) changed.current = true;
  return (
    <>
      {text.split(NUMBER).map((part, i) =>
        // The split puts the numbers at the odd places.
        i % 2 ? <Digits key={i} text={part} quiet={!changed.current} /> : part
      )}
    </>
  );
}

/** A number that grows while you watch. A fast stream is read in steps (`ROLL_STEP_MS`),
 * so it counts up instead of blurring. */
export default function RollingNumber({ value }: { value: number }) {
  return <RollingText text={useThrottled(value, ROLL_STEP_MS).toLocaleString()} />;
}
