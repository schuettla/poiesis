/**
 * `REG-T1`, `CMP-T1`, `CPX-T2`: the registry is one manifest, every entry in it
 * does something, and what a user types is read the way the plan says.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const voice = vi.hoisted(() => ({ openVoice: vi.fn(() => Promise.resolve("live")) }));
vi.mock("./voice/controller", () => voice);
import {
  HANDLERS,
  SPECS,
  argChoices,
  commandViews,
  parseCommandLine,
  rankCommands,
  runCommand,
  visibleViews,
} from "./commands";
import { useAppStore } from "./store";
import type { SkillView } from "./api";

function skill(name: string, over: Partial<SkillView> = {}): SkillView {
  return {
    name,
    description: `does ${name}`,
    when_to_use: null,
    source: "personal",
    dir: `/skills/${name}`,
    enabled: true,
    unsupported: [],
    used: 0,
    rough: 0,
    risk: 0,
    risk_flags: [],
    ...over,
  };
}

function views() {
  return commandViews(useAppStore.getState());
}

beforeEach(() => {
  useAppStore.setState({
    skills: [],
    turnModifiers: {},
    conversations: [],
    activeConversationId: null,
    activeRun: null,
    busy: false,
    changeSets: {},
    commandNotes: {},
  });
});

describe("the manifest (REG-T1)", () => {
  it("names are unique across commands and aliases", () => {
    const seen = new Set<string>();
    for (const spec of SPECS) {
      for (const n of [spec.name, ...spec.aliases]) {
        expect(seen.has(n), `${n} is declared twice`).toBe(false);
        seen.add(n);
      }
    }
  });

  it("every command that is not a skill has a handler, and every handler has a command", () => {
    const withHandlers = SPECS.filter((s) => s.kind !== "skill").map((s) => s.name);
    for (const name of withHandlers) {
      expect(HANDLERS[name], `/${name} has no handler`).toBeTypeOf("function");
    }
    const names = new Set(SPECS.map((s) => s.name));
    for (const key of Object.keys(HANDLERS)) {
      expect(names.has(key), `handler ${key} has no command`).toBe(true);
    }
  });

  it("an agent class is required exactly when the agent can call it", () => {
    for (const spec of SPECS) {
      if (spec.who === "both") expect(spec.agentClass, spec.name).toBeTruthy();
    }
  });

  it("every first-person section speaks as 'I' (CPX-T2)", () => {
    // `/autonomy` is "How much I may change without asking" in the plan's copy
    // table, which is first person even though it does not open with the word.
    const ok = /^(I\b|Let me |Visit me|How much I )/;
    for (const spec of SPECS.filter((s) => s.section === "me")) {
      expect(spec.summary, `/${spec.name}`).toMatch(ok);
    }
  });
});

describe("reading what was typed (CMP-T1)", () => {
  it("takes a known name, with and without arguments", () => {
    const v = views();
    expect(parseCommandLine("/compact", v)).toMatchObject({ name: "compact", args: "" });
    expect(parseCommandLine("/compact the budget talk", v)).toMatchObject({
      name: "compact",
      args: "the budget talk",
    });
  });

  it("takes an alias and reports the real name", () => {
    expect(parseCommandLine("/clear", views())?.name).toBe("new");
  });

  it("leaves paths and unknown words alone", () => {
    const v = views();
    expect(parseCommandLine("/usr/bin/env", v)).toBeNull();
    expect(parseCommandLine("/nonsense", v)).toBeNull();
    expect(parseCommandLine("/", v)).toBeNull();
  });

  it("ignores a slash that is not at the start", () => {
    expect(parseCommandLine("see /compact for that", views())).toBeNull();
    expect(parseCommandLine("2026/08/10", views())).toBeNull();
  });

  it("only knows the skills that are enabled, and never lets one shadow a built-in", () => {
    useAppStore.setState({
      skills: [skill("weekly-report"), skill("off-one", { enabled: false }), skill("compact")],
    });
    const v = views();
    expect(parseCommandLine("/weekly-report now", v)?.spec.source).toBe("skill");
    expect(parseCommandLine("/off-one", v)).toBeNull();
    expect(parseCommandLine("/compact", v)?.spec.source).toBe("builtin");
  });
});

describe("what is available right now (Rule 5)", () => {
  it("hides /stop and /continue until they mean something", () => {
    const names = visibleViews(views()).map((v) => v.name);
    expect(names).not.toContain("stop");
    expect(names).not.toContain("continue");
  });

  it("keeps a command that needs a folder, dimmed, with the reason", () => {
    const init = views().find((v) => v.name === "init")!;
    expect(init.hidden).toBe(false);
    expect(init.disabledReason).toBeTruthy();
  });

  it("says why /undo is unavailable when nothing changed", () => {
    const undo = views().find((v) => v.name === "undo")!;
    expect(undo.disabledReason).not.toBeNull();
  });

  it("refuses to run a disabled command", async () => {
    const parsed = parseCommandLine("/undo", views())!;
    const result = await runCommand(parsed, useAppStore.getState());
    expect(result.kind).toBe("error");
  });
});

describe("/talk (VOC-UI-2)", () => {
  it("opens voice mode in a conversation, and is known as /voice too", async () => {
    voice.openVoice.mockClear();
    expect(views().find((v) => v.name === "talk")!.disabledReason).toBeTruthy();
    useAppStore.setState({ activeConversationId: "c1", conversations: [{ id: "c1", messages: [] } as never] });
    const parsed = parseCommandLine("/voice", views())!;
    expect(parsed.name).toBe("talk");
    expect(await runCommand(parsed, useAppStore.getState())).toEqual({ kind: "done", note: undefined });
    expect(voice.openVoice).toHaveBeenCalledOnce();
  });

  it("says why not in workspace mode, where the voice surface is not shown", async () => {
    voice.openVoice.mockClear();
    useAppStore.setState({ activeConversationId: "c1", conversations: [{ id: "c1", messages: [] } as never], workspaceMode: true });
    const result = await runCommand(parseCommandLine("/talk", views())!, useAppStore.getState());
    expect(result.kind).toBe("error");
    expect(voice.openVoice).not.toHaveBeenCalled();
    useAppStore.setState({ workspaceMode: false });
  });
});

describe("ranking", () => {
  it("puts a name hit before a summary hit", () => {
    const ranked = rankCommands(views(), "model").map((v) => v.name);
    expect(ranked[0]).toBe("model");
  });

  it("finds a command by its alias", () => {
    expect(rankCommands(views(), "make-skill").map((v) => v.name)).toContain("skillify");
  });
});

describe("modifier commands (UCM-4)", () => {
  it("/effort sets a chip, and sends what follows it", async () => {
    const parsed = parseCommandLine("/effort high add tests", views())!;
    const result = await runCommand(parsed, useAppStore.getState());
    expect(result).toEqual({ kind: "modifier", send: "add tests" });
    expect(useAppStore.getState().turnModifiers).toEqual({ effort: "high" });
  });

  it("/effort with nothing after it only sets the chip", async () => {
    const result = await runCommand(parseCommandLine("/effort low", views())!, useAppStore.getState());
    expect(result).toEqual({ kind: "modifier", send: undefined });
  });

  it("/effort refuses a word it does not know", async () => {
    const result = await runCommand(parseCommandLine("/effort heroic", views())!, useAppStore.getState());
    expect(result.kind).toBe("error");
    expect(useAppStore.getState().turnModifiers).toEqual({});
  });

  it("/steps clamps to the same 1-50 the backend does", async () => {
    await runCommand(parseCommandLine("/steps 400", views())!, useAppStore.getState());
    expect(useAppStore.getState().turnModifiers.maxSteps).toBe(50);
  });

  it("/steps wants a number", async () => {
    const result = await runCommand(parseCommandLine("/steps many", views())!, useAppStore.getState());
    expect(result.kind).toBe("error");
  });
});

describe("a skill is a command (SKC-2)", () => {
  it("sends the words after it, with the skill named", async () => {
    useAppStore.setState({ skills: [skill("weekly-report")] });
    const parsed = parseCommandLine("/weekly-report for the team", views())!;
    expect(await runCommand(parsed, useAppStore.getState())).toEqual({
      kind: "send",
      text: "for the team",
      skill: "weekly-report",
      skillArgs: "for the team",
    });
  });

  it("says Run /name when there is nothing after it", async () => {
    useAppStore.setState({ skills: [skill("weekly-report")] });
    const parsed = parseCommandLine("/weekly-report", views())!;
    expect(await runCommand(parsed, useAppStore.getState())).toEqual({
      kind: "send",
      text: "Run /weekly-report",
      skill: "weekly-report",
      skillArgs: undefined,
    });
  });
});

describe("what the agent can call (AGC, REG-T1)", () => {
  it("is exactly the harness's set, and every one names its class", () => {
    const both = SPECS.filter((s) => s.who === "both").map((s) => s.name).sort();
    expect(both).toEqual(["compact", "plan", "schedule", "workspace"]);
    for (const spec of SPECS.filter((s) => s.who === "both")) {
      expect(["context", "suggest", "modes", "schedule"], spec.name).toContain(spec.agentClass);
    }
  });

  it("lets me suggest a command without letting me run it", () => {
    for (const name of ["skillify", "reflect", "checkup"]) {
      const spec = SPECS.find((s) => s.name === name)!;
      expect(spec.suggestible, name).toBe(true);
      expect(spec.who, `${name} is the user's to run`).toBe("user");
    }
  });
});

describe("/plan (PLF)", () => {
  it("sets the Plan first chip, and sends what follows it at once", async () => {
    const result = await runCommand(parseCommandLine("/plan refactor the parser", views())!, useAppStore.getState());
    expect(result).toEqual({ kind: "modifier", send: "refactor the parser" });
    expect(useAppStore.getState().turnModifiers.planFirst).toBe(true);
  });

  it("with nothing after it only sets the chip", async () => {
    const result = await runCommand(parseCommandLine("/plan", views())!, useAppStore.getState());
    expect(result).toEqual({ kind: "modifier", send: undefined });
  });

  it("a chip set by hand is not marked as my default", async () => {
    useAppStore.setState({ turnModifiers: { planFirst: true, planFirstIsDefault: true } });
    await runCommand(parseCommandLine("/plan", views())!, useAppStore.getState());
    expect(useAppStore.getState().turnModifiers.planFirstIsDefault).toBeUndefined();
  });
});

describe("/btw, /checkup (BTW, CHK)", () => {
  it("/btw needs a question", async () => {
    useAppStore.setState({
      conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
      activeConversationId: "c1",
    });
    const result = await runCommand(parseCommandLine("/btw", views())!, useAppStore.getState());
    expect(result.kind).toBe("needsArg");
  });

  it("both only work in the desktop app, and say so", async () => {
    useAppStore.setState({
      conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
      activeConversationId: "c1",
    });
    const btw = await runCommand(parseCommandLine("/btw which file was it", views())!, useAppStore.getState());
    expect(btw.kind).toBe("error");
    const checkup = await runCommand(parseCommandLine("/checkup", views())!, useAppStore.getState());
    expect(checkup.kind).toBe("error");
  });
});

describe("/rewind (RWD-UI-3)", () => {
  const talk = (id: string, role: "user" | "assistant", text: string) => ({
    id,
    role,
    text,
    createdAt: 0,
  });
  beforeEach(() => {
    useAppStore.setState({
      conversations: [
        {
          id: "c1",
          title: "t",
          updatedAt: 0,
          messages: [
            talk("m1", "user", "first question"),
            talk("m2", "assistant", "first answer"),
            talk("m3", "user", "second question\nwith a second line"),
            talk("m4", "assistant", "second answer"),
          ],
        },
      ],
      activeConversationId: "c1",
      rewindRequest: null,
    });
  });

  it("lists your messages newest first, one line each", () => {
    const choices = argChoices("turns", "", useAppStore.getState());
    expect(choices.map((c) => c.label)).toEqual(["1 · second question", "2 · first question"]);
    expect(choices.map((c) => c.value)).toEqual(["1", "2"]);
  });

  it("opens the dialog for the turn you chose", async () => {
    const result = await runCommand(parseCommandLine("/rewind 2", views())!, useAppStore.getState());
    expect(result.kind).toBe("done");
    expect(useAppStore.getState().rewindRequest).toBe("m1");
  });

  it("asks which turn when you did not say, and refuses one that is not there", async () => {
    expect((await runCommand(parseCommandLine("/rewind", views())!, useAppStore.getState())).kind).toBe("needsArg");
    expect((await runCommand(parseCommandLine("/rewind 9", views())!, useAppStore.getState())).kind).toBe("error");
    expect(useAppStore.getState().rewindRequest).toBeNull();
  });
});
