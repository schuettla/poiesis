import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import { useAppStore } from "../../lib/store";

/**
 * `WRK-3`: how I am working in this chat, always under the composer (the glyph
 * on the "this chat" button, `ChatSetup`), Ad hoc included. A person should be able to see at a glance how the work will go,
 * and a short question that works exactly as it always did says `Ad hoc`,
 * quietly, instead of showing nothing and leaving it to be guessed.
 *
 * `change` opens the `/style` menu, which is where it is changed. There is no
 * way to "remove" it: Ad hoc is a way of working like the others and is chosen
 * from the menu.
 *
 * A switch applies from the next message. In the middle of a run it applies at the
 * end of the item in hand (`WRK-13`), never in the middle of one, and until then it
 * says so: `Mission Command → Command & Control`, with "Switches after this item"
 * in the tooltip.
 */
export function useStyleState() {
  const convId = useAppStore((s) => s.activeConversationId);
  const style = useAppStore((s) => s.conversations.find((c) => c.id === s.activeConversationId)?.style ?? null);
  const presets = useAppStore((s) => s.presets);
  const switching = useAppStore((s) => (s.activeConversationId ? s.runSummaries[s.activeConversationId]?.switching : undefined));
  const working = useAppStore((s) => (s.activeConversationId ? s.runSummaries[s.activeConversationId]?.style?.name : undefined));
  const requestComposer = useAppStore((s) => s.requestComposer);

  // `WRK-17`: a chat nobody chose a preset for starts where its place says (its kind of
  // work, persona, project, or everywhere). Asked of the backend, so the chip says what
  // the next run will hold. It follows the list of presets, which reloads when a default
  // is set or a preset is deleted.
  const [started, setStarted] = useState<string | null>(null);
  useEffect(() => {
    if (!convId || style || !api.inTauri()) {
      setStarted(null);
      return;
    }
    let cancelled = false;
    api
      .conversationDefaultStyle(convId)
      .then((v) => !cancelled && setStarted(v))
      .catch(() => !cancelled && setStarted(null));
    return () => {
      cancelled = true;
    };
  }, [convId, style, presets]);

  const slug = style ?? started ?? "ad-hoc";
  const preset = presets.find((p) => p.slug === slug);
  const name = preset?.name ?? "Ad hoc";
  const line = preset?.line ?? "Just answer";
  // While the run still works the old way, the chip says both.
  const shown = switching ? `${working ?? "Ad hoc"} → ${switching.name}` : name;
  const glyph = switching ? (presets.find((p) => p.name === working)?.glyph ?? "ad-hoc") : (preset?.glyph ?? "ad-hoc");
  const where = switching
    ? "Switches after this item"
    : style
      ? "Your choice for this chat"
      : started
        ? "Where chats like this start"
        : "How I work unless you say otherwise";
  return {
    convId,
    slug,
    shown,
    line,
    where,
    glyph,
    branch: !switching && !!preset?.parent,
    /** Not the plain default: a chosen way of working, or a switch under way. */
    chosen: slug !== "ad-hoc" || !!switching,
    change: () => requestComposer("/style "),
  };
}
