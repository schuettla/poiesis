/**
 * `SHL-10`..`SHL-24`: the header strip's own state — the sidebar's item tabs
 * and its sub-view — apart from any component. Runs with `api.inTauri()` false
 * (no Tauri bridge in this environment), so persistence is skipped and only
 * the local state transitions are under test.
 */
import { beforeEach, describe, expect, it } from "vitest";
import { itemKey, useAppStore, validateTabSet } from "./store";
import type { Conversation } from "./types";

function conv(id: string, folderPath?: string): Conversation {
  return { id, title: id, updatedAt: 0, messages: [], ...(folderPath ? { folderPath } : {}) };
}

beforeEach(() => {
  useAppStore.setState({
    conversations: [conv("c1"), conv("c2"), conv("c3")],
    activeConversationId: "c1",
    itemTabs: [],
    activeItemId: null,
    selected: null,
    view: "chat",
    artifacts: {},
  });
});

describe("showing a conversation (SHL-24)", () => {
  it("shows the conversation asked for", async () => {
    await useAppStore.getState().openSession("c2");
    expect(useAppStore.getState().activeConversationId).toBe("c2");
  });

  it("keeps no list of open chats — a chat is a destination, not a tab", async () => {
    await useAppStore.getState().openSession("c2");
    await useAppStore.getState().openSession("c3");
    expect(useAppStore.getState()).not.toHaveProperty("sessionTabs");
  });
});

describe("item tabs (SHL-10/SHL-22)", () => {
  it("opening a file adds a tab, focuses it, and mirrors `selected`", () => {
    useAppStore.getState().openItem({ kind: "file", id: "a.txt" });
    const s = useAppStore.getState();
    expect(s.itemTabs).toEqual([{ kind: "file", id: "a.txt", conversationId: "c1" }]);
    expect(s.activeItemId).toBe(itemKey({ kind: "file", id: "a.txt" }));
    expect(s.selected).toEqual({ kind: "file", id: "a.txt" });
  });

  it("opening the same item twice focuses rather than duplicates", () => {
    useAppStore.getState().openItem({ kind: "file", id: "a.txt" });
    useAppStore.getState().openItem({ kind: "run", id: "r1" });
    useAppStore.getState().openItem({ kind: "file", id: "a.txt" });
    const s = useAppStore.getState();
    expect(s.itemTabs).toHaveLength(2);
    expect(s.activeItemId).toBe(itemKey({ kind: "file", id: "a.txt" }));
  });

  it("a run is not mirrored into `selected` — the Viewer doesn't render one", () => {
    useAppStore.getState().openItem({ kind: "run", id: "r1" });
    expect(useAppStore.getState().selected).toBeNull();
  });

  it("opening an item does not move the sidebar", () => {
    useAppStore.setState({ dockView: "agents" });
    useAppStore.getState().openItem({ kind: "file", id: "a.txt" });
    expect(useAppStore.getState().dockView).toBe("agents");
  });

  it("opening an item from a route comes back to the chat area to show it", () => {
    useAppStore.getState().setView("library");
    useAppStore.getState().openItem({ kind: "artifact", id: "a1" });
    expect(useAppStore.getState().view).toBe("chat");
  });

  it("closing the active tab focuses its neighbour", () => {
    useAppStore.getState().openItem({ kind: "file", id: "a.txt" });
    useAppStore.getState().openItem({ kind: "file", id: "b.txt" });
    useAppStore.getState().closeItem(itemKey({ kind: "file", id: "a.txt" }));
    const s = useAppStore.getState();
    expect(s.itemTabs).toEqual([{ kind: "file", id: "b.txt", conversationId: "c1" }]);
    expect(s.activeItemId).toBe(itemKey({ kind: "file", id: "b.txt" }));
    expect(s.selected).toEqual({ kind: "file", id: "b.txt" });
  });

  it("closing the last item tab shows the conversation again", () => {
    useAppStore.getState().openItem({ kind: "file", id: "a.txt" });
    useAppStore.getState().closeItem(itemKey({ kind: "file", id: "a.txt" }));
    const s = useAppStore.getState();
    expect(s.itemTabs).toEqual([]);
    expect(s.activeItemId).toBeNull();
    expect(s.selected).toBeNull();
  });

  it("selectNode(null) closes the active item tab", () => {
    useAppStore.getState().openItem({ kind: "file", id: "a.txt" });
    useAppStore.getState().selectNode(null);
    expect(useAppStore.getState().itemTabs).toEqual([]);
  });

  it("switching the active conversation keeps the item tabs and shows the chat", async () => {
    // One global strip: the tab stays open, remembering its own chat.
    useAppStore.getState().openItem({ kind: "file", id: "a.txt" });
    await useAppStore.getState().setActiveConversation("c2");
    const s = useAppStore.getState();
    expect(s.itemTabs).toEqual([{ kind: "file", id: "a.txt", conversationId: "c1" }]);
    expect(s.activeItemId).toBeNull();
    expect(s.selected).toBeNull();
  });

  it("re-selecting the already-active conversation does not close open tabs", async () => {
    useAppStore.getState().openItem({ kind: "file", id: "a.txt" });
    await useAppStore.getState().setActiveConversation("c1");
    expect(useAppStore.getState().itemTabs).toHaveLength(1);
  });

  it("going back to the live chat unfocuses the item but keeps its tab open", async () => {
    useAppStore.getState().openItem({ kind: "file", id: "a.txt" });
    await useAppStore.getState().openSession("c1");
    const s = useAppStore.getState();
    expect(s.activeItemId).toBeNull();
    expect(s.itemTabs).toHaveLength(1);
  });
});

