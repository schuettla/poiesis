/**
 * @vitest-environment jsdom
 *
 * `USE-UI-2`: a message sent to a running agent shows the class a quick decision
 * read it as. A sure stop has not been sent, and says so, with two answers under it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import UserTurn from "./UserTurn";
import { useAppStore } from "../../lib/store";
import type { Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const answerSteerStop = vi.fn().mockResolvedValue(undefined);

beforeEach(() => {
  answerSteerStop.mockClear();
  useAppStore.setState({ answerSteerStop });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const turn = (over: Partial<Message>): Message => ({
  id: "u1",
  role: "user",
  text: "also add a README",
  createdAt: 1,
  ...over,
});
const show = (m: Message) => act(() => root.render(<UserTurn message={m} />));
const tag = () => container.querySelector(".steer-tag");
const buttons = () => Array.from(container.querySelectorAll<HTMLButtonElement>(".steer-stop button"));

describe("a message sent mid-run", () => {
  it("shows the class as a quiet tag, in words a person uses", () => {
    show(turn({ midRun: "pending", steer: { kind: "add", p: 0.82, decision: "d1" } }));
    expect(tag()?.textContent).toBe("addition");
    expect(tag()?.getAttribute("title")).toBe("Read as an addition (0.82)");
    expect(container.querySelector(".steer-stop")).toBeNull();
  });

  it("names each class", () => {
    const words = (kind: "stop" | "redirect" | "add" | "question") => {
      show(turn({ midRun: "delivered", steer: { kind, p: 0.7, decision: "d" } }));
      return tag()?.textContent;
    };
    expect(["stop", "redirect", "add", "question"].map((k) => words(k as "stop"))).toEqual([
      "stop",
      "redirect",
      "addition",
      "question",
    ]);
  });

  it("has no tag and no extra row when nothing was read", () => {
    show(turn({ midRun: "pending" }));
    expect(tag()).toBeNull();
    expect(container.querySelector(".mid-run-mark")?.textContent).toBe("sending…");
    show(turn({}));
    expect(container.querySelector(".role")).toBeNull();
  });
});

describe("a held stop", () => {
  const held = turn({ text: "no wait, forget it", midRun: "held", steer: { kind: "stop", p: 0.95, decision: "d2" } });

  it("says it was not sent and asks, with both answers", () => {
    show(held);
    expect(container.querySelector(".mid-run-mark")?.textContent).toBe("not sent yet");
    expect(container.querySelector(".steer-stop")?.getAttribute("aria-label")).toBe("Stop the run?");
    expect(container.querySelector(".steer-stop-q")?.textContent).toBe("Stop the run?");
    expect(buttons().map((b) => b.textContent)).toEqual(["Stop", "Keep going"]);
  });

  it("sends each answer to the store with this message", () => {
    show(held);
    act(() => buttons()[0].click());
    expect(answerSteerStop).toHaveBeenLastCalledWith("u1", true);
    act(() => buttons()[1].click());
    expect(answerSteerStop).toHaveBeenLastCalledWith("u1", false);
  });

  it("asks nothing once it is answered or the run is over", () => {
    show({ ...held, midRun: "pending" });
    expect(container.querySelector(".steer-stop")).toBeNull();
    show({ ...held, midRun: undefined });
    expect(container.querySelector(".steer-stop")).toBeNull();
  });
});
