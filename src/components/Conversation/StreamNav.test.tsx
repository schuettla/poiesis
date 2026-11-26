/**
 * @vitest-environment jsdom
 *
 * `UI-5`: the Latest button and the line with the question. jsdom has no layout, so the sizes
 * of the scroll box and the position of the question card are set by hand.
 */
import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AskedLine, LatestButton, useStreamNav } from "./StreamNav";
import type { Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const msg = (id: string, role: "user" | "assistant", text = id): Message => ({ id, role, text }) as Message;

let nav: ReturnType<typeof useStreamNav>;
function Harness({ messages }: { messages: Message[] }) {
  const ref = useRef<HTMLDivElement>(null);
  nav = useStreamNav(ref, messages, "c1");
  return (
    <div className="main" ref={ref} {...nav.handlers}>
      <AskedLine nav={nav} />
      <div className="turn-user" data-message-id="u2" />
      <LatestButton nav={nav} />
    </div>
  );
}

let container: HTMLDivElement;
let root: Root;
const scrollTo = vi.fn();

/** A scroll box 400px high with 3000px of content, scrolled to `top`, and the question card
 * `cardBottom` pixels below the box's top edge. */
function layout(top: number, cardBottom = 100) {
  const main = container.querySelector<HTMLElement>(".main")!;
  Object.defineProperty(main, "scrollHeight", { configurable: true, value: 3000 });
  Object.defineProperty(main, "clientHeight", { configurable: true, value: 400 });
  main.scrollTop = top;
  main.scrollTo = scrollTo as never;
  main.getBoundingClientRect = () => ({ top: 0, bottom: 400 }) as DOMRect;
  container.querySelector<HTMLElement>(".turn-user")!.getBoundingClientRect = () =>
    ({ top: cardBottom - 60, bottom: cardBottom }) as DOMRect;
  return main;
}
const scroll = (main: HTMLElement) => act(() => main.dispatchEvent(new Event("scroll", { bubbles: true })));
const messages = [msg("u1", "user"), msg("a1", "assistant"), msg("u2", "user", "What does the export do?"), msg("a2", "assistant")];
const render = (m: Message[]) => act(() => root.render(<Harness messages={m} />));
const latest = () => container.querySelector<HTMLButtonElement>(".stream-latest");
const asked = () => container.querySelector<HTMLButtonElement>(".stream-asked");

beforeEach(() => {
  scrollTo.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  render(messages);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the Latest button", () => {
  it("is not there at the bottom, nor within one screen of it", () => {
    scroll(layout(2600));
    expect(latest()).toBeNull();
    scroll(layout(2300));
    expect(latest()).toBeNull();
  });

  it("shows when the view is more than one screen above the bottom, and goes when it is not", () => {
    const main = layout(1000);
    scroll(main);
    expect(latest()?.textContent).toBe("Latest");
    scroll(layout(2500));
    expect(latest()).toBeNull();
  });

  it("counts the messages that came in since the person scrolled up", () => {
    scroll(layout(1000));
    expect(latest()?.textContent).toBe("Latest");
    render([...messages, msg("a3", "assistant"), msg("a4", "assistant")]);
    expect(latest()?.textContent).toBe("Latest · 2 new");
  });

  it("scrolls to the bottom when clicked, and the stream follows again", () => {
    scroll(layout(1000));
    act(() => latest()!.click());
    expect(scrollTo).toHaveBeenCalledWith(expect.objectContaining({ top: 3000 }));
    expect(latest()).toBeNull();
    expect(nav.follow.current).toBe(true);
  });
});

describe("following the end of the answer", () => {
  it("lets go only when the person scrolls up, and takes hold again at the bottom", () => {
    const main = layout(2600);
    scroll(main);
    expect(nav.follow.current).toBe(true);
    // A scroll the page started, far from the bottom, is not the person letting go.
    scroll(layout(1500));
    expect(nav.follow.current).toBe(true);
    act(() => main.dispatchEvent(new WheelEvent("wheel", { deltaY: -40, bubbles: true })));
    expect(nav.follow.current).toBe(false);
    scroll(layout(2600));
    expect(nav.follow.current).toBe(true);
  });

  it("lets go for a press on the scrollbar, and not for a press on the text", () => {
    const main = layout(2600);
    act(() => main.querySelector(".turn-user")!.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true })));
    expect(nav.follow.current).toBe(true);
    act(() => main.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true })));
    expect(nav.follow.current).toBe(false);
  });
});

describe("the question line", () => {
  it("shows the newest question while its card is above the top edge, and not while it is in view", () => {
    scroll(layout(1000, 100));
    expect(asked()).toBeNull();
    scroll(layout(1000, -20));
    expect(asked()?.textContent).toBe("You asked: What does the export do?");
    scroll(layout(1000, 120));
    expect(asked()).toBeNull();
  });

  it("scrolls to the card when clicked", () => {
    const into = vi.fn();
    scroll(layout(1000, -20));
    container.querySelector<HTMLElement>(".turn-user")!.scrollIntoView = into;
    act(() => asked()!.click());
    expect(into).toHaveBeenCalledWith(expect.objectContaining({ block: "start" }));
  });

  it("is not shown for a chat with no question", () => {
    render([msg("a1", "assistant")]);
    scroll(layout(1000, -20));
    expect(asked()).toBeNull();
  });
});