describe("coding items (PRJ-UI-3/COD-UI-2)", () => {
  it("a diff opens as its own tab and is not mirrored into `selected`", () => {
    useAppStore.getState().openItem({ kind: "diff", id: "C:\\work\\a.rs" });
    const s = useAppStore.getState();
    expect(s.activeItemId).toBe("diff:C:\\work\\a.rs");
    expect(s.selected).toBeNull();
  });

  it("a diff and the file it patches are two tabs", () => {
    useAppStore.getState().openItem({ kind: "file", id: "C:\\work\\a.rs" });
    useAppStore.getState().openItem({ kind: "diff", id: "C:\\work\\a.rs" });
    expect(useAppStore.getState().itemTabs).toHaveLength(2);
  });

  it("opening a file at another line keeps one tab and moves the line", () => {
    useAppStore.getState().openItem({ kind: "file", id: "C:\\work\\a.rs", line: 3 });
    useAppStore.getState().openItem({ kind: "file", id: "C:\\work\\a.rs", line: 40 });
    const s = useAppStore.getState();
    expect(s.itemTabs).toHaveLength(1);
    expect(s.itemTabs[0]).toMatchObject({ kind: "file", line: 40 });
    expect(s.activeItemId).toBe("file:C:\\work\\a.rs");
  });

  it("a dot's click puts the sidebar on Changes, aimed at that file, and opens no tab", () => {
    useAppStore.setState({ dockView: "files", changesFocus: null });
    useAppStore.getState().focusChange("c1", "C:\\work\\a.rs");
    const s = useAppStore.getState();
    expect(s.dockView).toBe("changes");
    expect(s.changesFocus).toBe("C:\\work\\a.rs");
    expect(s.itemTabs).toEqual([]);
  });
});

describe("routes are destinations again (SHL-24)", () => {
  it("setView goes to a route and keeps no list of open ones", () => {
    useAppStore.getState().setView("settings");
    const s = useAppStore.getState();
    expect(s.view).toBe("settings");
    expect(s).not.toHaveProperty("routeTabs");
  });

  it("going to a route leaves the live conversation untouched", () => {
    // A route replaces what you are *looking at*, never what you are talking
    // to: coming back restores the chat because nothing about it was ever
    // unmounted from the store's point of view.
    const before = useAppStore.getState().activeConversationId;
    useAppStore.getState().setView("settings");
    expect(useAppStore.getState().activeConversationId).toBe(before);
    useAppStore.getState().setView("chat");
    const s = useAppStore.getState();
    expect(s.view).toBe("chat");
    expect(s.activeConversationId).toBe(before);
  });
});

/**
 * `SHL-17-T`: what a persisted set is allowed to bring back. Every case here
 * is an entry that once resolved and no longer does — the failure mode is a
 * tab that draws but does nothing, which reads as a broken app rather than as
 * stale data.
 */
