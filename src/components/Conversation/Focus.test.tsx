/**
 * @vitest-environment jsdom
 *
 * `/focus`: the stream shows the messages and nothing around them. It is a switch
 * in the `/` menu; the same handler answers the typed command and the click.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import AgentRun from "./AgentRun";
import UserTurn from "./UserTurn";
import { applyControl, controlFor, parseCommandLine, runCommand, commandViews } from "../../lib/commands";
import { useAppStore } from "../../lib/store";
import type { Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

function message(overrides: Partial<Message> = {}): Message {
  return { id: "m1", role: "assistant", text: "", createdAt: 1, ...overrides } as Message;
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ focusMode: false });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

function render(node: React.ReactNode) {
  act(() => {
    root.render(node);
  });
}

describe("/focus as a switch", () => {
  it("is a live switch that follows the store", () => {
    expect(controlFor("focus", useAppStore.getState())).toEqual({ kind: "switch", on: false });
    useAppStore.getState().setFocusMode(true);
    expect(controlFor("focus", useAppStore.getState())).toEqual({ kind: "switch", on: true });
  });

  it("the menu's click and the typed command do the same thing", async () => {
    await applyControl("focus", "on", useAppStore.getState());
    expect(useAppStore.getState().focusMode).toBe(true);
    await applyControl("focus", "off", useAppStore.getState());
    expect(useAppStore.getState().focusMode).toBe(false);
  });

  it("with no argument it flips, and a bad one is said", async () => {
    const views = commandViews(useAppStore.getState());
    const flip = parseCommandLine("/focus", views)!;
    await runCommand(flip, useAppStore.getState());
    expect(useAppStore.getState().focusMode).toBe(true);
    await runCommand(flip, useAppStore.getState());
    expect(useAppStore.getState().focusMode).toBe(false);
    const bad = await runCommand(parseCommandLine("/focus maybe", views)!, useAppStore.getState());
    expect(bad.kind).toBe("error");
  });
});

describe("the stream in focus", () => {
  const answer = message({
    streaming: false,
    text: "Here you go.",
    model: { name: "m", provenance: "local" } as Message["model"],
    steps: [{ id: "s1", verb: "Read", target: "a.txt", status: "done" }] as Message["steps"],
  });

  it("shows an answer's words and none of what surrounds them", () => {
    render(<AgentRun message={answer} focus />);
    expect(container.textContent).toContain("Here you go.");
    expect(container.querySelector(".timeline, .run-header, .turn-foot, .turn-actions")).toBeNull();
  });

  it("shows the whole turn when it is off", () => {
    render(<AgentRun message={answer} />);
    expect(container.querySelector(".turn-foot")).not.toBeNull();
  });

  it("a working turn with nothing to read yet shows one sign of life", () => {
    render(<AgentRun message={message({ streaming: true, text: "" })} focus />);
    expect(container.querySelector(".thinking")).not.toBeNull();
    expect(container.querySelector(".run-meter-row")).toBeNull();
  });

  it("a finished turn with nothing to say leaves no box behind", () => {
    render(<AgentRun message={message({ streaming: false, text: "", steps: answer.steps })} focus />);
    expect(container.querySelector(".agent-run")).toBeNull();
  });

  it("your message keeps its words and loses its stamp", () => {
    const mine = message({ id: "u1", role: "user", text: "Hello there" });
    render(<UserTurn message={mine} focus />);
    expect(container.textContent).toContain("Hello there");
    expect(container.querySelector(".turn-stamp")).toBeNull();
  });

  it("a skill command shows the words it stood for", () => {
    const mine = message({
      id: "u2",
      role: "user",
      text: "Run /review",
      command: { name: "review", args: "" },
    });
    render(<UserTurn message={mine} focus />);
    expect(container.querySelector(".command-chip")).toBeNull();
    expect(container.textContent).toContain("Run /review");
  });
});
