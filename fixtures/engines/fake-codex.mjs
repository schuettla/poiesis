// A stand-in for `codex app-server`, for tests (`CLI-T-2`).
//
// It speaks the same JSON-RPC on stdin/stdout as the real one (no "jsonrpc"
// field): `initialize`, `thread/start`, `thread/resume`, `turn/start`,
// `turn/steer`, `turn/interrupt`, `model/list`, and it asks
// `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`
// before a step, as the recordings show. Like Codex, a refused step does not end
// the turn: the model carries on and reports. What it does is chosen by a word in
// the task: `SCENARIO:<name>`. It is only a test double; the recordings in this
// folder are what the real one does.
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";

const send = (o) => process.stdout.write(JSON.stringify(o) + "\n");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let THREAD = "thr_fake1";
let TURN = "turn_1";
let threadCall = null; // the last thread/start or thread/resume, as it was sent
let model = "fake-model";
const steers = [];
let interrupted = false;
let interruptWaiters = [];
const untilInterrupt = () =>
  new Promise((r) => {
    if (interrupted) return r();
    interruptWaiters.push(r);
  });

const note = (method, params) => send({ method, params });
const itemStarted = (item) => note("item/started", { item, threadId: THREAD, turnId: TURN });
const itemCompleted = (item) => note("item/completed", { item, threadId: THREAD, turnId: TURN });

// ---- what we ask the client ----
let nextOut = 0;
const asked = new Map();
function ask(method, params) {
  const id = nextOut++;
  send({ id, method, params });
  return new Promise((res) => asked.set(id, res));
}

let n = 0;
async function useCommand(command, extra = {}) {
  const id = `exec-${++n}`;
  const cwd = process.cwd();
  const full = `"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" -Command '${command}'`;
  const base = { type: "commandExecution", id, command: full, cwd, source: "agent", commandActions: [{ type: "unknown", command }], aggregatedOutput: null, exitCode: null };
  itemStarted({ ...base, status: "inProgress" });
  const reply = await ask("item/commandExecution/requestApproval", {
    kind: "command",
    threadId: THREAD,
    turnId: TURN,
    itemId: id,
    command: full,
    cwd,
    commandActions: base.commandActions,
    availableDecisions: ["accept", "cancel"],
  });
  if (reply?.decision === "accept") {
    extra.run?.();
    itemCompleted({ ...base, status: "completed", aggregatedOutput: "ok", exitCode: 0 });
    return true;
  }
  itemCompleted({ ...base, status: "declined" });
  return false;
}

async function useEdit(file, text) {
  const id = `exec-${++n}`;
  const changes = [{ path: file, kind: { type: "add" }, diff: text }];
  itemStarted({ type: "fileChange", id, changes, status: "inProgress" });
  const reply = await ask("item/fileChange/requestApproval", { threadId: THREAD, turnId: TURN, itemId: id, reason: null, grantRoot: null });
  if (reply?.decision === "accept") {
    fs.writeFileSync(file, text);
    itemCompleted({ type: "fileChange", id, changes, status: "completed" });
    return true;
  }
  itemCompleted({ type: "fileChange", id, changes, status: "declined" });
  return false;
}

// A step that needs no question: Codex's own sandbox let it through.
function quietCommand(command) {
  const id = `exec-${++n}`;
  const base = { type: "commandExecution", id, command, cwd: process.cwd(), source: "agent", commandActions: [{ type: "unknown", command }] };
  itemStarted({ ...base, status: "inProgress", aggregatedOutput: null, exitCode: null });
  itemCompleted({ ...base, status: "completed", aggregatedOutput: "{}", exitCode: 0 });
}

// ---- the MCP server Poiesis mounts through thread/start `config` (`MCPS-2`) ----
let rpcId = 0;
async function rpc(url, method, params, notify = false) {
  const body = { jsonrpc: "2.0", method, params };
  if (!notify) body.id = rpcId++;
  const res = await fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  return notify ? null : res.json();
}

