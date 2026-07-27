import { useAppStore } from "../lib/store";
import SelfPanel, { useSelfRefresh } from "../components/Self/SelfPanel";
import GrowthRings from "../components/Self/GrowthRings";
import "./Surface.css";

const DAY = 86400_000;

/** The first-person narrative above the tabs (PRES-3). Plain sentences, not
 * statistics: the organism describing itself. */
function narrative(
  days: number,
  facts: number,
  lessons: number,
  recipes: number
): string {
  if (facts === 0 && lessons === 0 && recipes === 0) {
    return "I'm new here. As we work together I'll start remembering, learning, and keeping procedures — it all lands on this page, in plain files you own.";
  }
  const span = days < 1 ? "less than a day" : `${days} day${days === 1 ? "" : "s"}`;
  return (
    `I've been growing for ${span}. I know ${facts} thing${facts === 1 ? "" : "s"} about you, ` +
    `I've learned ${lessons} lesson${lessons === 1 ? "" : "s"} from my own mistakes, and I keep ` +
    `${recipes} procedure${recipes === 1 ? "" : "s"} we developed together.`
  );
}

/**
 * The Self view (PRES-3): a place, not a settings tab. Everything Poiesis is
 * made of — what it remembers, what it learned, the procedures it keeps, how
 * it's running, and how much freedom it has — reached from its own rail
 * destination and described in its own voice.
 */
export default function Self() {
  useSelfRefresh();
  const vitality = useAppStore((s) => s.vitality);
  const born = useAppStore((s) => s.selfBorn);
  const lessons = useAppStore((s) => s.lessons);
  const recipes = useAppStore((s) => s.recipes);

  const facts = vitality?.facts ?? 0;
  const lessonCount = vitality?.lessons ?? 0;
  const recipeCount = vitality?.recipes ?? 0;
  const pending = vitality?.pending_proposals ?? 0;
  const days = born ? Math.floor((Date.now() - born) / DAY) : 0;

  // Rings are drawn from what actually exists on disk, dated by its own
  // frontmatter — lessons and recipes here, facts via their created dates.
  const entries = [
    ...lessons.map((l) => ({ created: l.created })),
    ...recipes.map((r) => ({ created: r.created })),
  ];

  return (
    <div className="surface">
      <div className="surface-inner">
        <header className="self-header">
          <div className="self-narrative">
            <h1>Self</h1>
            <p className="lede">{narrative(days, facts, lessonCount, recipeCount)}</p>
            <p className="self-strip">
              {facts} facts · {lessonCount} lessons · {recipeCount} recipes · {pending} pending
            </p>
          </div>
          {born && <GrowthRings entries={entries} born={born} />}
        </header>

        <SelfPanel />
      </div>
    </div>
  );
}
