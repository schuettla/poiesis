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
  /** The voice could not start (no mic, no model). Blocks the surface and
   * offers "Try again". */
  error: string | null;
  /** Something went wrong while the conversation is live (a reply failed, the
   * voice or the mic stopped working). The conversation goes on, so this is a
   * line the user can dismiss, not a wall (VXP-7). */
  problem: string | null;
  /** Voice mode has been opened since the app started, so the composer offers
   * a way back to it however it was left. */
  used: boolean;
  /** When the live session began: the activity list only shows what happened
   * in it, not an old tool run from the chat. */
  startedAt: number;
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
  problem: null,
  used: false,
  startedAt: 0,
};

export const useVoiceStore = create<VoiceUiState>(() => initialVoiceUi);

export const setVoiceUi = (patch: Partial<VoiceUiState>) => useVoiceStore.setState(patch);
export const resetVoiceUi = () => useVoiceStore.setState({ ...initialVoiceUi });
