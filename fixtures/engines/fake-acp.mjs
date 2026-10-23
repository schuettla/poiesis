// A stand-in for an ACP agent such as OpenCode, for tests (`CLI-T-2`).
//
// It speaks the same JSON-RPC on stdin/stdout as `opencode acp`: `initialize`,
// `session/new`, `session/load`, `session/set_config_option`, `session/prompt`,
// `session/cancel`, and it asks `session/request_permission` before a tool, as the
// recordings show. Like OpenCode, a refused step ends the whole turn with
// `stopReason: cancelled`. What it does is chosen by a word in the task:
// `SCENARIO:<name>`. It is only a test double; the recordings in this folder are
// what the real one does.
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";

const send = (o) => process.stdout.write(JSON.stringify({ jsonrpc: "2.0", ...o }) + "\n");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let SESSION = "ses_fake1";
let currentModel = "fake/model";
const models = [
  { value: "fake/model", name: "Fake model" },
  { value: "fake/other-free", name: "Fake free model" },
];
const configOptions = () => [
  { id: "model", name: "Model", category: "model", type: "select", currentValue: currentModel, options: models },
];

const update = (u) => send({ method: "session/update", params: { sessionId: SESSION, update: u } });
const chunk = (text) => update({ sessionUpdate: "agent_message_chunk", content: { type: "text", text } });

// ---- what we ask the client, and what a cancel does to it ----
let nextOut = 100;
const asked = new Map();
let cancelled = false;
let cancelWaiters = [];
const untilCancel = () =>
  new Promise((r) => {
    if (cancelled) return r();
    cancelWaiters.push(r);
  });

function ask(toolCall) {
  const id = nextOut++;
  send({
    id,
    method: "session/request_permission",
    params: {
      sessionId: SESSION,
      toolCall,
      options: [
        { optionId: "once", kind: "allow_once", name: "Allow once" },
        { optionId: "always", kind: "allow_always", name: "Always allow" },
        { optionId: "reject", kind: "reject_once", name: "Reject" },
      ],
    },
  });
  return new Promise((res) => asked.set(id, res));
}

let n = 0;
async function useTool(kind, title, rawInput, extra = {}) {
  const callId = `call_${++n}`;
  const name = kind === "edit" ? "write" : "shell";
  update({ sessionUpdate: "tool_call", toolCallId: callId, name, title: name, kind, status: "pending", locations: [], rawInput: { cwd: process.cwd() } });
  update({ sessionUpdate: "tool_call_update", toolCallId: callId, status: "in_progress", kind, title, locations: extra.locations ?? [], rawInput });
  const reply = await ask({
    toolCallId: callId,
    name,
    title,
    kind,
    status: "pending",
    locations: extra.locations ?? [],
    rawInput,
    ...(extra.content ? { content: extra.content } : {}),
  });
  const picked = reply?.outcome?.outcome === "selected" ? reply.outcome.optionId : null;
  if (picked === "once") {
    extra.run?.();
    update({ sessionUpdate: "tool_call_update", toolCallId: callId, status: "completed", content: [{ type: "content", content: { type: "text", text: "ok" } }] });
    return true;
  }
  update({
    sessionUpdate: "tool_call_update",
    toolCallId: callId,
    status: "failed",
    content: [{ type: "content", content: { type: "text", text: "The user declined this tool call" } }],
  });
  return false;
}

// ---- the MCP server Poiesis mounts in `session/new` (`MCPS-2`) ----
let sessionNew = null;
let rpcId = 0;
async function rpc(url, method, params, notify = false) {
  const body = { jsonrpc: "2.0", method, params };
  if (!notify) body.id = rpcId++;
  const res = await fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  return notify ? null : res.json();
}

