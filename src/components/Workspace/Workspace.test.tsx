/**
 * @vitest-environment jsdom
 *
 * Workspace mode and the Workspace view are two things, as voice and its surface
 * are: the mode is the chat's own flag, the view is only which of the two
 * screens is up. Whichever is up, the other is one click away; and what a paused
 * run needs from you is never hidden behind the screen you are not on.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Workspace from "../../routes/Workspace";
import WorkspaceSwitch from "./WorkspaceSwitch";
import WorkspaceOffer from "./WorkspaceOffer";
import ModeChips from "../Composer/ModeChips";
import { useAppStore } from "../../lib/store";
import type { BlockView, Message } from "../../lib/types";
import BlockRenderer from "../Blocks/BlockRenderer";
import SurfaceRenderer from "../Surface/SurfaceRenderer";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

const user = (text: string): Message => ({ id: "u1", role: "user", text, createdAt: 1 });
const agent = (over: Partial<Message> = {}): Message => ({
  id: "a1",
  role: "assistant",
  text: "",
  createdAt: 2,
  ...over,
});

let container: HTMLDivElement;
let root: Root;

function reset(messages: Message[] = []) {
  useAppStore.setState({
    view: "chat",
    conversations: [{ id: "c1", title: "Trip", updatedAt: 0, messages, workspace: true }],
    activeConversationId: "c1",
    activeItemId: null,
    workspaceMode: true,
    workspaceShown: true,
    surfaceUnseen: {},
    surfaces: {},
    pendingActions: {},
    pendingQuestion: null,
    harnessProposals: [],
    busy: false,
    toolsEnabled: true,
  });
}

function mount(node: React.ReactNode) {
  act(() => root.render(node));
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  reset();
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const state = () => useAppStore.getState();
const button = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.trim() === label) as
    | HTMLButtonElement
    | undefined;

describe("mode and view", () => {
  it("turning the mode on opens the canvas, and on its own makes sure tools are on", () => {
    useAppStore.setState({ workspaceMode: false, workspaceShown: false, toolsEnabled: false });
    act(() => state().setWorkspaceMode(true));
    expect(state().workspaceMode).toBe(true);
    expect(state().workspaceShown).toBe(true);
    expect(state().toolsEnabled).toBe(true);
  });

  it("showing the stream keeps the mode on, and the flag on the chat", () => {
    act(() => state().setWorkspaceShown(false));
    expect(state().workspaceMode).toBe(true);
    expect(state().workspaceShown).toBe(false);
    expect(state().conversations[0].workspace).toBe(true);
  });

  it("has no canvas to show or hide while the mode is off", () => {
    useAppStore.setState({ workspaceMode: false, workspaceShown: false });
    act(() => state().setWorkspaceShown(true));
    expect(state().workspaceShown).toBe(false);
  });

  it("opening a workspace chat opens its canvas, not the stream behind it", async () => {
    act(() => state().setWorkspaceShown(false));
    await act(async () => {
      await state().setActiveConversation("c1");
    });
    expect(state().workspaceShown).toBe(true);
  });
});

describe("the top bar switch", () => {
  it("is not there while the mode is off, or outside a chat", () => {
    useAppStore.setState({ workspaceMode: false });
    mount(<WorkspaceSwitch />);
    expect(container.querySelector(".topbar-ws")).toBeNull();
    useAppStore.setState({ workspaceMode: true, view: "library" });
    mount(<WorkspaceSwitch />);
    expect(container.querySelector(".topbar-ws")).toBeNull();
  });

  it("moves between the stream and the canvas without ending the mode", () => {
    mount(<WorkspaceSwitch />);
    act(() => button("Chat")!.click());
    expect(state().workspaceShown).toBe(false);
    expect(state().workspaceMode).toBe(true);
    expect(button("Chat")!.getAttribute("aria-pressed")).toBe("true");
    act(() => button("Workspace")!.click());
    expect(state().workspaceShown).toBe(true);
  });

  it("brings the conversation back over an open file", () => {
    useAppStore.setState({ activeItemId: "file:a.txt" });
    mount(<WorkspaceSwitch />);
    act(() => button("Chat")!.click());
    expect(state().activeItemId).toBeNull();
  });

  it("marks the canvas as changed while it was out of sight, until it is opened", () => {
    useAppStore.setState({ workspaceShown: false, surfaceUnseen: { c1: true } });
    mount(<WorkspaceSwitch />);
    expect(container.querySelector(".topbar-ws-dot")).not.toBeNull();
    act(() => button("Workspace")!.click());
    expect(state().surfaceUnseen.c1).toBeUndefined();
    expect(container.querySelector(".topbar-ws-dot")).toBeNull();
  });
});

describe("the mode chip", () => {
  it("switches the view with its label and leaves the mode with its x", () => {
    mount(<ModeChips />);
    act(() => container.querySelector<HTMLButtonElement>(".mode-chip-label")!.click());
    expect(state().workspaceShown).toBe(false);
    act(() => container.querySelector<HTMLButtonElement>(".mode-chip-label")!.click());
    expect(state().workspaceShown).toBe(true);
    act(() => container.querySelector<HTMLButtonElement>(".mode-chip-x")!.click());
    expect(state().workspaceMode).toBe(false);
  });
});

describe("the unseen interface offer", () => {
  it("is silent in a chat that is not in the mode, which has no interface to change", () => {
    useAppStore.setState({ workspaceMode: false, surfaceUnseen: { c1: true } });
    mount(<WorkspaceOffer />);
    expect(container.querySelector(".ws-offer")).toBeNull();
  });

  it("offers to open the canvas when the mode is on and the stream is up", () => {
    useAppStore.setState({ workspaceShown: false, surfaceUnseen: { c1: true } });
    mount(<WorkspaceOffer />);
    act(() => button("Open workspace")!.click());
    expect(state().workspaceShown).toBe(true);
  });

  it("can be dismissed, and is silent when nothing changed", () => {
    useAppStore.setState({ workspaceShown: false, surfaceUnseen: { c1: true } });
    mount(<WorkspaceOffer />);
    act(() => container.querySelector<HTMLButtonElement>(".ws-offer-x")!.click());
    expect(container.querySelector(".ws-offer")).toBeNull();
    expect(state().workspaceMode).toBe(true);
    expect(state().surfaceUnseen.c1).toBeUndefined();
  });
});

describe("the workspace view", () => {
  it("says which mode this is, and gets back to the stream or out of the mode", () => {
    mount(<Workspace />);
    expect(container.querySelector(".ws-mode")?.textContent).toContain("Workspace");
    act(() => button("Show chat")!.click());
    expect(state().workspaceShown).toBe(false);
    expect(state().workspaceMode).toBe(true);
    reset();
    mount(<Workspace />);
    act(() => button("Turn off")!.click());
    expect(state().workspaceMode).toBe(false);
  });

  it("shows the latest reply without opening the stream", () => {
    reset([user("plan it"), agent({ text: "I put the plan on the board." })]);
    mount(<Workspace />);
    expect(container.querySelector(".ws-reply")?.textContent).toContain("I put the plan on the board.");
  });

  it("shows the question a paused run is waiting on, and answering it is possible here", () => {
    reset([user("book it"), agent({ streaming: true })]);
    useAppStore.setState({
      busy: true,
      pendingQuestion: {
        runId: "r1",
        id: "q1",
        convId: "c1",
        messageId: "a1",
        question: "Which hotel?",
        options: [{ label: "The harbour" }, { label: "The old town" }],
        multi: false,
      },
    });
    mount(<Workspace />);
    expect(container.querySelector(".question-card")?.textContent).toContain("Which hotel?");
    expect(container.querySelectorAll(".question-card-option")).toHaveLength(2);
  });

  it("shows a plan that waits for a go-ahead", () => {
    reset([
      user("rebuild it"),
      agent({
        plan: {
          items: [{ text: "Move the files", status: "todo" }],
          revisions: 0,
          awaiting_approval: true,
        } as never,
      }),
    ]);
    mount(<Workspace />);
    expect(container.querySelector(".plan-card")).not.toBeNull();
    expect(container.textContent).toContain("Go ahead");
  });

  it("says what the agent is doing while it works", () => {
    reset([
      user("look it up"),
      agent({
        streaming: true,
        steps: [{ id: "s1", verb: "searched", target: "the web", status: "running" } as never],
      }),
    ]);
    useAppStore.setState({ busy: true });
    mount(<Workspace />);
    expect(container.querySelector(".run-meter")?.textContent).toContain("searched the web");
  });

  it("says plainly when nothing is composed yet", () => {
    mount(<Workspace />);
    expect(container.querySelector(".ws-empty")?.textContent).toContain("Nothing here yet.");
  });

  it("keeps the other changes a person made to share with the next message", () => {
    useAppStore.setState({ pendingActions: { c1: ["Checked step 2"] } });
    reset([user("go"), agent({ text: "ok" })]);
    useAppStore.setState({ pendingActions: { c1: ["Checked step 2"] } });
    mount(<Workspace />);
    expect(container.querySelector(".ws-pending")?.textContent).toContain("1 change will be shared");
  });
});

describe("the session menu", () => {
  it("turns the mode on and off for the chat", async () => {
    const { default: SessionMenu } = await import("../Conversation/SessionMenu");
    useAppStore.setState({ workspaceMode: false });
    mount(<SessionMenu />);
    act(() => container.querySelector<HTMLButtonElement>(".session-more")!.click());
    act(() => button("Turn on Workspace mode")!.click());
    expect(state().workspaceMode).toBe(true);
    act(() => container.querySelector<HTMLButtonElement>(".session-more")!.click());
    act(() => button("Turn off Workspace mode")!.click());
    expect(state().workspaceMode).toBe(false);
  });
});

describe("what the model hands over as options", () => {
  const ctx = { state: {}, disabled: false, onBind: () => {}, onAction: () => {} };

  it("a surface choice takes bare strings and objects alike", () => {
    mount(
      <SurfaceRenderer
        tree={{
          type: "choice",
          bind: "area",
          options: ["Soho", { value: "kx", title: "King's Cross", detail: "trains" }],
        }}
        ctx={ctx}
      />
    );
    const labels = Array.from(container.querySelectorAll(".sf-choice-label")).map((e) => e.textContent);
    expect(labels).toEqual(["Soho", "King's Cross"]);
    expect(container.querySelector(".sf-choice-detail")?.textContent).toBe("trains");
  });

  it("a surface table draws its columns and rows, and survives loose cells", () => {
    mount(
      <SurfaceRenderer
        tree={{ type: "table", columns: ["Hotel", "Price"], rows: [["Harbour", 120], ["Old town"], "odd"] }}
        ctx={ctx}
      />
    );
    expect(Array.from(container.querySelectorAll("th")).map((e) => e.textContent)).toEqual(["Hotel", "Price"]);
    expect(container.querySelectorAll("tbody tr")).toHaveLength(3);
    expect(container.querySelector("tbody td")?.textContent).toBe("Harbour");
  });

  it("a form block shows option labels, and a submitted form shows what was chosen", () => {
    const block = (submitted: boolean): BlockView => ({
      id: "b1",
      kind: "form",
      title: "Trip",
      data: {
        fields: [
          { id: "budget", label: "Budget", type: "select", options: [{ id: "mid", label: "Mid-range" }] },
          { id: "likes", label: "Interests", type: "multiselect", options: [{ id: "art", label: "Art" }] },
        ],
      },
      state: submitted ? { submitted: true, values: { budget: "mid", likes: [] } } : undefined,
    });
    mount(<BlockRenderer block={block(false)} />);
    expect(container.querySelector("option[value='mid']")?.textContent).toBe("Mid-range");
    expect(container.textContent).not.toContain("[object Object]");
    act(() => root.render(<BlockRenderer key="submitted" block={block(true)} />));
    const frozen = Array.from(container.querySelectorAll(".form-frozen")).map((e) => e.textContent);
    expect(frozen).toEqual(["Mid-range", "—"]);
  });
});

describe("blocks from the present tool in the workspace view", () => {
  it("stay visible next to a composed surface", () => {
    reset([
      user("go"),
      agent({
        text: "done",
        blocks: [{ id: "b1", kind: "plan", title: "Steps", data: { steps: [{ id: "s", label: "Pack" }] } }],
      }),
    ]);
    useAppStore.setState({
      surfaces: { c1: { id: "s1", kind: "surface", title: "Workspace", data: { type: "text", value: "Hello" } } },
    });
    mount(<Workspace />);
    expect(container.querySelector(".sf-surface")).not.toBeNull();
    expect(container.querySelector(".ws-block")?.textContent).toContain("Steps");
  });
});
