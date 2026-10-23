import "./Toggle.css";

/**
 * The app's on/off switch. A real checkbox underneath (`role="switch"`), so the
 * keyboard, a wrapping `<label>` and screen readers behave as they do for any
 * checkbox; only the drawing is different. It carries no text of its own: put it
 * inside a label that does.
 */
export default function Toggle({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (on: boolean) => void;
  disabled?: boolean;
  /** Only when no wrapping label names it. */
  label?: string;
}) {
  return (
    <span className="toggle">
      <input
        className="toggle-input"
        type="checkbox"
        role="switch"
        checked={checked}
        disabled={disabled}
        aria-label={label}
        onChange={(ev) => onChange(ev.target.checked)}
      />
      <span className="toggle-track" aria-hidden="true">
        <span className="toggle-knob" />
      </span>
    </span>
  );
}