// A call to one of the mounted tools, shown the way OpenCode 2.1.0 shows it: as its
// own `execute` tool whose code reads `tools.poiesis.<name>(...)`. No permission
// request, as recorded.
async function useMcpTool(url, tool, args) {
  const callId = `call_${++n}`;
  const code = `const r = await tools.poiesis.${tool}(${JSON.stringify(args)});\nreturn r;`;
  update({ sessionUpdate: "tool_call", toolCallId: callId, name: "execute", title: "execute", kind: "other", status: "pending", locations: [], rawInput: {} });
  update({ sessionUpdate: "tool_call_update", toolCallId: callId, status: "in_progress", kind: "other", title: "execute", locations: [], rawInput: { code } });
  const r = await rpc(url, "tools/call", { name: tool, arguments: args });
  const text = r.error ? `error: ${r.error.message}` : r.result.content[0].text;
  update({ sessionUpdate: "tool_call_update", toolCallId: callId, status: "completed", locations: [], content: [{ type: "content", content: { type: "text", text } }] });
  return text;
}

async function prompt(id, params) {
  cancelled = false;
  const texts = params.prompt.map((b) => b.text ?? "");
  const task = texts.join("\n");
  const scenario = /SCENARIO:([a-z-]+)/.exec(task)?.[1] ?? "plain";
  const cwd = process.cwd();
  const end = (stopReason = "end_turn") =>
    send({ id, result: { stopReason, usage: { inputTokens: 10, outputTokens: 5, totalTokens: 15, cachedReadTokens: 0 } } });

  switch (scenario) {
    case "command": {
      chunk("Starting.");
      const ok = await useTool("execute", "echo hi", { command: "echo hi", cwd });
      if (!ok) return end("cancelled");
      chunk("ran the command");
      return end();
    }
    case "same-command": {
      if (!(await useTool("execute", "echo same", { command: "echo same", cwd }))) return end("cancelled");
      if (!(await useTool("execute", "echo same", { command: "echo same", cwd }))) return end("cancelled");
      chunk("ran it twice");
      return end();
    }
    case "edit": {
      const file = path.join(cwd, "note.txt");
      const ok = await useTool("edit", "note.txt", { path: "note.txt", content: "hello" }, {
        locations: [{ path: file }],
        content: [{ type: "diff", path: file, oldText: null, newText: "hello" }],
        run: () => fs.writeFileSync(file, "hello"),
      });
      if (!ok) return end("cancelled");
      chunk("wrote note.txt");
      return end();
    }
    case "read": {
      const callId = `call_${++n}`;
      update({ sessionUpdate: "tool_call", toolCallId: callId, name: "read", title: "read", kind: "read", status: "pending", locations: [], rawInput: {} });
      update({ sessionUpdate: "tool_call_update", toolCallId: callId, status: "in_progress", kind: "read", title: "package.json", locations: [{ path: path.join(cwd, "package.json") }], rawInput: { path: "package.json" } });
      update({ sessionUpdate: "tool_call_update", toolCallId: callId, status: "completed", content: [{ type: "content", content: { type: "text", text: "{}" } }] });
      chunk("read it without asking");
      return end();
    }
    case "report": {
      const env = process.env;
      chunk(
        JSON.stringify({
          cwd,
          permissionConfig: env.OPENCODE_CONFIG_CONTENT ?? null,
          poiesisEnv: Object.keys(env).filter((k) => k.toUpperCase().startsWith("POIESIS_")),
          openaiKey: env.OPENAI_API_KEY ?? null,
          argv: process.argv.slice(2),
          blocks: params.prompt.length,
          firstBlock: texts[0],
          fenceStart: texts[texts.length - 1].startsWith("<<<POIESIS_TASK\nSCENARIO:report"),
          fenceEnd: texts[texts.length - 1].endsWith("\nPOIESIS_TASK>>>"),
          fenceCloses: (texts[texts.length - 1].match(/POIESIS_TASK>>>/g) ?? []).length,
          sawWords: task.includes("the task words"),
          model: currentModel,
        }),
      );
      return end();
    }
    case "memory": {
      const server = (sessionNew?.mcpServers ?? []).find((x) => x.name === "poiesis");
      if (!server) {
        chunk(JSON.stringify({ mounted: false, mcpServers: sessionNew?.mcpServers ?? null }));
        return end();
      }
      await rpc(server.url, "initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "fake-acp" } });
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
      chunk(JSON.stringify({ mounted: true, shape: { type: server.type, headers: server.headers }, listed, results }));
      return end();
    }
    case "novel": {
      // What a newer version might send that this one has never seen.
      update({ sessionUpdate: "something_from_the_future", payload: { a: 1 } });
      send({ method: "session/something_new", params: {} });
      chunk("still works");
      return end();
    }
    case "steer": {
      chunk("working");
      await sleep(1500);
      chunk(" and done");
      return end();
    }
    case "hang": {
      const callId = `call_${++n}`;
      update({ sessionUpdate: "tool_call", toolCallId: callId, name: "shell", title: "shell", kind: "execute", status: "in_progress", locations: [], rawInput: { command: "sleep forever", cwd } });
      await untilCancel();
      return end("cancelled");
    }
    case "ask-forever": {
      await useTool("execute", "echo never answered", { command: "echo never answered", cwd });
      return end("cancelled");
    }
    case "grandchild": {
      // A child of its own, as an npm shim or a build tool would make. Stop must
      // take it away too. It ignores the cancel on purpose.
      const g = spawn(process.execPath, ["-e", "setInterval(()=>{},1000)"], { stdio: "ignore" });
      fs.writeFileSync(path.join(cwd, "grandchild.pid"), String(g.pid));
      await sleep(600000);
      return;
    }
    case "fail": {
      process.stderr.write("fake-acp: something broke\nsecond line\n");
      process.exit(3);
      return;
    }
    default: {
      // A follow-up is whatever was typed; a plain task just finishes.
      chunk(scenario === "plain" && !task.includes("plain task") ? `follow-up got: ${task}` : "done");
      return end();
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

function handle(m) {
  // An answer to something we asked.
  if (m.method === undefined && m.id !== undefined) {
    asked.get(m.id)?.(m.result);
    asked.delete(m.id);
    return;
  }
  if (m.method === "session/cancel") {
    cancelled = true;
    for (const [id, res] of asked) {
      res({ outcome: { outcome: "cancelled" } });
      asked.delete(id);
    }
    for (const w of cancelWaiters.splice(0)) w();
    return;
  }
  const p = m.params ?? {};
  switch (m.method) {
    case "initialize":
      return send({
        id: m.id,
        result: {
          protocolVersion: 1,
          agentCapabilities: { loadSession: true, mcpCapabilities: { http: true, sse: false }, promptCapabilities: {} },
          authMethods: [{ id: "fake-login", name: "Login with the fake agent", description: "Run the fake login in a terminal" }],
          agentInfo: { name: "Fake", version: "0.0.0" },
        },
      });
    case "session/new":
      if (fs.existsSync(path.join(p.cwd, "auth-fail"))) {
        return send({ id: m.id, error: { code: -32000, message: "Authentication required" } });
      }
      SESSION = "ses_fake1";
      sessionNew = p;
      return send({ id: m.id, result: { sessionId: SESSION, configOptions: configOptions() } });
    case "session/load":
      if (p.sessionId === "ses_gone") return send({ id: m.id, error: { code: -32002, message: "Session not found" } });
      SESSION = p.sessionId;
      // History comes back as updates, and is not marked as history.
      chunk("OLD HISTORY");
      return send({ id: m.id, result: { configOptions: configOptions() } });
    case "session/set_config_option":
      if (p.configId === "model" && models.some((x) => x.value === p.value)) currentModel = p.value;
      return send({ id: m.id, result: { configOptions: configOptions() } });
    case "session/prompt":
      prompt(m.id, p);
      return;
    default:
      return send({ id: m.id, error: { code: -32601, message: "method not found" } });
  }
}
