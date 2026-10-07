/**
 * `UCM-9`: `/export` asks where to put the file, writes the Markdown the backend
 * built there, and says where it went. Leaving the dialog writes nothing.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const save = vi.fn();
const exportConversation = vi.fn(() => Promise.resolve("# Plan the trip\n"));
const saveArtifactFile = vi.fn(() => Promise.resolve());

vi.mock("@tauri-apps/plugin-dialog", () => ({ save: (...a: unknown[]) => save(...a) }));
vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  exportConversation: (...a: unknown[]) => (exportConversation as (...x: unknown[]) => unknown)(...a),
  saveArtifactFile: (...a: unknown[]) => (saveArtifactFile as (...x: unknown[]) => unknown)(...a),
}));

import { useAppStore } from "./store";

beforeEach(() => {
  save.mockReset();
  exportConversation.mockClear();
  saveArtifactFile.mockClear();
  useAppStore.setState({
    conversations: [{ id: "c1", title: "Plan the trip!", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    savedToast: null,
  });
});

describe("exporting a conversation", () => {
  it("suggests a file name from the title, saves the Markdown there, and says where", async () => {
    save.mockResolvedValue("C:/notes/trip.md");
    const dest = await useAppStore.getState().exportConversation("c1");
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "plan-the-trip.md" }));
    expect(saveArtifactFile).toHaveBeenCalledWith("C:/notes/trip.md", "markdown", "# Plan the trip\n");
    expect(dest).toBe("C:/notes/trip.md");
    expect(useAppStore.getState().savedToast).toBe("◆ Saved to C:/notes/trip.md");
  });

  it("writes nothing when the dialog is closed without a place", async () => {
    save.mockResolvedValue(null);
    expect(await useAppStore.getState().exportConversation("c1")).toBeNull();
    expect(exportConversation).not.toHaveBeenCalled();
    expect(saveArtifactFile).not.toHaveBeenCalled();
    expect(useAppStore.getState().savedToast).toBeNull();
  });

  it("falls back to a plain name for a title with nothing usable in it", async () => {
    useAppStore.setState({ conversations: [{ id: "c1", title: "???", updatedAt: 0, messages: [] }] });
    save.mockResolvedValue(null);
    await useAppStore.getState().exportConversation("c1");
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "conversation.md" }));
  });
});
