import { describe, expect, it } from "vitest";
import type { RemoteSender, RemoteTelegramStatus } from "./api";
import { codeGroups, countdown, lastSeenWords, phonesOf, remoteStateLine, telegramLine, telegramOf } from "./remote";

describe("the words of the Remote tab", () => {
  it("says plainly whether anything outside can reach me", () => {
    expect(remoteStateLine(false, 3)).toBe("Off. Nothing outside can reach me.");
    expect(remoteStateLine(true, 0)).toBe("On. Nobody is paired yet.");
    expect(remoteStateLine(true, 1)).toBe("On. I am reachable by 1 paired sender.");
    expect(remoteStateLine(true, 2)).toBe("On. I am reachable by 2 paired senders.");
  });

  it("says when a phone was last seen", () => {
    const now = 10_000_000_000;
    expect(lastSeenWords(0, now)).toBe("Not seen yet");
    expect(lastSeenWords(now - 20_000, now)).toBe("Seen just now");
    expect(lastSeenWords(now - 60_000, now)).toBe("Seen 1 minute ago");
    expect(lastSeenWords(now - 5 * 60_000, now)).toBe("Seen 5 minutes ago");
    expect(lastSeenWords(now - 3 * 3_600_000, now)).toBe("Seen 3 hours ago");
    expect(lastSeenWords(now - 2 * 86_400_000, now)).toBe("Seen 2 days ago");
    // A clock a little ahead of the desktop's is not "in the future".
    expect(lastSeenWords(now + 5_000, now)).toBe("Seen just now");
  });

  it("counts a pairing code down to zero and then says nothing", () => {
    expect(countdown(300_000, 0)).toBe("5:00");
    expect(countdown(300_000, 1_000)).toBe("4:59");
    expect(countdown(300_000, 299_001)).toBe("0:01");
    expect(countdown(300_000, 300_000)).toBe("");
    expect(countdown(300_000, 400_000)).toBe("");
  });

  it("shows a code in two groups of four", () => {
    expect(codeGroups("ABCD2345")).toBe("ABCD 2345");
    expect(codeGroups("SHORT")).toBe("SHORT");
  });

  it("lists phones only", () => {
    const row = (transport: string): RemoteSender => ({
      transport,
      id: transport,
      label: transport,
      paired_at: 1,
      last_seen: 1,
      level: "chat",
    });
    expect(phonesOf([row("phone"), row("telegram")]).map((s) => s.transport)).toEqual(["phone"]);
  });
});

describe("the Telegram words", () => {
  const bot = (over: Partial<RemoteTelegramStatus> = {}): RemoteTelegramStatus => ({
    token_hint: "...x9Z4",
    bot: "@poiesis_bot",
    error: null,
    polling: false,
    ...over,
  });

  it("says not connected until a token is kept", () => {
    expect(telegramLine(null, true)).toBe("Not connected.");
    expect(telegramLine(bot({ token_hint: null }), true)).toBe("Not connected.");
  });

  it("says what the bot is doing, and Telegram's words when it refused", () => {
    expect(telegramLine(bot({ polling: true }), true)).toBe("Connected as @poiesis_bot.");
    expect(telegramLine(bot(), false)).toBe("Connected as @poiesis_bot. It listens when the switch is on.");
    expect(telegramLine(bot(), true)).toBe("Connected as @poiesis_bot. Connecting ...");
    expect(telegramLine(bot({ bot: null }), false)).toBe("Token kept. It listens when the switch is on.");
    expect(telegramLine(bot({ error: "Unauthorized", polling: true }), true)).toBe("Unauthorized");
  });

  it("lists the accounts apart from the phones", () => {
    const row = (transport: string): RemoteSender => ({ transport, id: transport, label: "", paired_at: 1, last_seen: 1, level: "chat" });
    expect(telegramOf([row("phone"), row("telegram")]).map((s) => s.transport)).toEqual(["telegram"]);
  });
});
