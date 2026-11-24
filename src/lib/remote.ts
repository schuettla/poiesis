/** The words of the Remote tab (`REM-UI-1`), as pure functions so they are tested without a screen. */
import type { RemoteSender, RemoteTelegramStatus } from "./api";

/** The one line under the switch. */
export function remoteStateLine(enabled: boolean, senders: number): string {
  if (!enabled) return "Off. Nothing outside can reach me.";
  if (senders === 0) return "On. Nobody is paired yet.";
  return `On. I am reachable by ${senders} paired ${senders === 1 ? "sender" : "senders"}.`;
}

/** When a paired phone last spoke, in words. */
export function lastSeenWords(lastSeen: number, now: number): string {
  if (!lastSeen) return "Not seen yet";
  const minutes = Math.floor(Math.max(0, now - lastSeen) / 60_000);
  if (minutes < 1) return "Seen just now";
  if (minutes < 60) return `Seen ${minutes} ${minutes === 1 ? "minute" : "minutes"} ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `Seen ${hours} ${hours === 1 ? "hour" : "hours"} ago`;
  const days = Math.floor(hours / 24);
  return `Seen ${days} ${days === 1 ? "day" : "days"} ago`;
}

/** The time a pairing code has left, `4:59`. Empty once it has run out. */
export function countdown(expiresAt: number, now: number): string {
  const left = Math.ceil((expiresAt - now) / 1000);
  if (left <= 0) return "";
  const m = Math.floor(left / 60);
  const s = String(left % 60).padStart(2, "0");
  return `${m}:${s}`;
}

/** `ABCD2345` shown as `ABCD 2345`, easier to read off a screen and type. */
export function codeGroups(code: string): string {
  return code.length === 8 ? `${code.slice(0, 4)} ${code.slice(4)}` : code;
}

/** The paired phones, the way the tab lists them. */
export const phonesOf = (senders: RemoteSender[]): RemoteSender[] => senders.filter((s) => s.transport === "phone");

/** The paired Telegram accounts. */
export const telegramOf = (senders: RemoteSender[]): RemoteSender[] =>
  senders.filter((s) => s.transport === "telegram");

/** The one line under the Telegram heading: what the bot is doing, or what Telegram said when it refused. */
export function telegramLine(status: RemoteTelegramStatus | null, enabled: boolean): string {
  if (!status?.token_hint) return "Not connected.";
  if (status.error) return status.error;
  const who = status.bot ? `Connected as ${status.bot}.` : "Token kept.";
  if (status.polling) return who;
  return enabled ? `${who} Connecting ...` : `${who} It listens when the switch is on.`;
}
