/**
 * `CLI-T-11`: `/delegate [agent] task` and `/external on|off` (`CLI-UI-6`). Choosing
 * an agent goes through the same `/` menu as everything else; there is no other
 * syntax.
 */
import { beforeEach, describe, expect, it } from "vitest";

import { argChoices, commandViews, parseCommandLine, runCommand, splitLead, visibleViews } from "./commands";
import { turnRunOptions, useAppStore } from "./store";
import type { DelegateTarget } from "./api";

const target = (over: Partial<DelegateTarget>): DelegateTarget => ({
  slug: "claude-code",
  name: "Claude Code",
  kind: "engine",
  engine_id: "claude-code",
  hint: "Agent app",
  disabled_reason: null,
  ...over,
});

const TARGETS: DelegateTarget[] = [
  target({ slug: "general", name: "General", kind: "general", engine_id: null, hint: "Same tools as me" }),
  target({ slug: "refactorer", name: "Refactorer", kind: "persona", engine_id: null, hint: "Renames code" }),
  target({}),
  target({ slug: "opencode", name: "OpenCode", engine_id: "acp:opencode" }),
  target({ slug: "codex", name: "Codex", engine_id: "codex", disabled_reason: "Off. Turn it on in External agents" }),
];

const views = () => commandViews(useAppStore.getState());
const run = (line: string) => runCommand(parseCommandLine(line, views())!, useAppStore.getState());

beforeEach(() => {
  useAppStore.setState({
    skills: [],
    turnModifiers: {},
    activeConversationId: "c1",
    conversations: [{ id: "c1", messages: [] } as never],
    busy: false,
    activeRun: null,
    delegateTargets: TARGETS,
    engines: [{ id: "claude-code", enabled: true, delegate_ready: true } as never],
    view: "chat",
    engineFocus: null,
  });
});

describe("/delegate [agent] task", () => {
  it("names the agent first and the task after it", () => {
    expect(splitLead("claude-code fix the tests", TARGETS)).toMatchObject({ agent: { slug: "claude-code" }, task: "fix the tests" });
    expect(splitLead("Claude-Code fix it", TARGETS).agent?.slug).toBe("claude-code");
  });

  it("a first word that is no agent is part of the task", () => {
    expect(splitLead("fix the tests", TARGETS)).toEqual({ agent: null, task: "fix the tests" });
    expect(splitLead("codexify the file", TARGETS).agent).toBeNull();
  });

  it("sends the task with the agent as `delegateTo`", async () => {
    expect(await run("/delegate claude-code fix the tests")).toEqual({
      kind: "send",
      text: "fix the tests",
      skill: "delegate",
      skillArgs: "fix the tests",
      delegateTo: "claude-code",
    });
  });

  it("with no agent named, sends what it always did", async () => {
    expect(await run("/delegate fix the tests")).toEqual({
      kind: "send",
      text: "fix the tests",
      skill: "delegate",
      skillArgs: "fix the tests",
    });
  });

  it("asks what to do when only the agent is named, and keeps the agent in the box", async () => {
    expect(await run("/delegate opencode")).toEqual({ kind: "needsArg", message: "What should OpenCode do?", keep: "opencode" });
  });

  it("refuses an agent that cannot be used here, with the reason", async () => {
    const r = await run("/delegate codex fix it");
    expect(r.kind).toBe("error");
    expect(r.kind === "error" && r.message).toContain("off. turn it on in external agents");
  });

  it("the agent goes to the backend for this message only", () => {
    expect(turnRunOptions({}, { skill: "delegate", skillArgs: "x", delegateTo: "opencode" })).toMatchObject({ delegateTo: "opencode" });
    expect(turnRunOptions({}, { skill: "delegate", skillArgs: "x" })?.delegateTo).toBeUndefined();
  });

  it("lists the general agent, personas and found agent apps, a disabled one with its reason", () => {
    const all = argChoices("agents", "", useAppStore.getState());
    expect(all.map((c) => c.value)).toEqual(["general", "refactorer", "claude-code", "opencode", "codex"]);
    const off = all.find((c) => c.value === "codex")!;
    expect(off.disabledReason).toBe("Off. Turn it on in External agents");
    expect(off.engineId).toBe("codex");
  });

  it("filters on what is typed, not by fuzzy luck", () => {
    expect(argChoices("agents", "op", useAppStore.getState()).map((c) => c.value)).toEqual(["opencode"]);
    expect(argChoices("agents", "claude", useAppStore.getState()).map((c) => c.value)).toEqual(["claude-code"]);
    expect(argChoices("agents", "fix", useAppStore.getState())).toEqual([]);
  });

  it("says in the menu that the first word may be an agent", () => {
    const v = views().find((x) => x.name === "delegate")!;
    expect(v.args).toMatchObject({ lead: "agents", hint: "[agent] task" });
    expect(v.summary).toBe("Hand this to an agent and keep talking");
  });
});

describe("/external on|off (RTE-5)", () => {
  it("turns agent apps off for this chat and on again", async () => {
    expect(await run("/external off")).toMatchObject({ kind: "done" });
    expect(useAppStore.getState().conversations[0].enginesOff).toBe(true);
    expect(await run("/external on")).toMatchObject({ kind: "done" });
    expect(useAppStore.getState().conversations[0].enginesOff).toBe(false);
  });

  it("asks for on or off, and refuses anything else", async () => {
    expect((await run("/external")).kind).toBe("needsArg");
    expect((await run("/external maybe")).kind).toBe("error");
  });

  it("is hidden until an agent app can take a job, and says why if typed anyway", async () => {
    useAppStore.setState({ engines: [{ id: "claude-code", enabled: false, delegate_ready: true } as never] });
    expect(visibleViews(views()).some((v) => v.name === "external")).toBe(false);
    const r = await run("/external off");
    expect(r).toEqual({ kind: "error", message: "No external agents are set up. See Settings → External agents." });
    // An agent that is on but cannot take jobs yet does not count either.
    useAppStore.setState({ engines: [{ id: "codex", enabled: true, delegate_ready: false } as never] });
    expect(visibleViews(views()).some((v) => v.name === "external")).toBe(false);
  });

  it("shows on the menu row as a switch that follows the chat", () => {
    expect(visibleViews(views()).some((v) => v.name === "external")).toBe(true);
  });
});
