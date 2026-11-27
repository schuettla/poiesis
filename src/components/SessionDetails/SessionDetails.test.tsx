/**
 * @vitest-environment jsdom
 *
 * `DBG-3`: the session details dialog, opened from a chat's menu.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sessionReport = vi.fn();
const sessionEvent = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  sessionReport: (...a: unknown[]) => sessionReport(...a),
  sessionEvent: (...a: unknown[]) => sessionEvent(...a),
}));

import type { SessionReport } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import SessionMenu from "../Conversation/SessionMenu";
import SessionDetails from "./SessionDetails";
import { summaryText } from "./summary";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const report: SessionReport = {
  generated_at: 1,
  schema_version: 55,
  app_version: "0.4.0",
  os: "Windows 11",
  data_dir: "C:/data",
  conversation: {
    id: "c1",
    title: "A chat",
    model_id: "m1",
    persona_id: null,
    overrides_json: null,
    workspace: false,
    created_at: 1_700_000_000_000,
    updated_at: 1_700_000_000_000,
    summary: null,
    summary_upto_message_id: null,
    reflected_at: null,
    folder_path: null,
    folder_trust: "confirm",
    parent_conversation_id: null,
  },
  engine_session: null,
  project: null,
  folder: null,
  folder_trust: "confirm",
  totals: {
    user_messages: 1,
    assistant_messages: 1,
    runs: 1,
    events: 1,
    tool_calls: 1,
    tool_errors: 1,
    prompt_tokens: 100,
    output_tokens: 20,
    cost_usd: null,
    helper_tokens: 0,
    artifacts: 0,
    files_changed: 0,
    compactions: 0,
  },
  runs: [
    {
      run_id: "run-1",
      started_at: 1_700_000_000_000,
      ended_at: 1_700_000_002_000,
      stop_reason: "error",
      live: false,
      steps: 2,
      model: "m1",
      provenance: "cloud",
      cost_usd: null,
      usage: [{ prompt_tokens: 100, output_tokens: 20, turns: 2, paid_by: null }],
      setup: { tier: "full", effort: "low", max_steps: 12, context_window: 8000, tools: ["read_file"], toolsets: ["filesystem"] },
      prompt_seq: 1,
      prompt_messages: 2,
      prompt_chars: 300,
      tool_calls: [{ id: "a", name: "read_file", args: "{}", ok: false, result: "Error: no", call_seq: 3, result_seq: 4 }],
      steers: [],
    },
  ],
  messages: [],
  events: [{ seq: 1, at: 1_700_000_000_000, kind: "prompt", run_id: "run-1", bytes: 300, preview: "2 messages sent to the model" }],
  helpers: [],
  posts: [],
  decisions: [],
  tool_counts: [],
  tool_fixes: [],
  skills: [],
  activity: [],
};

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  sessionReport.mockReset().mockResolvedValue(report);
  sessionEvent.mockReset().mockResolvedValue('{ "role": "system" }');
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    conversations: [{ id: "c1", title: "A chat", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    projects: [],
    engines: [],
    sessionDetailsId: null,
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const button = (text: string) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent?.startsWith(text))!;
const click = (el: HTMLElement) =>
  act(async () => {
    el.click();
  });

describe("session details", () => {
  it("opens from the chat's menu and shows the session id", async () => {
    await act(async () =>
      root.render(
        <>
          <SessionMenu />
          <SessionDetails />
        </>
      )
    );
    expect(container.querySelector(".sd-dialog")).toBeNull();
    await click(container.querySelector<HTMLButtonElement>(".session-more")!);
    await click(button("Session details…"));
    expect(sessionReport).toHaveBeenCalledWith("c1");
    expect(container.querySelector(".sd-dialog")!.textContent).toContain("Session idc1");
  });

  it("shows how a run was set up and reads a log row only when asked", async () => {
    useAppStore.setState({ sessionDetailsId: "c1" });
    await act(async () => root.render(<SessionDetails />));
    await click(button("Runs"));
    await click(container.querySelector<HTMLButtonElement>(".sd-fold-line")!);
    const body = container.querySelector(".sd-body")!.textContent;
    expect(body).toContain("run-1");
    expect(body).toContain("Tools offered at startread_file");
    expect(body).toContain("failed");
    expect(sessionEvent).not.toHaveBeenCalled();
    await click(button("What the model was sent"));
    expect(sessionEvent).toHaveBeenCalledWith("c1", 1);
    expect(container.querySelector(".sd-body pre")!.textContent).toContain("system");
  });

  it("closes on Escape", async () => {
    useAppStore.setState({ sessionDetailsId: "c1" });
    await act(async () => root.render(<SessionDetails />));
    act(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(container.querySelector(".sd-dialog")).toBeNull();
  });

  it("closes with the X in the corner", async () => {
    useAppStore.setState({ sessionDetailsId: "c1" });
    await act(async () => root.render(<SessionDetails />));
    await click(container.querySelector<HTMLButtonElement>(".sd-x")!);
    expect(container.querySelector(".sd-dialog")).toBeNull();
  });

  it("writes a summary with ids and failures and no message text", () => {
    const text = summaryText(report);
    expect(text).toContain("Session c1");
    expect(text).toContain("Run run-1");
    expect(text).toContain("failed read_file (row 4)");
    expect(text).not.toContain("Error: no");
  });
});
