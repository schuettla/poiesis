/**
 * @vitest-environment jsdom
 *
 * `EXP-UI-1` and `EXP-3`: the Experiments screen, and what turning Workspace off
 * does to every way into it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExperimentInfo } from "../lib/api";

const setExperimentApi = vi.fn<(id: string, on: boolean) => Promise<ExperimentInfo[]>>();
const experimentsSeen = vi.fn();
const experimentsListApi = vi.fn<() => Promise<ExperimentInfo[]>>();
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  experimentsList: () => experimentsListApi(),
  setExperiment: (id: string, on: boolean) => setExperimentApi(id, on),
  experimentsSeen: () => experimentsSeen(),
  setConversationWorkspace: () => Promise.resolve(),
}));

import Experiments, { needsLine } from "./Experiments";
import SessionMenu from "../components/Conversation/SessionMenu";
import WorkspaceOffBanner from "../components/Workspace/WorkspaceOffBanner";
import { useAppStore } from "../lib/store";
import { applyControl, commandViews, controlFor, parseCommandLine, runCommand } from "../lib/commands";
import { experiment } from "../lib/experiments.fixture";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const setView = vi.fn();

const state = () => useAppStore.getState();
const mount = (node: React.ReactNode) => act(() => root.render(node));
const button = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.trim() === label) as
    | HTMLButtonElement
    | undefined;

function put(experiments: ExperimentInfo[] | null, over: Partial<ReturnType<typeof state>> = {}) {
  act(() => useAppStore.setState({ experiments, ...over }));
}

const WORKSPACE_CHAT = { id: "c1", title: "Trip", updatedAt: 0, messages: [], workspace: true };

beforeEach(() => {
  setExperimentApi.mockReset();
  experimentsSeen.mockReset().mockResolvedValue(undefined);
  // Opening the screen reads the list again; by default that read never answers.
  experimentsListApi.mockReset().mockReturnValue(new Promise(() => {}));
  setView.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    experiments: null,
    setView,
    view: "experiments",
    conversations: [WORKSPACE_CHAT],
    activeConversationId: "c1",
    workspaceMode: false,
    workspaceShown: true,
    toolsEnabled: true,
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the Experiments screen", () => {
  it("says what it is, and lists each experiment with what changes", () => {
    put([experiment()]);
    mount(<Experiments />);
    expect(container.querySelector("h1")?.textContent).toBe("Experiments");
    expect(container.textContent).toContain("off until you turn them on");
    expect(container.querySelector(".exp-title")?.textContent).toBe("Workspace mode");
    expect(container.querySelector(".exp-stage")?.textContent).toBe("Beta");
    expect(container.textContent).toContain("A chat can open as a Workspace");
    expect(container.textContent).toContain("Nothing of its own.");
  });

  it("opening it clears the new dot, in the store and on the backend", () => {
    put([experiment({ is_new: true })]);
    mount(<Experiments />);
    expect(state().experiments![0].is_new).toBe(false);
    expect(experimentsSeen).toHaveBeenCalledTimes(1);
  });

  it("flipping the switch asks the backend and shows the list it sends back", async () => {
    put([experiment()]);
    setExperimentApi.mockResolvedValue([experiment({ on: true })]);
    mount(<Experiments />);
    const input = () => container.querySelector<HTMLInputElement>("input[role=switch]")!;
    expect(input().checked).toBe(false);
    await act(async () => input().click());
    expect(setExperimentApi).toHaveBeenCalledWith("workspace", true);
    expect(input().checked).toBe(true);
    expect(state().experiments![0].on).toBe(true);
  });

  it("shows the backend's words when it refuses", async () => {
    put([experiment()]);
    setExperimentApi.mockRejectedValue("Workspace mode needs: a decision model.");
    mount(<Experiments />);
    await act(async () => container.querySelector<HTMLInputElement>("input[role=switch]")!.click());
    expect(container.querySelector(".exp-error")?.textContent).toContain("needs: a decision model");
    expect(state().experiments![0].on).toBe(false);
  });

  it("a missing requirement disables the switch, names the fix, and links to it", () => {
    put([
      experiment({
        id: "fast-slow",
        title: "Fast & slow thinking",
        can_turn_on: false,
        requires: [{ kind: "decision-model", label: "A decision model", met: false, fix_view: "models" }],
      }),
    ]);
    mount(<Experiments />);
    expect(container.querySelector<HTMLInputElement>("input[role=switch]")!.disabled).toBe(true);
    expect(container.textContent).toContain("Needs a decision model.");
    act(() => button("Set up")!.click());
    // The decision model is chosen on its own tab in Models.
    expect(state().view).toBe("models");
    expect(state().modelsTabRequest).toBe("decide");
  });

  it("says why it cannot be turned on in the status line, and on the switch", () => {
    put([
      experiment({
        id: "fast-slow",
        can_turn_on: false,
        requires: [{ kind: "decision-model", label: "A decision model", met: false, fix_view: "models" }],
      }),
    ]);
    mount(<Experiments />);
    expect(container.querySelector(".exp-status")?.textContent).toBe("Needs a decision model.");
    expect(container.querySelector(".exp-switch")?.getAttribute("title")).toBe("Needs a decision model.");
  });

  it("reads the list again when it opens, so a need met on another screen unblocks the switch", async () => {
    const blocked = experiment({
      id: "fast-slow",
      can_turn_on: false,
      requires: [{ kind: "decision-model", label: "A decision model", met: false, fix_view: "models" }],
    });
    put([blocked]);
    experimentsListApi.mockResolvedValue([
      { ...blocked, can_turn_on: true, requires: [{ ...blocked.requires[0], met: true }] },
    ]);
    await act(async () => root.render(<Experiments />));
    expect(experimentsListApi).toHaveBeenCalledTimes(1);
    expect(container.querySelector<HTMLInputElement>("input[role=switch]")!.disabled).toBe(false);
    expect(container.querySelector(".exp-status")?.textContent).toBe("Off. Ready to turn on.");
  });

  it("Mail triage without a mail account names it and links to the Mail screen (USE-6a)", () => {
    put([
      experiment({
        id: "mail-triage",
        title: "Mail triage",
        can_turn_on: false,
        requires: [
          { kind: "decision-model", label: "A decision model", met: true, fix_view: "models" },
          { kind: "mail-account", label: "A mail account", met: false, fix_view: "mail" },
        ],
      }),
    ]);
    mount(<Experiments />);
    expect(container.textContent).toContain("Needs a mail account.");
    expect(button("Set up")).toBeDefined();
    act(() => button("Set up")!.click());
    expect(setView).toHaveBeenCalledWith("mail");
  });

  it("an experiment that is on can always be turned off, even if its needs went away", () => {
    put([experiment({ on: true, can_turn_on: false })]);
    mount(<Experiments />);
    expect(container.querySelector<HTMLInputElement>("input[role=switch]")!.disabled).toBe(false);
  });

  it("needsLine names every missing piece and nothing else", () => {
    expect(needsLine(experiment())).toBeNull();
    expect(
      needsLine({
        requires: [
          { kind: "decision-model", label: "A decision model", met: false, fix_view: "models" },
          { kind: "provider", label: "A key for OpenRouter", met: true, fix_view: "providers" },
          { kind: "vision", label: "A decision model that can read images", met: false, fix_view: "models" },
        ],
      })
    ).toBe("Needs a decision model and a decision model that can read images");
  });
});

describe("Workspace while the experiment is off (EXP-3)", () => {
  const run = (text: string) => runCommand(parseCommandLine(text, commandViews(state()))!, state());

  it("the session menu does not offer the mode, and does once it is on", () => {
    put([experiment({ on: false })]);
    mount(<SessionMenu />);
    act(() => container.querySelector<HTMLButtonElement>(".session-more")!.click());
    expect(button("Turn on Workspace mode")).toBeUndefined();
    expect(button("Schedule this…")).toBeDefined();

    put([experiment({ on: true })]);
    expect(button("Turn on Workspace mode")).toBeDefined();
  });

  it("setWorkspaceMode(true) does nothing, and turning it off still works", () => {
    put([experiment({ on: false })], { workspaceMode: false });
    act(() => state().setWorkspaceMode(true));
    expect(state().workspaceMode).toBe(false);
    expect(state().conversations[0].workspace).toBe(true);

    put([experiment({ on: true })], { workspaceMode: true });
    act(() => state().setWorkspaceMode(false));
    expect(state().workspaceMode).toBe(false);
  });

  it("before the list is read the mode is available, as it was before experiments", () => {
    put(null, { workspaceMode: false });
    act(() => state().setWorkspaceMode(true));
    expect(state().workspaceMode).toBe(true);
  });

  it("/workspace answers with a pointer and a button, not a switch", async () => {
    put([experiment({ on: false })], { workspaceMode: false });
    const result = await run("/workspace");
    expect(result.kind).toBe("confirm");
    if (result.kind !== "confirm") return;
    expect(result.title).toBe("Workspace is an experiment");
    expect(result.body).toContain("Settings → Experiments");
    await result.run();
    expect(setView).toHaveBeenCalledWith("experiments");
    expect(state().workspaceMode).toBe(false);
  });

  it("the menu row's switch says where to go instead of switching", async () => {
    put([experiment({ on: false })], { workspaceMode: false });
    expect(controlFor("workspace", state())).toEqual({ kind: "switch", on: false });
    const result = await applyControl("workspace", "on", state());
    expect(result.kind).toBe("error");
    expect(result.kind === "error" && result.message).toContain("Settings → Experiments");
    expect(state().workspaceMode).toBe(false);
  });

  it("off is never refused, so nobody is trapped in the mode", async () => {
    put([experiment({ on: false })], { workspaceMode: true });
    const result = await run("/workspace off");
    expect(result.kind).toBe("done");
    expect(state().workspaceMode).toBe(false);
  });

  it("turning the experiment off takes the canvas away but not the chat's flag; on brings it back", async () => {
    put([experiment({ on: true })], { workspaceMode: true });
    setExperimentApi.mockResolvedValueOnce([experiment({ on: false })]);
    await act(async () => state().setExperiment("workspace", false));
    expect(state().workspaceMode).toBe(false);
    expect(state().conversations[0].workspace).toBe(true);

    setExperimentApi.mockResolvedValueOnce([experiment({ on: true })]);
    await act(async () => state().setExperiment("workspace", true));
    expect(state().workspaceMode).toBe(true);
    expect(state().toolsEnabled).toBe(true);
  });

  it("opening a Workspace chat while it is off gives the normal layout", async () => {
    put([experiment({ on: false })], { workspaceMode: false });
    expect(state().conversations[0].workspace).toBe(true);
    // The layout is decided before the messages load; those calls are not mocked here.
    await act(async () => state().setActiveConversation("c1").catch(() => {}));
    expect(state().workspaceMode).toBe(false);

    put([experiment({ on: true })], { workspaceMode: false });
    await act(async () => state().setActiveConversation("c1").catch(() => {}));
    expect(state().workspaceMode).toBe(true);
  });

  it("a Workspace chat opened while it is off shows the banner, and the button turns it on", async () => {
    put([experiment({ on: false })]);
    setExperimentApi.mockResolvedValue([experiment({ on: true })]);
    mount(<WorkspaceOffBanner />);
    expect(container.textContent).toContain("This was a Workspace session.");
    await act(async () => button("Turn Workspace on")!.click());
    expect(setExperimentApi).toHaveBeenCalledWith("workspace", true);
    expect(container.textContent).toBe("");
  });

  it("shows no banner when it is on, for a plain chat, or before the list is read", () => {
    put([experiment({ on: true })]);
    mount(<WorkspaceOffBanner />);
    expect(container.textContent).toBe("");

    put([experiment({ on: false })], {
      conversations: [{ id: "c1", title: "Plain", updatedAt: 0, messages: [] }],
    });
    expect(container.textContent).toBe("");

    put(null, { conversations: [WORKSPACE_CHAT] });
    expect(container.textContent).toBe("");
  });
});
