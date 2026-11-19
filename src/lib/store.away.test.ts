/**
 * @vitest-environment jsdom
 *
 * `TEAM-20`: a session shows a dot when a run changed it while the person was not looking, and
 * opening it clears the dot. Only a session a team leads gets one from a turn; a chat the person
 * watched to the end, or that no team leads, never does.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls = vi.hoisted(() => ({ away: [] as string[], seen: [] as string[], unread: [] as string[] }));

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  markAway: async (id: string) => {
    calls.away.push(id);
    if (!calls.unread.includes(id)) calls.unread.push(id);
  },
  markSeen: async (id: string) => {
    calls.seen.push(id);
    calls.unread = calls.unread.filter((x) => x !== id);
  },
  unreadSessions: async () => [...calls.unread],
}));

import { markAwayIfElsewhere, useAppStore, waitingAnswersMessage } from "./store";

const conv = (id: string, leader: string | null) => ({ id, title: id, updatedAt: 0, messages: [], leader, personaId: null });

function put(active: string | null, view = "chat") {
  calls.away = [];
  calls.seen = [];
  calls.unread = [];
  useAppStore.setState({
    conversations: [conv("team-chat", "team:shop-team"), conv("plain", null), conv("mira-chat", "persona:p1")],
    activeConversationId: active,
    view,
    unreadIds: [],
  } as never);
}

const get = () => useAppStore.getState();

beforeEach(() => put("team-chat"));

describe("a team's run that ended while the person looked elsewhere", () => {
  it("leaves its session unread when another chat is open", async () => {
    put("plain");
    await markAwayIfElsewhere(get, "team-chat");
    expect(calls.away).toEqual(["team-chat"]);
    expect(get().unreadIds).toEqual(["team-chat"]);
  });

  it("leaves it unread when the person is in Settings, not in the chat", async () => {
    put("team-chat", "settings");
    await markAwayIfElsewhere(get, "team-chat");
    expect(get().unreadIds).toEqual(["team-chat"]);
  });

  it("does nothing for a chat watched to the end", async () => {
    put("team-chat");
    await markAwayIfElsewhere(get, "team-chat");
    expect(calls.away).toEqual([]);
    expect(get().unreadIds).toEqual([]);
  });

  it("does nothing for a chat no team leads, however it ended", async () => {
    put("team-chat");
    await markAwayIfElsewhere(get, "plain");
    await markAwayIfElsewhere(get, "mira-chat");
    expect(calls.away).toEqual([]);
    expect(get().unreadIds).toEqual([]);
  });
});

describe("opening a session", () => {
  it("clears its dot and tells the backend, and leaves the other dots", async () => {
    put("plain");
    useAppStore.setState({ unreadIds: ["team-chat", "mira-chat"] } as never);
    // A click on a row: what is not about the dot is the store's own business.
    await get().setActiveConversation("team-chat").catch(() => {});
    expect(get().unreadIds).toEqual(["mira-chat"]);
    expect(calls.seen).toEqual(["team-chat"]);
  });
});

describe("the words the session is told", () => {
  it("list what was decided, one line each, and say to go on", () => {
    const said = waitingAnswersMessage(["The person said yes to this change, and it is made: Delete old.txt.", "The person answered api: orders"]);
    expect(said).toBe(
      "Here is what I decided about what was waiting for me:\n- The person said yes to this change, and it is made: Delete old.txt.\n- The person answered api: orders\nGo on from there."
    );
  });
});
