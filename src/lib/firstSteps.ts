import type { CatalogEntry, HardwareProfile } from "./api";

/** What the first-steps screen needs to know about the app to judge "can I chat?". */
export interface ChatReadiness {
  libraryModels: { role?: string }[];
  providers: { key_set: boolean }[];
  endpointModels: unknown[];
}

/** True when a chat can start right now: a model on this PC, a saved account
 * key, or a server the user connected. Any one is enough. */
export function canChatNow(s: ChatReadiness): boolean {
  return (
    s.libraryModels.some((m) => !m.role || m.role === "chat") ||
    s.providers.some((p) => p.key_set) ||
    s.endpointModels.length > 0
  );
}

/** The screen opens by itself only when the model lists have arrived and there
 * is still nothing to chat with. Judging earlier would flash it at people who
 * are set up. */
export function needsFirstSteps(s: ChatReadiness & { modelsLoaded: boolean }, desktop: boolean): boolean {
  return desktop && s.modelsLoaded && !canChatNow(s);
}

/** The model the button downloads: the first catalog entry that fits this PC.
 * `null` when nothing fits, so the screen can say so instead of failing later. */
export function pickFirstModel(catalog: CatalogEntry[]): CatalogEntry | null {
  return catalog.find((e) => e.fit !== "wont-fit") ?? null;
}

export const filenameOf = (url: string) => url.split("?")[0].split("/").pop() ?? "";

export function sizeLabel(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}

/** One plain line about this PC: graphics card (if any) and memory. */
export function computerLine(hw: HardwareProfile): string {
  const gpu = hw.gpus.find((g) => g.vram_mb) ?? hw.gpus[0];
  const parts = [gpu ? gpu.name : hw.cpu.brand];
  parts.push(`${Math.round(hw.ram_mb / 1024)} GB memory`);
  return parts.join(", ");
}
