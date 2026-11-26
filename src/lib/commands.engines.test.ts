/**
 * `CLI-T-11`: `/delegate [agent] task` and `/external on|off` (`CLI-UI-6`). Choosing
 * an agent goes through the same `/` menu as everything else; there is no other
 * syntax.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { argChoices, commandViews, parseCommandLine, runCommand, splitLead, splitOutside, visibleViews } from "./commands";
import { outsideToken } from "./engines";
import { turnRunOptions, useAppStore } from "./store";
import type { DelegateTarget, OutsideSession } from "./api";

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

/** `XA-14`: `/resume [session] task` picks up a Claude Code or Codex session made outside Poiesis. */
describe("/resume [session] task", () => {
  const NOW = 1_794_500_000_000;
  const session = (over: Partial<OutsideSession>): OutsideSession => ({
    engine_id: "claude-code",
    agent: "claude-code",
    name: "Claude Code",
    session_id: "11111111-2222-4333-8444-555555555555",
    title: "Fix the login",
    last_at_ms: NOW - 2 * 3_600_000,
    token: "cc-55555555",
    ...over,
  });
  const SESSIONS: OutsideSession[] = [
    session({}),
    session({ engine_id: "codex", agent: "codex", name: "Codex", session_id: "01a1d060-4ab7-72b2-b02b-1dbee1d5f52c", title: "Port the parser", last_at_ms: NOW - 30 * 3_600_000, token: "cx-e1d5f52c" }),
  ];

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    useAppStore.setState({
      outsideSessions: SESSIONS,
      conversations: [{ id: "c1", messages: [], folderPath: "C:\work\app" } as never],
      delegateTargets: [target({}), target({ slug: "codex", name: "Codex", engine_id: "codex" })],
    });
  });
  afterEach(() => vi.useRealTimers());

  it("lists the sessions as name · title with their age, newest as given", () => {
    const rows = argChoices("outside", "", useAppStore.getState());
    expect(rows.map((c) => [c.value, c.label, c.hint])).toEqual([
      ["cc-55555555", "Claude Code · Fix the login", "2 h ago"],
      ["cx-e1d5f52c", "Codex · Port the parser", "yesterday"],
    ]);
  });

  it("filters on what is typed, never by fuzzy luck", () => {
    expect(argChoices("outside", "cx-", useAppStore.getState()).map((c) => c.value)).toEqual([SESSIONS[1].token]);
    expect(argChoices("outside", "parser", useAppStore.getState()).map((c) => c.label)).toEqual(["Codex · Port the parser"]);
    expect(argChoices("outside", "add tests", useAppStore.getState())).toEqual([]);
  });

  it("names the session first and the task after it; any other first word is the task", () => {
    expect(splitOutside("cc-55555555 add tests", SESSIONS)).toMatchObject({ session: { title: "Fix the login" }, task: "add tests" });
    expect(splitOutside("CC-55555555", SESSIONS)).toMatchObject({ session: { name: "Claude Code" }, task: "" });
    expect(splitOutside("add tests", SESSIONS)).toEqual({ session: null, task: "add tests" });
  });

  it("asks what to do next when only the session is named, and keeps the word in the box", async () => {
    expect(await run("/resume cc-55555555")).toEqual({
      kind: "needsArg",
      message: 'What should Claude Code do next in "Fix the login"?',
      keep: "cc-55555555",
    });
  });

  it("sends as /delegate to that session's agent, with the session to continue", async () => {
    expect(await run("/resume cc-55555555 add tests")).toEqual({
      kind: "send",
      text: "add tests",
      skill: "delegate",
      skillArgs: "add tests",
      delegateTo: "claude-code",
      resumeSession: "11111111-2222-4333-8444-555555555555",
    });
  });

  it("says so when the first word is no session of this folder", async () => {
    expect(await run("/resume zz-00000000 add tests")).toEqual({
      kind: "error",
      message: "I don't know that session. Type /resume and pick one from the list.",
    });
    expect(await run("/resume add tests")).toMatchObject({ kind: "error" });
  });

  it("refuses when the agent cannot take it now, with the reason", async () => {
    useAppStore.setState({ delegateTargets: [target({ disabled_reason: "It hit its plan limit, back at 14:30" })] });
    const r = await run("/resume cc-55555555 add tests");
    expect(r.kind === "error" && r.message).toContain("it hit its plan limit, back at 14:30");
  });

  it("the session goes to the backend for this message only", () => {
    expect(turnRunOptions({}, { skill: "delegate", skillArgs: "x", delegateTo: "codex", resumeSession: "abc" })).toMatchObject({ delegateTo: "codex", resumeSession: "abc" });
    expect(turnRunOptions({}, { skill: "delegate", skillArgs: "x", delegateTo: "codex" })?.resumeSession).toBeUndefined();
  });

  it("is on the menu under the helpers, and says the first word is a session", () => {
    const v = views().find((x) => x.name === "resume")!;
    expect(v.args).toMatchObject({ lead: "outside", hint: "[session] task" });
    expect(v.section).toBe("agents");
    expect(v.disabledReason).toBeNull();
    // no working folder: it says why
    useAppStore.setState({ conversations: [{ id: "c1", messages: [] } as never] });
    expect(views().find((x) => x.name === "resume")!.disabledReason).toBe("needs a working folder");
  });

  it("the one word is built the same way for a job's own session", () => {
    expect(outsideToken("claude-code", "11111111-2222-4333-8444-555555555555")).toBe("cc-55555555");
    expect(outsideToken("codex", "01a1d060-4ab7-72b2-b02b-1dbee1d5f52c")).toBe("cx-e1d5f52c");
    expect(outsideToken("acp:opencode", "ses_1")).toBeNull();
  });
});
