/**
 * @vitest-environment jsdom
 *
 * `TEAM-23`: trying to speak to a member the person has no line to is remembered (once a chat), so that
 * after three chats Poiesis can offer to draw the line. Nothing else is.
 */
import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Persona, Team } from "../../lib/api";
import { commandViews, parseCommandLine, runCommand } from "../../lib/commands";
import { membersWithoutLine } from "../../lib/members";
import { useAppStore } from "../../lib/store";

const asked = vi.fn<(conversation: string, role: string) => Promise<boolean>>(async () => true);
vi.mock("../../lib/api", async (orig) => ({
  ...(await orig<typeof import("../../lib/api")>()),
  teamAsked: (conversation: string, role: string) => asked(conversation, role),
}));

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const line = (kind: Team["lines"][number]["kind"], from: string, to: string) => ({ kind, from, to });
const team = (talks: string[]): Team => ({
  slug: "shop",
  name: "Shop team",
  line: "",
  coordinator: "persona:p0",
  style: "lean",
  member_style: "lean",
  shape: "hub",
  report: "each-item",
  limits: { at_once: 2, budget: 0, minutes: 30, rounds: 2, ask_cost: true },
  folder: "copies",
  locked: false,
  version: 1,
  members: [
    { role: "api", persona: "persona:p1", job: "", owns: [] },
    { role: "docs", persona: "helper", job: "", owns: [] },
    { role: "ui", persona: "persona:p2", job: "", owns: [] },
  ],
  lines: [
    line("talks", "you", "coordinator"),
    line("reports", "coordinator", "you"),
    ...["api", "docs", "ui"].map((to) => line("hands-over", "coordinator", to)),
    ...talks.map((to) => line("talks", "you", to)),
  ],
});

const personas = [
  { id: "p0", name: "Mira", look: null },
  { id: "p1", name: "Kai", look: null },
  { id: "p2", name: "Juno", look: null },
] as unknown as Persona[];

function put(talks: string[]) {
  act(() =>
    useAppStore.setState({
      conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], leader: "team:shop", personaId: null }],
      activeConversationId: "c1",
      teams: [team(talks)],
      personas,
      turnModifiers: {},
    } as never)
  );
}

const run = (text: string) => runCommand(parseCommandLine(text, commandViews(useAppStore.getState()))!, useAppStore.getState());

beforeEach(() => asked.mockClear());

describe("a member the person has no line to", () => {
  it("is a persona that plays a member and is not reachable: never a helper, never one with a line", () => {
    expect(membersWithoutLine(team([]), personas).map((m) => [m.role, m.name])).toEqual([
      ["api", "Kai"],
      ["ui", "Juno"],
    ]);
    expect(membersWithoutLine(team(["api"]), personas).map((m) => m.role)).toEqual(["ui"]);
    expect(membersWithoutLine(team(["api", "ui"]), personas)).toEqual([]);
  });

  it("is remembered when /ask names it, and the answer says what to do", async () => {
    put([]);
    expect(await run("/ask kai is the copy fine?")).toEqual({
      kind: "error",
      message: "You have no line to Kai in Shop team. Draw one in Personas & teams.",
    });
    expect(asked).toHaveBeenCalledWith("c1", "api");
  });

  it("is remembered when only some members have a line, for the one that has none", async () => {
    put(["api"]);
    await run("/ask juno hello");
    expect(asked).toHaveBeenCalledWith("c1", "ui");
  });

  it("is not remembered when there is a line, or the name is a helper's, or nobody's", async () => {
    put(["api"]);
    expect(await run("/ask kai hello")).toMatchObject({ kind: "modifier" });
    await run("/ask docs hello");
    await run("/ask zed hello");
    expect(asked).not.toHaveBeenCalled();
  });
});
