/**
 * `GOL`, `UCM-8`, `UCM-9`, and a connector's prompts as commands: the longer
 * horizons. `/goal` only ever acts for the user, `/schedule` saves nothing,
 * `/export` asks where, and a connector's prompt is sent as the server built it.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const getMcpPrompt = vi.fn();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  getMcpPrompt: (...args: unknown[]) => getMcpPrompt(...args),
}));

import { commandViews, parseCommandLine, parsePromptArgs, runCommand } from "./commands";
import { useAppStore } from "./store";
import { newGoal } from "./goal";
import type { McpPromptView } from "./api";

const startGoal = vi.fn(() => Promise.resolve());
const stopGoal = vi.fn();
const exportConversation = vi.fn(() => Promise.resolve<string | null>("C:/notes/trip.md"));

function prompt(name: string, over: Partial<McpPromptView> = {}): McpPromptView {
  return {
    connector_id: "k1",
    connector_name: "Notes",
    name,
    title: null,
    description: `does ${name}`,
    arguments: [],
    ...over,
  };
}

beforeEach(() => {
  startGoal.mockClear();
  stopGoal.mockClear();
  exportConversation.mockClear();
  getMcpPrompt.mockReset();
  useAppStore.setState({
    skills: [],
    mcpPrompts: [],
    goals: {},
    turnModifiers: {},
    conversations: [{ id: "c1", title: "Plan the trip", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    activeRun: null,
    busy: false,
    changeSets: {},
    commandNotes: {},
    taskDraft: null,
    startGoal,
    stopGoal,
    exportConversation,
  });
});

async function run(text: string) {
  const s = useAppStore.getState();
  const cmd = parseCommandLine(text, commandViews(s));
  if (!cmd) throw new Error(`not a command: ${text}`);
  return runCommand(cmd, s);
}

describe("/goal (GOL-1)", () => {
  it("starts a goal with its condition, and does not wait for it", async () => {
    const r = await run("/goal make the tests pass until npm test exits 0");
    expect(r).toMatchObject({ kind: "done" });
    expect(startGoal).toHaveBeenCalledWith("c1", "make the tests pass", "npm test exits 0");
  });

  it("without a condition the goal is its own condition", async () => {
    await run("/goal tidy the docs folder");
    expect(startGoal).toHaveBeenCalledWith("c1", "tidy the docs folder", "tidy the docs folder");
  });

  it("asks for the goal when none was given", async () => {
    const r = await run("/goal");
    expect(r).toMatchObject({ kind: "needsArg" });
    expect(startGoal).not.toHaveBeenCalled();
  });

  it("one goal at a time in a chat", async () => {
    useAppStore.setState({ goals: { c1: newGoal("a", "a") } });
    const r = await run("/goal b");
    expect(r).toMatchObject({ kind: "error", message: expect.stringContaining("/goal stop") });
    expect(startGoal).not.toHaveBeenCalled();
  });

  it("will not start while I am busy", async () => {
    useAppStore.setState({ busy: true });
    const r = await run("/goal b");
    expect(r).toMatchObject({ kind: "error" });
    expect(startGoal).not.toHaveBeenCalled();
  });

  it("stop ends it, even while a round is running", async () => {
    useAppStore.setState({ goals: { c1: newGoal("a", "a") }, busy: true });
    const r = await run("/goal stop");
    expect(r).toMatchObject({ kind: "done" });
    expect(stopGoal).toHaveBeenCalledWith("c1");
  });

  it("stop with no goal says there is none", async () => {
    const r = await run("/goal stop");
    expect(r).toMatchObject({ kind: "error", message: expect.stringContaining("not working toward") });
    expect(stopGoal).not.toHaveBeenCalled();
  });

  it("is the user's: not something the agent may call", () => {
    const spec = commandViews(useAppStore.getState()).find((v) => v.name === "goal");
    expect(spec).toMatchObject({ who: "user", trace: true });
    expect(spec?.agentClass).toBeUndefined();
  });
});

describe("/schedule (UCM-8)", () => {
  it("opens a draft with the time read in and the task apart, and saves nothing", async () => {
    const r = await run("/schedule every 6 hours sync my notes");
    expect(r).toMatchObject({ kind: "done" });
    const s = useAppStore.getState();
    expect(s.view).toBe("tasks");
    expect(s.taskDraft).toEqual({
      name: "Plan the trip",
      prompt: "sync my notes",
      conversationId: "c1",
      cadence: "six-hourly",
    });
  });

  it("says where it could not do what was asked", async () => {
    await run("/schedule every weekday at 9 summarise my mail");
    expect(useAppStore.getState().taskDraft).toMatchObject({
      prompt: "summarise my mail",
      cadence: "daily",
      whenNote: expect.stringContaining("weekends"),
    });
  });

  it("leaves the field empty when there is no time to read", async () => {
    await run("/schedule summarise my mail");
    expect(useAppStore.getState().taskDraft).toMatchObject({ prompt: "summarise my mail", cadence: null });
  });
});

describe("/export (UCM-9)", () => {
  it("asks where to save it, for this conversation", async () => {
    const r = await run("/export");
    expect(r).toMatchObject({ kind: "done" });
    expect(exportConversation).toHaveBeenCalledWith("c1");
  });

  it("needs a conversation", async () => {
    useAppStore.setState({ conversations: [], activeConversationId: null });
    const spec = commandViews(useAppStore.getState()).find((v) => v.name === "export");
    expect(spec?.disabledReason).toBeTruthy();
  });
});

describe("a connector's prompts as commands", () => {
  it("sit beside the skills, and say which connector they came from", () => {
    useAppStore.setState({ mcpPrompts: [prompt("summarise")] });
    const v = commandViews(useAppStore.getState()).find((x) => x.name === "summarise");
    expect(v).toMatchObject({
      source: "mcp",
      section: "skills",
      kind: "skill",
      summary: "from Notes: does summarise",
    });
  });

  it("never shadow a built-in, a skill or each other: the connector's name goes in front", () => {
    useAppStore.setState({
      skills: [
        {
          name: "review-pr",
          description: "x",
          when_to_use: null,
          source: "personal",
          dir: "/s",
          enabled: true,
          unsupported: [],
          used: 0,
          rough: 0,
          risk: 0,
          risk_flags: [],
        },
      ],
      mcpPrompts: [
        prompt("plan"),
        prompt("review-pr"),
        prompt("daily", { connector_name: "Work Notes" }),
        prompt("daily", { connector_id: "k2", connector_name: "Home" }),
      ],
    });
    const names = commandViews(useAppStore.getState())
      .filter((v) => v.source === "mcp")
      .map((v) => v.name);
    expect(names).toEqual(["Notes-plan", "Notes-review-pr", "daily", "Home-daily"]);
    // The built-in /plan is still the built-in.
    expect(commandViews(useAppStore.getState()).find((v) => v.name === "plan")?.source).toBe("builtin");
  });

  it("never take a built-in's alias either: /clear stays the built-in", () => {
    useAppStore.setState({ mcpPrompts: [prompt("clear")] });
    const views = commandViews(useAppStore.getState());
    expect(views.filter((v) => v.source === "mcp").map((v) => v.name)).toEqual(["Notes-clear"]);
    expect(parseCommandLine("/clear", views)?.spec.source).toBe("builtin");
  });

  it("send the text the server built, with what was typed filled in", async () => {
    useAppStore.setState({
      mcpPrompts: [prompt("summarise", { arguments: [{ name: "topic", description: "", required: true }] })],
    });
    getMcpPrompt.mockResolvedValue("Summarise the notes about the trip.");
    const r = await run("/summarise the trip");
    expect(getMcpPrompt).toHaveBeenCalledWith("k1", "summarise", { topic: "the trip" });
    expect(r).toEqual({ kind: "send", text: "Summarise the notes about the trip." });
  });

  it("go back to the box when a required argument is missing", async () => {
    useAppStore.setState({
      mcpPrompts: [prompt("summarise", { arguments: [{ name: "topic", description: "", required: true }] })],
    });
    const r = await run("/summarise");
    expect(r).toMatchObject({ kind: "needsArg", message: expect.stringContaining("topic") });
    expect(getMcpPrompt).not.toHaveBeenCalled();
  });

  it("say so when the server cannot build it", async () => {
    useAppStore.setState({ mcpPrompts: [prompt("hello")] });
    getMcpPrompt.mockRejectedValue("Notes is turned off.");
    const r = await run("/hello");
    expect(r).toMatchObject({ kind: "error", message: expect.stringContaining("Notes couldn't give me that prompt") });
  });
});

describe("filling a prompt's arguments", () => {
  const defs = [
    { name: "topic", description: "", required: true },
    { name: "tone", description: "", required: false },
  ];

  it("takes plain words for the first argument", () => {
    expect(parsePromptArgs("the trip", defs)).toEqual({ values: { topic: "the trip" }, missing: [] });
  });

  it("takes name=value, quoted when it has spaces, and the rest for what is still empty", () => {
    expect(parsePromptArgs('tone="very dry" the trip', defs)).toEqual({
      values: { tone: "very dry", topic: "the trip" },
      missing: [],
    });
    expect(parsePromptArgs("topic=rome tone=warm", defs).values).toEqual({ topic: "rome", tone: "warm" });
  });

  it("leaves an unknown name=value in the words", () => {
    expect(parsePromptArgs("compare a=b", defs).values).toEqual({ topic: "compare a=b" });
  });

  it("names what is still missing", () => {
    expect(parsePromptArgs("", defs).missing).toEqual(["topic"]);
    expect(parsePromptArgs("tone=warm", defs).missing).toEqual(["topic"]);
    expect(parsePromptArgs("", []).missing).toEqual([]);
  });
});
