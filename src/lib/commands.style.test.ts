/**
 * `WRK-3`: `/style`. How a chat works is chosen through the same `/` menu as
 * everything else; `/style lean` sets the chat, `/style lean <message>` works
 * that one message the lean way and leaves the chat as it was.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const setStyle = vi.fn();
const setDefault = vi.fn();
vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  setConversationStyle: (...a: unknown[]) => setStyle(...a),
  setStyleDefault: (...a: unknown[]) => setDefault(...a),
}));

import { argChoices, commandViews, controlFor, matchPreset, parseCommandLine, runCommand } from "./commands";
import { useAppStore } from "./store";
import type { PresetView } from "./api";

const preset = (over: Partial<PresetView>): PresetView => ({
  slug: "lean",
  name: "Lean",
  line: "One thing at a time, stop the line when something breaks",
  glyph: "lean",
  built_in: true,
  locked: false,
  version: 1,
  rules: [],
  ...over,
});

const PRESETS: PresetView[] = [
  preset({ slug: "ad-hoc", name: "Ad hoc", line: "Just answer", glyph: "ad-hoc" }),
  preset({}),
  preset({ slug: "mission-command", name: "Mission Command", line: "You set the goal, I find the way", glyph: "mission-command" }),
];

const views = () => commandViews(useAppStore.getState());
const run = (line: string) => runCommand(parseCommandLine(line, views())!, useAppStore.getState());

beforeEach(() => {
  setStyle.mockReset().mockImplementation(async (_id: string, p: string | null) => p);
  setDefault.mockReset().mockResolvedValue(undefined);
  useAppStore.setState({
    presets: PRESETS,
    activeConversationId: "c1",
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: null }],
    turnModifiers: {},
    skills: [],
    busy: false,
    activeRun: null,
  });
});

describe("naming a preset", () => {
  it("finds it by name or slug in any case, and says what is left over", () => {
    expect(matchPreset(PRESETS, "lean")).toMatchObject({ preset: { slug: "lean" }, rest: "" });
    expect(matchPreset(PRESETS, "LEAN fix the export")).toMatchObject({ preset: { slug: "lean" }, rest: "fix the export" });
    expect(matchPreset(PRESETS, "mission command fix it")).toMatchObject({
      preset: { slug: "mission-command" },
      rest: "fix it",
    });
    expect(matchPreset(PRESETS, "mission-command")).toMatchObject({ preset: { slug: "mission-command" } });
    expect(matchPreset(PRESETS, "ad hoc")).toMatchObject({ preset: { slug: "ad-hoc" }, rest: "" });
  });

  it("does not take the start of a longer word for a preset", () => {
    expect(matchPreset(PRESETS, "leaner")).toBeNull();
    expect(matchPreset(PRESETS, "swarm do it")).toBeNull();
  });
});

describe("/style", () => {
  it("sets how this chat works", async () => {
    expect(await run("/style lean")).toEqual({ kind: "done", note: undefined });
    expect(setStyle).toHaveBeenCalledWith("c1", "lean");
    expect(useAppStore.getState().conversations[0].style).toBe("lean");
  });

  it("keeps Ad hoc as a choice, so a project or global default cannot override it", async () => {
    useAppStore.setState({ conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: "lean" }] });
    await run("/style ad hoc");
    expect(setStyle).toHaveBeenCalledWith("c1", "ad-hoc");
    expect(useAppStore.getState().conversations[0].style).toBe("ad-hoc");
  });

  it("with a message works that one message the preset's way and leaves the chat alone", async () => {
    const result = await run("/style lean fix the export");
    expect(result).toEqual({ kind: "modifier", send: "fix the export" });
    expect(useAppStore.getState().turnModifiers.style).toBe("lean");
    expect(setStyle).not.toHaveBeenCalled();
    expect(useAppStore.getState().conversations[0].style).toBeNull();
  });

  it("asks for a preset when none is given, and says so when it is not one", async () => {
    expect(await run("/style")).toEqual({ kind: "needsArg" });
    expect(await run("/style swarm")).toMatchObject({ kind: "error" });
  });

  it("offers every preset the way it is typed, with its second line", () => {
    const names = argChoices("styles", "", useAppStore.getState())
      .filter((c) => !c.value.startsWith("@"))
      .map((c) => [c.value, c.hint]);
    expect(names).toEqual([
      ["Ad hoc", "Just answer"],
      ["Lean", "One thing at a time, stop the line when something breaks"],
      ["Mission Command", "You set the goal, I find the way"],
    ]);
  });

  it("shows the preset in force on its row", () => {
    expect(controlFor("style", useAppStore.getState())).toMatchObject({ kind: "pick", current: "Ad hoc" });
    useAppStore.setState({ conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: "lean" }] });
    expect(controlFor("style", useAppStore.getState())).toMatchObject({ kind: "pick", current: "Lean", value: "Lean" });
  });
});

describe("/style: yours, and the rows about the ways themselves (WRK-17)", () => {
  const mine = preset({ slug: "careful", name: "Careful with mail", line: "I ask first", built_in: false });
  const values = () => argChoices("styles", "", useAppStore.getState()).map((c) => c.value);

  it("puts the ones you made after the built-in ones, under Yours", () => {
    useAppStore.setState({ presets: [...PRESETS, mine] });
    const rows = argChoices("styles", "", useAppStore.getState());
    expect(rows.find((c) => c.value === "Careful with mail")?.group).toBe("Yours");
    expect(rows.find((c) => c.value === "Lean")?.group).toBeUndefined();
  });

  it("offers to change what they do, and the rest only where it makes sense", () => {
    expect(values()).toEqual(["Ad hoc", "Lean", "Mission Command", "@edit"]);
    useAppStore.setState({ conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: "lean" }] });
    expect(values()).toContain("@save");
    expect(values()).not.toContain("@project");
    useAppStore.setState({
      conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: "lean", projectId: "p1" }],
    });
    expect(values()).toContain("@project");
  });

  it("keeps the rows away from what is being typed", () => {
    expect(argChoices("styles", "le", useAppStore.getState()).map((c) => c.value)).toEqual(["Lean"]);
  });

  it("opens How I work, the save dialog, and sets the project's default", async () => {
    const openSelf = vi.fn();
    const openPresetDialog = vi.fn();
    const loadPresets = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({
      openSelf,
      openPresetDialog,
      loadPresets,
      conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: "lean", projectId: "p1" }],
    } as never);
    const state = () => useAppStore.getState();
    const go = (arg: string) => runCommand(parseCommandLine(`/style ${arg}`, views())!, state());
    expect(await go("@edit")).toMatchObject({ kind: "done" });
    expect(openSelf).toHaveBeenCalledWith("ways");
    expect(await go("@save")).toMatchObject({ kind: "done" });
    expect(openPresetDialog).toHaveBeenCalledWith({ mode: "new", from: "lean" });
    expect(await go("@project")).toMatchObject({ kind: "done" });
    expect(setDefault).toHaveBeenCalledWith("project:p1", "lean");
    expect(loadPresets).toHaveBeenCalled();
    expect(setStyle).not.toHaveBeenCalled();
  });

  it("does not pretend a row applies where it does not", async () => {
    expect(await run("/style @project")).toMatchObject({ kind: "error" });
    expect(await run("/style @save")).toMatchObject({ kind: "error" });
  });

  it("/ways opens the tab", async () => {
    const openSelf = vi.fn();
    useAppStore.setState({ openSelf } as never);
    expect(await run("/ways")).toMatchObject({ kind: "done" });
    expect(openSelf).toHaveBeenCalledWith("ways");
  });
});

describe("/style: what evolution left alone, and what branched off (WRK-20)", () => {
  it("marks a locked preset, and says where an offspring came from", () => {
    const lean = { ...PRESETS.find((p) => p.slug === "lean")!, locked: true };
    const kid = preset({ slug: "lean-for-code", name: "Lean for code", line: "Without splitting", built_in: false, parent: "lean", lineage: "from Lean, Nov 7", glyph: "lean" });
    useAppStore.setState({ presets: [PRESETS[0], lean, kid] });
    const rows = argChoices("styles", "", useAppStore.getState());
    expect(rows.find((c) => c.value === "Lean")).toMatchObject({ locked: true, branch: false });
    expect(rows.find((c) => c.value === "Ad hoc")?.locked).toBe(false);
    expect(rows.find((c) => c.value === "Lean for code")).toMatchObject({
      branch: true,
      hint: "Without splitting (from Lean, Nov 7)",
    });
  });
});
