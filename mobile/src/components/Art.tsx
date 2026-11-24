import amphora from "../assets/09-amphora.webp";
import athena from "../assets/04-athena-olive.webp";
import lockup from "../assets/logo-lockup-light.png";
import penelope from "../assets/02-penelope.webp";

/** The desktop's own pictures (public/illustrations and public/brand), copied in: the phone imports nothing from
 * the desktop's folders. Each one stands for a moment: pairing is working side by side, home is the olive tree that
 * grows from its roots, a new chat is the loom. */
const PICTURES = {
  amphora: { src: amphora, alt: "A person and a machine working side by side, painted on a Greek amphora" },
  athena: { src: athena, alt: "An old olive tree above the city, its roots wide under the ground" },
  penelope: { src: penelope, alt: "Penelope weaving at her loom by lamplight" },
} as const;

export type Picture = keyof typeof PICTURES;

export default function Art({ name, className = "" }: { name: Picture; className?: string }) {
  const p = PICTURES[name];
  return <img className={`art ${className}`} src={p.src} alt={p.alt} draggable={false} />;
}

/** The mark with the name under it, for the loading screen. Black on white; the stylesheet turns it for dark mode. */
export function Lockup() {
  return <img className="lockup" src={lockup} alt="Poiesis" draggable={false} />;
}