// One MCP tool call, the way the recordings show it: an `mcpToolCall` item, then
// `mcpServer/elicitation/request` asking leave, then the call itself.
async function useMcpTool(url, tool, args) {
  const id = `exec-${++n}`;
  const base = { type: "mcpToolCall", id, server: "poiesis", tool, arguments: args, appContext: null, mcpAppUi: null, pluginId: null, readOnlyHint: null };
  itemStarted({ ...base, status: "inProgress", result: null, error: null, durationMs: null });
  const reply = await ask("mcpServer/elicitation/request", {
    threadId: THREAD,
    turnId: TURN,
    serverName: "poiesis",
    mode: "form",
    _meta: { codex_approval_kind: "mcp_tool_call", persist: ["session", "always"], tool_params: args },
    message: `Allow the poiesis MCP server to run tool "${tool}"?`,
    requestedSchema: { type: "object", properties: {} },
  });
  if (reply?.action !== "accept") {
    itemCompleted({ ...base, status: "failed", result: null, error: { message: "user cancelled MCP tool call" }, durationMs: 1 });
    return `declined: ${reply?.action}`;
  }
  const r = await rpc(url, "tools/call", { name: tool, arguments: args });
  if (r.error) {
    itemCompleted({ ...base, status: "failed", result: null, error: { message: r.error.message }, durationMs: 1 });
    return `error: ${r.error.message}`;
  }
  const text = r.result.content[0].text;
  itemCompleted({ ...base, status: "completed", result: { content: r.result.content, structuredContent: null, _meta: null }, error: null, durationMs: 5 });
  return text;
}

function say(text, phase) {
  const id = `msg_${++n}`;
  const item = { type: "agentMessage", id, text: "", phase, memoryCitation: null, delivery: null, questions: null };
  itemStarted(item);
  note("item/agentMessage/delta", { threadId: THREAD, turnId: TURN, itemId: id, delta: text });
  itemCompleted({ ...item, text });
  return { ...item, text };
}

function end(finalText, status = "completed") {
  const items = finalText === null ? [] : [say(finalText, "final_answer")];
  note("thread/tokenUsage/updated", {
    threadId: THREAD,
    turnId: TURN,
    tokenUsage: { total: { totalTokens: 15, inputTokens: 10, cachedInputTokens: 0, outputTokens: 5, reasoningOutputTokens: 0 } },
  });
  note("turn/completed", { threadId: THREAD, turn: { id: TURN, status, items: status === "completed" ? items : [], error: null } });
}

async function turn(params) {
  interrupted = false;
  const texts = params.input.map((b) => b.text ?? "");
  const task = texts.join("\n");
  const scenario = /SCENARIO:([a-z-]+)/.exec(task)?.[1] ?? "plain";
  const cwd = process.cwd();

  switch (scenario) {
    case "command": {
      say("Starting.", "commentary");
      const ok = await useCommand("echo hi");
      return end(ok ? "ran the command" : "the command was refused");
    }
    case "same-command": {
      await useCommand("echo same");
      await useCommand("echo same");
      return end("ran it twice");
    }
    case "edit": {
      const ok = await useEdit(path.join(cwd, "note.txt"), "hello");
      return end(ok ? "wrote note.txt" : "the write was refused");
    }
    case "read": {
      quietCommand("cat package.json");
      return end("read it without asking");
    }
    case "report": {
      const env = process.env;
      return end(
        JSON.stringify({
          cwd,
          threadCall: threadCall?.method,
          threadParams: threadCall?.params,
          poiesisEnv: Object.keys(env).filter((k) => k.toUpperCase().startsWith("POIESIS_")),
          openaiKey: env.OPENAI_API_KEY ?? null,
          argv: process.argv.slice(2),
          items: params.input.length,
          firstItem: texts[0],
          fenceStart: texts[texts.length - 1].startsWith("<<<POIESIS_TASK\nSCENARIO:report"),
          fenceEnd: texts[texts.length - 1].endsWith("\nPOIESIS_TASK>>>"),
          fenceCloses: (texts[texts.length - 1].match(/POIESIS_TASK>>>/g) ?? []).length,
          sawWords: task.includes("the task words"),
          model,
        }),
      );
    }
    case "memory": {
      const server = threadCall?.params?.config?.mcp_servers?.poiesis;
      if (!server) return end(JSON.stringify({ mounted: false, config: threadCall?.params?.config ?? null }));
      await rpc(server.url, "initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "fake-codex" } });
      await rpc(server.url, "notifications/initialized", {}, true);
      const listed = (await rpc(server.url, "tools/list", {})).result.tools.map((t) => t.name);
      const results = {};
      const calls = [
        ["recall", { query: "pnpm" }],
        ["present", { kind: "progress", title: "Tests", current: 1, total: 4 }],
        ["ask_user", { question: "Which package manager?", options: ["pnpm", "npm"] }],
        ["remember", { text: "This project uses pnpm, never npm." }],
      ];
      for (const [name, args] of calls) results[name] = await useMcpTool(server.url, name, args);
      return end(JSON.stringify({ mounted: true, listed, results, toolTimeout: server.tool_timeout_sec, method: threadCall.method }));
    }
    case "novel": {
      // What a newer version might send that this one has never seen.
      note("something/from-the-future", { a: 1 });
      itemStarted({ type: "somethingNew", id: "x1" });
      send({ id: nextOut++, method: "item/future/request", params: {} });
      return end("still works");
    }
    case "steer": {
      say("working", "commentary");
      for (let i = 0; i < 60 && steers.length === 0 && !interrupted; i++) await sleep(50);
      return end(steers.length ? `steered with: ${steers.join(", ")}` : "no steer arrived");
    }
    case "hang": {
      itemStarted({ type: "commandExecution", id: `exec-${++n}`, command: "sleep forever", cwd, source: "agent", commandActions: [{ type: "unknown", command: "sleep forever" }], status: "inProgress" });
      await untilInterrupt();
      return end(null, "interrupted");
    }
    case "ask-forever": {
      await useCommand("echo never answered");
      return end(null, interrupted ? "interrupted" : "completed");
    }
    case "silent": {
      // Takes the task and says nothing more, like a wedged app-server.
      await sleep(600000);
      return;
    }
    case "grandchild": {
      // A child of its own, as an npm shim or a build tool would make. Stop must
      // take it away too. It ignores the interrupt on purpose.
      const g = spawn(process.execPath, ["-e", "setInterval(()=>{},1000)"], { stdio: "ignore" });
      fs.writeFileSync(path.join(cwd, "grandchild.pid"), String(g.pid));
      await sleep(600000);
      return;
    }
    case "fail": {
      process.stderr.write("fake-codex: something broke\nsecond line\n");
      process.exit(3);
      return;
    }
    default: {
      return end(scenario === "plain" && !task.includes("plain task") ? `follow-up got: ${task}` : "done");
    }
  }
}

// ---- stdin: one JSON-RPC message per line ----
let buf = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (d) => {
  buf += d;
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    if (line) handle(JSON.parse(line));
  }
});
process.stdin.on("end", () => process.exit(0));

