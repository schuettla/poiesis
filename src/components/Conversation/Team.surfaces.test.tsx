/**
 * @vitest-environment jsdom
 *
 * `TEAM-14`: where a team shows outside the Coordination view. The Fleet card says "api · Atlas",
 * the organization strip shows the members at work as their faces, and the header mark has a bud
 * for each member that is working. Chats no team leads look as they always did.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PlanView, Team } from "../../lib/api";
import type { SubRun } from "../../lib/types";

const teamGet = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => false,
  teamGet: (...a: unknown[]) => teamGet(...a),
}));

import FleetCard from "./FleetCard";
import OrgStrip from "./OrgStrip";
import PoiesisMark from "../Mark/PoiesisMark";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const team = {
  slug: "shop",
  coordinator: "poiesis",
  members: [
    { role: "api", persona: "persona:p1", job: "", owns: [] },
    { role: "docs", persona: "helper", job: "", owns: [] },
  ],
} as unknown as Team;

const sub = (id: string, member: string | undefined, over: Partial<SubRun> = {}): SubRun => ({
  runId: id,
  conversationId: `child-${id}`,
  parentConversationId: "c1",
  agent: member ?? "general",
  ...(member ? { member } : {}),
  task: "do it",
  status: "running",
  steps: [],
  text: "",
  startedAt: 1,
  ...over,
});
const runsOf = (...rs: SubRun[]) => Object.fromEntries(rs.map((r) => [r.runId, r]));

let container: HTMLDivElement;
let root: Root;
const render = async (el: React.ReactElement) => {
  await act(async () => {
    root.render(el);
  });
};
const chat = (leader: string) => ({ id: "c1", title: "t", updatedAt: 0, messages: [], leader, personaId: null });

beforeEach(() => {
  teamGet.mockReset().mockResolvedValue({ team, problems: [] });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    activeConversationId: "c1",
    conversations: [chat("team:shop")],
    personas: [{ id: "p1", name: "Atlas", look: null }],
    presets: [{ slug: "lean", name: "Lean", line: "x", glyph: "lean", built_in: true, locked: false, version: 1, rules: [] }],
    subRuns: {},
    runSummaries: {},
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState({ subRuns: {} });
});

describe("the Fleet card", () => {
  it("says role and persona for a member, the role alone for a helper in the team, and keeps its engine chips", async () => {
    useAppStore.setState({
      subRuns: runsOf(
        sub("a", "api", { engine: { id: "claude-code", name: "Claude Code", paid_by: "plan", because: "U1", because_text: null, can_steer: false } }),
        sub("d", "docs"),
        sub("g", undefined, { agent: "general" })
      ),
    });
    await render(<FleetCard runIds={["a", "d", "g"]} />);
    expect([...container.querySelectorAll(".fleet-agent")].map((e) => e.textContent)).toEqual(["api · Atlas", "docs", "general"]);
    expect(container.querySelector(".fleet-row")!.querySelector(".engine-chip, .fleet-who [class*=chip]")).not.toBeNull();
  });

  it("is the agent type when no team leads the chat, and reads no team file", async () => {
    useAppStore.setState({ conversations: [chat("poiesis")], subRuns: runsOf(sub("g", undefined, { agent: "Researcher" })) });
    await render(<FleetCard runIds={["g"]} />);
    expect(container.querySelector(".fleet-agent")!.textContent).toBe("Researcher");
    expect(teamGet).not.toHaveBeenCalled();
  });
});

describe("the organization strip", () => {
  const plan: PlanView = { style: "lean", revisions: 0, items: [{ text: "Export", status: "doing", by: "api" }] };

  it("shows each member at work as its face instead of a helper mark", async () => {
    useAppStore.setState({ subRuns: runsOf(sub("a", "api"), sub("d", "docs"), sub("w", "api", { waiting: true })) });
    await render(<OrgStrip plan={plan} live />);
    const faces = [...container.querySelectorAll(".org-member")];
    expect(faces.map((f) => f.getAttribute("title"))).toEqual(["api is working", "docs is working"]);
    expect(faces[0].querySelector(".persona-glyph")?.textContent).toBe("A");
    expect(faces[1].querySelector(".member-glyph-helper")).not.toBeNull();
    expect(container.querySelector(".org-who")!.textContent).not.toContain("helper");
  });

  it("keeps the helper mark for a helper that is no member, and reads no team for a chat no team leads", async () => {
    useAppStore.setState({ conversations: [chat("poiesis")], subRuns: runsOf(sub("g", undefined)) });
    await render(<OrgStrip plan={plan} live />);
    expect(container.querySelector(".org-members")).toBeNull();
    expect(container.querySelector(".org-who")!.textContent).toBe("◌ helper");
    expect(teamGet).not.toHaveBeenCalled();
  });
});

describe("the header mark", () => {
  const buds = () => container.querySelectorAll(".mark-bud").length;
  const settle = async () => {
    useAppStore.setState({ runSummaries: { c1: { runId: "r", startedAt: 0, live: true, files: [], costUsd: null, localRun: false, style: { preset: "lean", name: "Lean" } } } } as never);
    await render(<PoiesisMark />);
  };

  it("has a bud for each member that is working, and none for one still in the queue or one that is done", async () => {
    useAppStore.setState({
      subRuns: runsOf(sub("a", "api"), sub("d", "docs"), sub("w", "api", { waiting: true }), sub("x", "docs", { status: "done" })),
    });
    await settle();
    expect(buds()).toBe(2);
  });
});
