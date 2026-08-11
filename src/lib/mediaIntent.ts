import type { Attachment } from "./types";

/** What a bare (undeclared) message is probably asking for (`PIK-3`). Only
 * ever consulted when a *chat* model is selected — a declaration through the
 * model chooser (`PIK-2`) always wins and this never runs against it. */
export type MediaIntent = "chat" | "image" | "video" | "edit";

// Anchored at the start (`^`): the verb has to be the sentence's own
// imperative for this to count. "how do I draw a fox in Illustrator?" fails
// here — "how" is first — which is what keeps a genuine question out of the
// chip without needing an object noun ("draw a fox" alone has none).
const LEADING_VERB = /^(draw|generate|create|make|paint|render|zeichne|male|erstelle)\b/i;
const VIDEO_NOUN = /\b(video|clip|animation|film)\b/i;
const EDIT_LANGUAGE = /\b(remove|replace|make it|turn.*into|entferne|mach)\b/i;
const QUESTION = /\b(what|who|why|how|is|does|was|wer)\b/i;

export function detectIntent(
  draft: string,
  attachments: Attachment[] = []
): { intent: MediaIntent; confidence: "high" | "low" } {
  const text = draft.trim();
  const hasImageAttachment = attachments.some((a) => a.kind === "image");

  if (LEADING_VERB.test(text)) {
    return VIDEO_NOUN.test(text)
      ? { intent: "video", confidence: "high" }
      : { intent: "image", confidence: "high" };
  }

  if (hasImageAttachment && EDIT_LANGUAGE.test(text)) return { intent: "edit", confidence: "high" };
  if (hasImageAttachment && QUESTION.test(text)) return { intent: "chat", confidence: "high" };

  return { intent: "chat", confidence: "low" };
}