const threadResult = (id, params) => {
  if (params.model) model = params.model;
  return { thread: { id, model, modelProvider: "fake", cwd: params.cwd }, approvalPolicy: params.approvalPolicy, sandbox: params.sandbox };
};

function handle(m) {
  // An answer to something we asked.
  if (m.method === undefined && m.id !== undefined) {
    asked.get(m.id)?.(m.result);
    asked.delete(m.id);
    return;
  }
  const p = m.params ?? {};
  switch (m.method) {
    case "initialized":
      return;
    case "initialize":
      return send({ id: m.id, result: { userAgent: "poiesis/9.9.9 (fake)", codexHome: "", platformFamily: "windows", platformOs: "windows" } });
    case "windowsSandbox/readiness":
      // A file named `sandbox-not-ready` in the folder plays a computer where
      // the sandbox was never set up.
      return send({ id: m.id, result: { status: fs.existsSync(path.join(process.cwd(), "sandbox-not-ready")) ? "notConfigured" : "ready" } });
    case "model/list":
      return send({
        id: m.id,
        result: {
          data: [
            { id: "fake-model", displayName: "Fake model", isDefault: true, supportedReasoningEfforts: [] },
            { id: "fake-other", displayName: "Fake other", isDefault: false, supportedReasoningEfforts: [] },
          ],
        },
      });
    case "thread/start": {
      if (fs.existsSync(path.join(p.cwd, "auth-fail"))) {
        return send({ id: m.id, error: { code: -32000, message: "Not logged in. Run codex login." } });
      }
      THREAD = "thr_fake1";
      threadCall = { method: m.method, params: p };
      send({ id: m.id, result: threadResult(THREAD, p) });
      return note("account/updated", { authMode: "chatgpt", planType: "free" });
    }
    case "thread/resume": {
      if (p.threadId === "thr_gone") {
        return send({ id: m.id, error: { code: -32600, message: `no rollout found for thread id ${p.threadId}` } });
      }
      THREAD = p.threadId;
      threadCall = { method: m.method, params: p };
      send({ id: m.id, result: threadResult(THREAD, p) });
      return note("account/updated", { authMode: "chatgpt", planType: "free" });
    }
    case "turn/start":
      send({ id: m.id, result: { turn: { id: TURN, status: "inProgress" } } });
      note("turn/started", { threadId: THREAD, turn: { id: TURN, status: "inProgress", items: [] } });
      turn(p);
      return;
    case "turn/steer":
      steers.push(p.input.map((b) => b.text).join(" "));
      return send({ id: m.id, result: { turnId: TURN } });
    case "turn/interrupt": {
      interrupted = true;
      send({ id: m.id, result: {} });
      for (const [id, res] of asked) {
        res({ decision: "cancel" });
        asked.delete(id);
      }
      for (const w of interruptWaiters.splice(0)) w();
      return;
    }
    default:
      return send({ id: m.id, error: { code: -32601, message: "method not found" } });
  }
}
