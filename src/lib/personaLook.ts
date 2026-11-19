/**
 * `TEAM-2`: how a persona is drawn: a letter in a circle, one of 8 muted hues
 * (`--persona-1` .. `--persona-8` in `tokens.css`). Made from the name, so a persona
 * looks the same wherever it appears and nothing has to be chosen. A hue only tells
 * personas apart; it never means good or bad.
 */
export const PERSONA_HUES = 8;

export interface PersonaLook {
  letter: string;
  /** 1 to `PERSONA_HUES`. */
  hue: number;
}

/** The look a persona has: the one it was given, or the one its name makes. `stored` is the
 * JSON `personas.look` holds. Anything that does not read as a letter and a hue is ignored. */
export function lookOf(p: { name: string; look?: string | null }): PersonaLook {
  if (p.look) {
    try {
      const v = JSON.parse(p.look) as Partial<PersonaLook>;
      const letter = typeof v.letter === "string" ? Array.from(v.letter.trim())[0] : undefined;
      if (letter && typeof v.hue === "number" && v.hue >= 1 && v.hue <= PERSONA_HUES) {
        return { letter: letter.toUpperCase(), hue: Math.round(v.hue) };
      }
    } catch {
      /* made from the name */
    }
  }
  return personaLook(p.name);
}

export function personaLook(name: string): PersonaLook {
  const trimmed = name.trim();
  const letter = (Array.from(trimmed)[0] ?? "?").toUpperCase();
  let sum = 0;
  for (const ch of trimmed.toLowerCase()) sum += ch.codePointAt(0) ?? 0;
  return { letter, hue: (sum % PERSONA_HUES) + 1 };
}
