// What the voice surface shows. Its own small store, so a mic level arriving
// twenty times a second re-renders the voice surface and nothing else.

import { create } from "zustand";
import type { VoiceFloorName } from "../../components/Orb/orbState";

/** `off` means no live session. */
export type VoiceFloor = "off" | "starting" | VoiceFloorName;

export interface VoiceUiState {
  /** The voice surface covers the conversation. A session can run without it
   * ("Show chat" leaves the surface and keeps the session). */
  shown: boolean;
  floor: VoiceFloor;
  /** When the floor last changed, for "nobody has spoken for a while". */
  floorSince: number;
  /** Mic and speaker loudness, 0 to 1. */
  input: number;
  output: number;
  /** The user's words: partial while they pause, then the finished turn. */
  user: string;
  /** The sentence Poiesis is saying. */
  reply: string;
  muted: boolean;
  /** One plain line, e.g. no voice for this language (VXP-7). */
  hint: string | null;
  error: string | null;
}

export const initialVoiceUi: VoiceUiState = {
  shown: false,
  floor: "off",
  floorSince: 0,
  input: 0,
  output: 0,
  user: "",
  reply: "",
  muted: false,
  hint: null,
  error: null,
};

export const useVoiceStore = create<VoiceUiState>(() => initialVoiceUi);

export const setVoiceUi = (patch: Partial<VoiceUiState>) => useVoiceStore.setState(patch);
export const resetVoiceUi = () => useVoiceStore.setState({ ...initialVoiceUi });