describe("restoring a persisted tab set (SHL-17)", () => {
  const conversations = [conv("c1", "C:\\work"), conv("c2")];
  const write = (o: Record<string, unknown>) => JSON.stringify(o);

  it("ignores a set written when chats and routes were tabs (SHL-24)", () => {
    // Both lists are still on disk for anyone upgrading. There is nowhere to
    // restore them into, and reading them back as anything would recreate the
    // navigation model this removed.
    const out = validateTabSet(write({ sessionTabs: ["c1", "c2"], routeTabs: ["settings"] }), conversations);
    expect(out).toEqual({ itemTabs: [], activeItemId: null, dockView: "files" });
  });

  it("drops a file tab that is no longer inside the conversation's folder", () => {
    const out = validateTabSet(
      write({
        itemConversationId: "c1",
        itemTabs: [
          { kind: "file", id: "C:\\work\\notes.md" },
          { kind: "file", id: "C:\\elsewhere\\stray.md" },
        ],
      }),
      conversations
    );
    expect(out.itemTabs).toEqual([{ kind: "file", id: "C:\\work\\notes.md", conversationId: "c1" }]);
  });

  it("drops every file tab when the folder was detached entirely", () => {
    const out = validateTabSet(
      write({ itemConversationId: "c2", itemTabs: [{ kind: "file", id: "C:\\work\\notes.md" }] }),
      [conv("c2"), conv("c1")]
    );
    expect(out.itemTabs).toEqual([]);
  });

  it("keeps another chat's item tabs, stamped with that chat, but not focused", () => {
    // One global strip: the tab stays, but it is not shown over the chat that
    // comes back live, since it belongs to a different one.
    const out = validateTabSet(
      write({ itemTabs: [{ kind: "artifact", id: "a1", conversationId: "c2" }], activeItemId: "artifact:a1" }),
      conversations
    );
    expect(out.itemTabs).toEqual([{ kind: "artifact", id: "a1", conversationId: "c2" }]);
    expect(out.activeItemId).toBeNull();
  });

  it("drops item tabs whose chat is gone", () => {
    const out = validateTabSet(
      write({ itemTabs: [{ kind: "artifact", id: "a1", conversationId: "deleted" }] }),
      conversations
    );
    expect(out.itemTabs).toEqual([]);
  });

  it("checks a file tab against its own chat's folder, not the live chat's", () => {
    const out = validateTabSet(
      write({ itemTabs: [{ kind: "file", id: "D:\\other\\a.md", conversationId: "c3" }] }),
      [...conversations, conv("c3", "D:\\other")]
    );
    expect(out.itemTabs).toHaveLength(1);
  });

  it("restores the sidebar's sub-view", () => {
    const out = validateTabSet(write({ dockView: "browser" }), conversations);
    expect(out.dockView).toBe("browser");
  });

  it("restores the Changes sub-view", () => {
    const out = validateTabSet(write({ dockView: "changes" }), conversations);
    expect(out.dockView).toBe("changes");
  });

  it("keeps a diff tab for a chat that works in a folder, and drops one that does not", () => {
    const out = validateTabSet(
      write({
        itemTabs: [
          { kind: "diff", id: "C:\\work\\a.rs", conversationId: "c1" },
          { kind: "diff", id: "C:\\work\\b.rs", conversationId: "c2" },
        ],
      }),
      conversations
    );
    expect(out.itemTabs).toEqual([{ kind: "diff", id: "C:\\work\\a.rs", conversationId: "c1" }]);
  });

  it("keeps a file tab's line through a restore", () => {
    const out = validateTabSet(
      write({ itemTabs: [{ kind: "file", id: "C:\\work\\a.rs", line: 12, conversationId: "c1" }] }),
      conversations
    );
    expect(out.itemTabs).toEqual([{ kind: "file", id: "C:\\work\\a.rs", line: 12, conversationId: "c1" }]);
  });

  it("ignores a sub-view this build does not have", () => {
    const out = validateTabSet(write({ dockView: "wormhole" }), conversations);
    expect(out.dockView).toBe("files");
  });

  // `SHL-22` migration: a set written with the two-zone strip holds `docTabs`
  // with `panel` entries. Those are sections, not items.
  it("migrates an older set: panels become the sub-view, files and artifacts stay tabs", () => {
    const out = validateTabSet(
      write({
        docConversationId: "c1",
        docTabs: [
          { kind: "panel", id: "files" },
          { kind: "file", id: "C:\\work\\notes.md" },
          { kind: "panel", id: "agents" },
        ],
        activeDocId: "file:C:\\work\\notes.md",
      }),
      conversations
    );
    expect(out.itemTabs).toEqual([{ kind: "file", id: "C:\\work\\notes.md", conversationId: "c1" }]);
    expect(out.activeItemId).toBe("file:C:\\work\\notes.md");
    expect(out.dockView, "the last panel seen").toBe("agents");
  });

  it("nulls an older set's active panel, since it is no longer a tab", () => {
    const out = validateTabSet(
      write({ docConversationId: "c1", docTabs: [{ kind: "panel", id: "browser" }], activeDocId: "panel:browser" }),
      conversations
    );
    expect(out.itemTabs).toEqual([]);
    expect(out.activeItemId).toBeNull();
    expect(out.dockView).toBe("browser");
  });

  it("nulls an active item that is not among the restored tabs", () => {
    const out = validateTabSet(
      write({ itemConversationId: "c1", itemTabs: [], activeItemId: "file:C:\\work\\notes.md" }),
      conversations
    );
    expect(out.activeItemId).toBeNull();
  });

  it("discards a set that does not parse rather than half-recovering it", () => {
    const out = validateTabSet("{not json", conversations);
    expect(out).toEqual({ itemTabs: [], activeItemId: null, dockView: "files" });
  });

  it("survives having nothing stored at all", () => {
    const out = validateTabSet(null, conversations);
    expect(out).toEqual({ itemTabs: [], activeItemId: null, dockView: "files" });
  });
});
