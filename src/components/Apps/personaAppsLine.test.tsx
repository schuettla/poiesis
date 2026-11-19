/**
 * @vitest-environment jsdom
 *
 * `CA-UI-6`: a persona narrowed to some apps says which, in the tooltip of who leads
 * the chat (in the composer's this-chat menu) and of its row on the Fleet card.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PersonaAppGrant } from "../../lib/api";

const getPersonaAppGrants = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  getPersonaAppGrants: (...a: unknown[]) => getPersonaAppGrants(...a),
}));

import { appsLine, forgetPersonaApps } from "./personaAppsLine";
import ChatSetup from "../Composer/ChatSetup";
import FleetCard from "../Conversation/FleetCard";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const grant = (app: string): PersonaAppGrant => ({ connector_id: "comp", app, tools: app ? "*" : "[]" });
const flush = () => act(async () => void (await new Promise((r) => setTimeout(r, 0))));

let container: HTMLDivElement;
let root: Root;
const before = useAppStore.getState();

beforeEach(() => {
  forgetPersonaApps();
  getPersonaAppGrants.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    personas: [{ id: "p1", name: "Researcher" }] as never,
    conversations: [{ id: "c1", title: "t", personaId: "p1" }] as never,
    activeConversationId: "c1",
    appAccounts: [
      { id: "connect:notion", app: "notion", app_name: "Notion", label: null, status: "ACTIVE" },
      { id: "connect:linear", app: "linear", app_name: "Linear", label: null, status: "ACTIVE" },
    ],
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState(before, true);
});

describe("the apps a persona may use", () => {
  it("reads as one line, and only when the persona is narrowed", () => {
    const name = (s: string) => ({ notion: "Notion", linear: "Linear" })[s] ?? s;
    expect(appsLine([], name)).toBeNull();
    expect(appsLine([grant("notion"), grant("linear")], name)).toBe("Apps: Notion, Linear");
    expect(appsLine([grant("")], name)).toBe("Apps: none");
  });

  /** `TEAM-2`: the persona's chip is the "Who leads" row of the this-chat menu now. */
  const whoLeads = async () => {
    act(() => root.render(<ChatSetup />));
    await flush();
    act(() => container.querySelector<HTMLButtonElement>(".chat-setup-btn")!.click());
    return container.querySelector<HTMLButtonElement>(".chat-setup-menu [role=menuitem]")!;
  };

  it("is said in the tooltip of who leads", async () => {
    getPersonaAppGrants.mockResolvedValue([grant("notion"), grant("linear")]);
    const row = await whoLeads();
    expect(row.textContent).toContain("Researcher");
    expect(row.title).toBe("Your choice for this chat · Apps: Notion, Linear");
    expect(getPersonaAppGrants).toHaveBeenCalledWith("p1");
  });

  it("says nothing extra for a persona that may use every app, and reads again after a save", async () => {
    getPersonaAppGrants.mockResolvedValue([]);
    expect((await whoLeads()).title).toBe("Your choice for this chat");

    getPersonaAppGrants.mockResolvedValue([grant("linear")]);
    act(() => forgetPersonaApps("p1"));
    await flush();
    expect(container.querySelector<HTMLButtonElement>(".chat-setup-menu [role=menuitem]")!.title).toBe(
      "Your choice for this chat · Apps: Linear"
    );
  });

  it("is said on the Fleet row of an agent backed by that persona", async () => {
    getPersonaAppGrants.mockResolvedValue([grant("notion")]);
    useAppStore.setState({
      subRuns: {
        r1: {
          runId: "r1",
          conversationId: "k1",
          parentConversationId: "c1",
          agent: "Researcher",
          task: "Find the spec",
          status: "done",
          steps: [],
          text: "",
          startedAt: 1,
        },
      } as never,
    });
    act(() => root.render(<FleetCard runIds={["r1"]} />));
    await flush();
    expect(container.querySelector<HTMLElement>(".fleet-agent")!.title).toBe("Find the spec\nApps: Notion");
  });
});
