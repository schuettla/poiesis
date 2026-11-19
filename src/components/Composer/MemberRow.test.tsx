/**
 * @vitest-environment jsdom
 *
 * `TEAM-17`: who the next message goes to, in a session a team leads. The row is there only when the
 * person has a line to a member besides the coordinator; pressing a face addresses the next message
 * to that member, and `/ask` does the same from the menu.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Persona, Team } from "../../lib/api";
import { addressableMembers, matchMember } from "../../lib/members";
import { HANDLERS, commandViews, parseCommandLine, runCommand, argChoices } from "../../lib/commands";
import { turnRunOptions, useAppStore } from "../../lib/store";
import MemberRow from "./MemberRow";

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
    line("hands-over", "coordinator", "api"),
    line("hands-over", "coordinator", "docs"),
    line("hands-over", "coordinator", "ui"),
    ...talks.map((to) => line("talks", "you", to)),
  ],
});

const personas = [
  { id: "p0", name: "Mira", look: null },
  { id: "p1", name: "Kai", look: null },
  { id: "p2", name: "Juno", look: null },
] as unknown as Persona[];

let container: HTMLDivElement;
let root: Root;

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

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = () => act(() => root.render(<MemberRow />));
const picks = () => [...container.querySelectorAll<HTMLButtonElement>(".member-row-pick")];

describe("who may be spoken to", () => {
  it("is a persona the team draws a talks line to, and never a helper or someone with no line", () => {
    expect(addressableMembers(team(["api", "docs", "ui"]), personas).map((m) => [m.role, m.name])).toEqual([
      ["api", "Kai"],
      ["ui", "Juno"],
    ]);
    expect(addressableMembers(team([]), personas)).toEqual([]);
    // A persona that was deleted has no voice left.
    expect(addressableMembers(team(["api"]), [])).toEqual([]);
  });

  it("reads a name first in what is typed, by persona name or role", () => {
    const members = addressableMembers(team(["api", "ui"]), personas);
    expect(matchMember(members, "kai is the copy fine?")?.member.role).toBe("api");
    expect(matchMember(members, "UI check it")?.rest).toBe("check it");
    expect(matchMember(members, "juno")).toMatchObject({ member: { role: "ui" }, rest: "" });
    expect(matchMember(members, "nobody hello")).toBeNull();
  });
});

describe("the row above the composer", () => {
  it("is not shown in a Hub, where the person only talks to the coordinator", () => {
    put([]);
    render();
    expect(container.querySelector(".member-row")).toBeNull();
  });

  it("is not shown in a chat no team leads", () => {
    put(["api"]);
    act(() => useAppStore.setState({ conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], leader: "poiesis", personaId: null }] } as never));
    render();
    expect(container.querySelector(".member-row")).toBeNull();
  });

  it("starts on the coordinator; pressing a face addresses the next message, pressing the coordinator takes it back", () => {
    put(["api", "ui"]);
    render();
    expect(container.querySelector(".member-row-label")?.textContent).toBe("To:");
    expect(picks().map((b) => b.querySelector(":scope > span")?.nextElementSibling?.textContent ?? b.lastElementChild?.textContent)).toEqual(["Mira", "Kai", "Juno"]);
    expect(picks().map((b) => b.getAttribute("aria-pressed"))).toEqual(["true", "false", "false"]);

    act(() => picks()[1].click());
    expect(useAppStore.getState().turnModifiers.addressed).toBe("api");
    expect(picks().map((b) => b.getAttribute("aria-pressed"))).toEqual(["false", "true", "false"]);

    act(() => picks()[0].click());
    expect(useAppStore.getState().turnModifiers.addressed).toBeUndefined();
  });

  it("sends who it is for with the message, and only that message", () => {
    put(["api"]);
    render();
    act(() => picks()[1].click());
    expect(turnRunOptions(useAppStore.getState().turnModifiers)).toEqual({ addressed: "api" });
    expect(turnRunOptions({})).toBeUndefined();
  });
});

describe("/ask", () => {
  const run = (text: string) => runCommand(parseCommandLine(text, commandViews(useAppStore.getState()))!, useAppStore.getState());

  it("is a command of the menu with its own handler", () => {
    expect(HANDLERS.ask).toBeTypeOf("function");
  });

  it("lists the members the person may speak to, by name with the role beside it", () => {
    put(["api", "ui"]);
    expect(argChoices("members", "", useAppStore.getState()).map((c) => [c.value, c.hint])).toEqual([
      ["Kai", "api"],
      ["Juno", "ui"],
    ]);
    expect(argChoices("members", "ju", useAppStore.getState()).map((c) => c.value)).toEqual(["Juno"]);
  });

  it("with a message speaks to the member with that message alone; without, it picks the member", async () => {
    put(["api", "ui"]);
    const sent = await run("/ask kai is the copy fine?");
    expect(sent).toEqual({ kind: "modifier", send: "is the copy fine?" });
    expect(useAppStore.getState().turnModifiers.addressed).toBe("api");

    put(["api", "ui"]);
    const picked = await run("/ask juno");
    expect(picked).toMatchObject({ kind: "done", note: "your next message goes to Juno" });
    expect(useAppStore.getState().turnModifiers.addressed).toBe("ui");
  });

  it("says why when there is no one to ask", async () => {
    put([]);
    expect(await run("/ask kai hello")).toMatchObject({ kind: "error" });
    put(["api"]);
    expect(await run("/ask zed hello")).toMatchObject({ kind: "error", message: expect.stringContaining('"zed"') });
    act(() => useAppStore.setState({ conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], leader: "poiesis", personaId: null }] } as never));
    expect(await run("/ask kai hello")).toMatchObject({ kind: "error", message: expect.stringContaining("No team leads this chat") });
  });
});
