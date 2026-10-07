import { ThinkingOrb, type OrbState } from "thinking-orbs";
import { useAppStore } from "../../lib/store";

/**
 * An activity orb. The library's `theme="auto"` looks for `data-theme` or a
 * `dark` class, and this app sets neither (it sets `data-mode`), so the theme
 * is passed from the store instead of being guessed.
 *
 * Always decorative: every place that shows one has the same fact in words
 * beside it, and a second `img` label would make a screen reader say it twice.
 */
export default function Orb({
  state,
  size = 20,
  paused,
}: {
  state: OrbState;
  size?: 20 | 64;
  paused?: boolean;
}) {
  const mode = useAppStore((s) => s.mode);
  return <ThinkingOrb state={state} size={size} theme={mode} paused={paused} aria-hidden="true" />;
}
