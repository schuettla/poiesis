/**
 * @vitest-environment jsdom
 *
 * `MCG-UI-1`: the permission panel learns "app". A connected app's change or
 * send shows what it would run with, and each kind gets its own words.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import PermissionPanel, { appChoices } from "./PermissionPanel";
import { useAppStore } from "../../lib/store";
import type { PermissionRequest } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState({ pendingPermissions: [] });
});

function request(over: Partial<PermissionRequest> = {}): PermissionRequest {
  return {
    id: "perm_1",
    summary: "I'd like to send something through Slack: post message → #general",
    path: "Slack",
    mode: "read",
    in_folder: false,
    capability: "app",
    call_kind: "send",
    cap: 25,
    diff: JSON.stringify({ channel: "#general", text: "Release notes for 1.4" }),
    ...over,
  };
}

function show(r: PermissionRequest) {
  useAppStore.setState({ pendingPermissions: [r] });
  act(() => root.render(<PermissionPanel />));
}

const buttons = () => Array.from(container.querySelectorAll<HTMLButtonElement>(".permission-btn")).map((b) => b.textContent);

describe("the permission panel for a connected app", () => {
  it("words each kind for what 'always' means for it", () => {
    expect(appChoices("send", "Slack", 25).map((c) => c.label)).toEqual(["Send once", "Up to 25 a day for Slack", "No"]);
    expect(appChoices("change", "Slack", undefined).map((c) => c.label)).toEqual([
      "Allow once",
      "Always for Slack changes",
      "No",
    ]);
    expect(appChoices("read", "Slack", undefined)[1].label).toBe("Always let me read Slack");
  });

  it("draws a send in the outbound colour and states the cap on the button", () => {
    show(request());
    expect(container.querySelector(".side-panel.outbound")).not.toBeNull();
    expect(buttons()).toEqual(["Send once", "Up to 25 a day for Slack", "No"]);
    expect(container.textContent).toContain("post message");
  });

  it("does not paint a change as a send", () => {
    show(request({ call_kind: "change", cap: undefined }));
    expect(container.querySelector(".side-panel.outbound")).toBeNull();
    expect(buttons()).toEqual(["Allow once", "Always for Slack changes", "No"]);
  });

  it("lists the arguments as key and value rows", () => {
    show(request());
    const keys = Array.from(container.querySelectorAll(".permission-arg dt")).map((e) => e.textContent);
    expect(keys).toEqual(["channel", "text"]);
    expect(container.querySelector(".permission-args")?.textContent).toContain("Release notes for 1.4");
  });

  it("folds a long value until asked for", () => {
    const long = "word ".repeat(80).trim();
    show(request({ diff: JSON.stringify({ body: long }) }));
    const dd = container.querySelector(".permission-arg dd")!;
    expect(dd.textContent).toContain("Show all");
    expect(dd.textContent).not.toContain(long);
    act(() => container.querySelector<HTMLButtonElement>(".permission-fold")!.click());
    expect(container.querySelector(".permission-arg dd")!.textContent).toContain(long);
  });

  it("falls back to the raw text when a long payload was cut mid-JSON", () => {
    show(request({ diff: '{ "text": "cut off…' }));
    expect(container.querySelector("pre.permission-diff")?.textContent).toContain("cut off");
  });

  it("answers the request and takes the panel away", () => {
    show(request());
    const send = Array.from(container.querySelectorAll<HTMLButtonElement>(".permission-btn")).find(
      (b) => b.textContent === "No",
    )!;
    act(() => send.click());
    expect(useAppStore.getState().pendingPermissions).toHaveLength(0);
  });
});
