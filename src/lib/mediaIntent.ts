import type { Attachment } from "./types";

/** What a bare (undeclared) message is probably asking for (`PIK-3`). Only
 * ever consulted when a *chat* model is selected — a declaration through the
 * model chooser (`PIK-2`) always wins and this never runs against it. */
export type MediaIntent = "chat" | "image" | "video" | "edit";

// Anchored at the start (`^`): the verb has to be the sentence's own
// imperative for this to count. "how do I draw a fox in Illustrator?" fails
// here — "how" is first — which is what keeps a genuine question out of the
// chip without needing an object noun.
//
// Two tiers of verb. VISUAL_VERB is unambiguously about making a picture —
// "draw", "paint", "sketch" — so it stands on its own. GENERIC_VERB ("create",
// "make", "generate", "erstelle") is the bulk of *every* request ("create a
// component", "make a plan"), so on its own it means nothing: it only counts as
// media intent when the sentence also names something visual (IMAGE_NOUN) or a
// clip (VIDEO_NOUN).
const VISUAL_VERB = /^(draw|paint|sketch|illustrate|render|zeichne|male|skizziere)\b/i;
const GENERIC_VERB = /^(generate|create|make|erstelle|generiere)\b/i;
const VIDEO_NOUN = /\b(video|clip|animation|film|movie|gif|reel|footage|animate)\b/i;
const IMAGE_NOUN =
  /\b(image|images|picture|pic|photo|photos|photograph|illustration|drawing|painting|artwork|art|logo|icon|sprite|wallpaper|poster|banner|thumbnail|portrait|avatar|graphic|mockup|render|scene|bild|foto|zeichnung|grafik|abbildung)\b/i;
const EDIT_LANGUAGE = /\b(remove|replace|make it|turn.*into|entferne|mach)\b/i;
const QUESTION = /\b(what|who|why|how|is|does|was|wer)\b/i;

export function detectIntent(
  draft: string,
  attachments: Attachment[] = []
): { intent: MediaIntent; confidence: "high" | "low" } {
  const text = draft.trim();
  const hasImageAttachment = attachments.some((a) => a.kind === "image");

  // A visual verb is enough on its own; a generic verb needs the sentence to
  // actually name a picture or a clip, otherwise "create a route handler" would
  // light up the image chip.
  const hasMediaVerb =
    VISUAL_VERB.test(text) ||
    (GENERIC_VERB.test(text) && (IMAGE_NOUN.test(text) || VIDEO_NOUN.test(text)));

  if (hasMediaVerb) {
    return VIDEO_NOUN.test(text)
      ? { intent: "video", confidence: "high" }
      : { intent: "image", confidence: "high" };
  }

  if (hasImageAttachment && EDIT_LANGUAGE.test(text)) return { intent: "edit", confidence: "high" };
  if (hasImageAttachment && QUESTION.test(text)) return { intent: "chat", confidence: "high" };

  return { intent: "chat", confidence: "low" };
}
