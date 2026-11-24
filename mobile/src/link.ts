/**
 * The page's side of the link (REM-13). Five calls and a listener; the shell holds the key, the address and the
 * connection, so the page never sees them. Tests replace `@tauri-apps/api/core` with a fake.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { LinkState, PhoneState } from "./types";

/** What the shell knows: paired or not, the desktop's name, and the state of the link. */
export const state = () => invoke<PhoneState>("remote_state_cmd");

/** Make sure there is a live connection. Used on start and when the app comes back to the front. */
export const connect = () => invoke<PhoneState>("remote_connect_cmd");

/** Pair with the desktop whose pairing text this is. Resolves with the desktop's name. */
export const pair = (text: string, label: string) => invoke<string>("remote_pair_cmd", { text, label });

export const forget = () => invoke<void>("remote_forget_cmd");

/** One command, one answer. Rejects with a plain sentence. */
export function call<T = unknown>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  return invoke<T>("remote_call_cmd", { cmd, args });
}

/** A command that answers with events until it is done. Resolves when it is. */
export function stream(cmd: string, args: Record<string, unknown>, onEvent: (event: unknown) => void): Promise<void> {
  const channel = new Channel<unknown>();
  channel.onmessage = onEvent;
  return invoke<void>("remote_stream_cmd", { cmd, args, onEvent: channel });
}

/** The state of the link as it changes. Returns the function that stops listening. */
export async function onLink(cb: (link: LinkState) => void): Promise<() => void> {
  return listen<LinkState>("remote-link", (e) => cb(e.payload));
}

/** Words for a rejection, whatever it was. */
export function words(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Something went wrong.";
}
