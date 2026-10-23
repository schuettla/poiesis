// A stand-in for Claude Code, for tests (`CLI-T-2`).
//
// It speaks the same stream-json on stdin/stdout as `claude -p ...` and, like the
// real one, asks Poiesis's MCP endpoint (`--mcp-config`) before it uses a tool.
// What it does is chosen by a word in the task: `SCENARIO:<name>`. It is only a
// test double; the recordings in this folder are what the real one does.
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";

const argv = process.argv.slice(2);
const arg = (name) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const out = (o) => process.stdout.write(JSON.stringify(o) + "\n");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const mode = arg("--permission-mode");
const resume = arg("--resume");
const mcpPath = arg("--mcp-config");
const session = resume && resume !== "gone-session" ? resume : "fake-session-1";

// ---- stdin: one JSON user message per line ----
const messages = [];
const waiters = [];
let buf = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (d) => {
  buf += d;
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    if (!line) continue;
    const m = JSON.parse(line);
    if (m.type === "control_request" && m.request?.subtype === "interrupt") {
      interrupted = true;
      out({ type: "control_response", response: { subtype: "success", request_id: m.request_id, response: {} } });
      for (const w of waiters.splice(0)) w(null);
      continue;
    }
    // A message goes to whoever is waiting, else it is kept for the next ask.
    const waiting = waiters.shift();
    if (waiting) waiting(m);
    else messages.push(m);
  }
});
process.stdin.on("end", () => {
  ended = true;
  for (const w of waiters.splice(0)) w(null);
});
let interrupted = false;
let ended = false;
const nextMessage = () =>
  new Promise((r) => {
    if (messages.length) return r(messages.shift());
    if (interrupted || ended) return r(null);
    waiters.push(r);
  });
const textOf = (m) => (m?.message?.content ?? []).map((c) => c.text ?? "").join("");

// ---- the MCP endpoint ----
let mcpUrl = null;
let rpcId = 0;
async function rpc(method, params, notify = false) {
  const body = { jsonrpc: "2.0", method, params };
  if (!notify) body.id = rpcId++;
  const res = await fetch(mcpUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  if (notify) return null;
  return res.json();
}
async function handshake() {
  if (!mcpPath) return;
  mcpUrl = JSON.parse(fs.readFileSync(mcpPath, "utf8")).mcpServers.poiesis.url;
  await rpc("server/discover", {});
  await rpc("initialize", { protocolVersion: "2025-11-25", capabilities: {}, clientInfo: { name: "fake-claude" } });
  await rpc("notifications/initialized", {}, true);
  await rpc("tools/list", {});
}
async function approve(tool_name, input, tool_use_id) {
  if (!mcpUrl) return { behavior: "allow", updatedInput: input };
  const r = await rpc("tools/call", { name: "approve", arguments: { tool_name, input, tool_use_id } });
  return JSON.parse(r.result.content[0].text);
}

// ---- events ----
let n = 0;
const toolUse = (name, input) => {
  const id = `toolu_${++n}`;
  out({ type: "assistant", message: { role: "assistant", content: [{ type: "tool_use", id, name, input }] } });
  return id;
};
const toolResult = (id, text, isError, source) =>
  out({
    type: "user",
    message: { role: "user", content: [{ type: "tool_result", tool_use_id: id, content: text, is_error: isError }] },
    tool_result_meta: [{ id, permission_decision: { decision: isError ? "reject" : "accept", source } }],
  });
const finish = (text, extra = {}) => {
  out({
    type: "result",
    subtype: "success",
    is_error: false,
    result: text,
    session_id: session,
    stop_reason: "end_turn",
    total_cost_usd: 0.01,
    usage: { input_tokens: 10, output_tokens: 5, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 },
    permission_denials: [],
    ...extra,
  });
};

async function useTool(name, input) {
  const id = toolUse(name, input);
  const v = await approve(name, input, id);
  if (v.behavior === "deny") {
    toolResult(id, `Error: ${v.message}`, true, "user_reject");
    return false;
  }
  if (name === "Write") fs.writeFileSync(input.file_path, input.content);
  toolResult(id, name === "Bash" ? "ok" : "done", false, "user_temporary");
  return true;
}

async function main() {
  const first = await nextMessage();
  if (!first) process.exit(0);
  const task = textOf(first);
  out({ type: "system", subtype: "hook_started", hook_name: "SessionStart:startup" });
  out({ type: "system", subtype: "init", session_id: session, model: "fake-model", apiKeySource: "none", permissionMode: mode });

  if (resume === "gone-session") {
    out({
      type: "result",
      subtype: "error_during_execution",
      is_error: true,
      result: null,
      errors: ["No conversation found with session ID: gone-session"],
      session_id: session,
    });
    process.exit(1);
  }
  await handshake();
  out({ type: "assistant", message: { role: "assistant", content: [{ type: "text", text: "Starting." }] } });

  const scenario = /SCENARIO:([a-z-]+)/.exec(task)?.[1] ?? "plain";
  const cwd = process.cwd();
  switch (scenario) {
    case "command": {
      const ok = await useTool("Bash", { command: "echo hi" });
      return finish(ok ? "ran the command" : "the command was refused");
    }
    case "same-command": {
      await useTool("Bash", { command: "echo same" });
      await useTool("Bash", { command: "echo same" });
      return finish("ran it twice");
    }
    case "edit": {
      const file = path.join(cwd, "note.txt");
      const ok = await useTool("Write", { file_path: file, content: "hello" });
      return finish(ok ? "wrote note.txt" : "the write was refused");
    }
    case "read": {
      const id = toolUse("Read", { file_path: path.join(cwd, "package.json") });
      toolResult(id, "{}", false, "config");
      return finish("read it without asking");
    }
    case "report": {
      const env = process.env;
      return finish(
        JSON.stringify({
          mode,
          cwd,
          poiesisEnv: Object.keys(env).filter((k) => k.toUpperCase().startsWith("POIESIS_")),
          anthropicKey: env.ANTHROPIC_API_KEY ?? null,
          openaiKey: env.OPENAI_API_KEY ?? null,
          argv: argv.filter((a) => !a.includes(path.sep)),
          appendFile: arg("--append-system-prompt-file") ? fs.readFileSync(arg("--append-system-prompt-file"), "utf8") : null,
          // Poiesis defangs the report it hands the lead, so what was received is
          // described here, not echoed.
          fenceStart: task.startsWith("<<<POIESIS_TASK\nSCENARIO:report"),
          fenceCloses: (task.match(/POIESIS_TASK>>>/g) ?? []).length,
          fenceEnd: task.endsWith("\nPOIESIS_TASK>>>"),
          sawWords: task.includes("the task words"),
        }),
      );
    }
    case "memory": {
      // Poiesis's own tools (`MCPS-2`), called the way Claude Code calls an MCP
      // tool: it asks the permission tool first, then calls the tool.
      const listed = (await rpc("tools/list", {})).result.tools.map((t) => t.name);
      const results = {};
      const calls = [
        ["recall", { query: "pnpm" }],
        ["present", { kind: "progress", title: "Tests", current: 1, total: 4 }],
        ["ask_user", { question: "Which package manager?", options: ["pnpm", "npm"] }],
        ["remember", { text: "This project uses pnpm, never npm." }],
      ];
      for (const [name, args] of calls) {
        const full = `mcp__poiesis__${name}`;
        const id = toolUse(full, args);
        const v = await approve(full, args, id);
        if (v.behavior === "deny") {
          toolResult(id, `Error: ${v.message}`, true, "user_reject");
          results[name] = `denied: ${v.message}`;
          continue;
        }
        const r = await rpc("tools/call", { name, arguments: args });
        results[name] = r.error ? `error: ${r.error.message}` : r.result.content[0].text;
        toolResult(id, results[name], false, "user_temporary");
      }
      return finish(JSON.stringify({ listed, results, appendFile: arg("--append-system-prompt-file") ? fs.readFileSync(arg("--append-system-prompt-file"), "utf8") : null }));
    }
    case "steer": {
      const id = toolUse("Bash", { command: "echo waiting" });
      toolResult(id, "ok", false, "config");
      const more = await nextMessage();
      return finish(`steered with: ${textOf(more)}`);
    }
    case "hang": {
      toolUse("Bash", { command: "sleep forever" });
      await nextMessage();
      out({ type: "result", subtype: "error_during_execution", is_error: true, result: null, errors: ["interrupted"], session_id: session });
      process.exit(1);
    }
    case "ask-forever": {
      await useTool("Bash", { command: "echo never answered" });
      return finish("answered");
    }
    case "grandchild": {
      // A child of its own, as an npm shim or a build tool would make. Stop must
      // take it away too. It ignores the interrupt on purpose.
      const g = spawn(process.execPath, ["-e", "setInterval(()=>{},1000)"], { stdio: "ignore" });
      fs.writeFileSync(path.join(cwd, "grandchild.pid"), String(g.pid));
      toolUse("Bash", { command: "sleep forever" });
      await sleep(600000);
      return;
    }
    case "fail": {
      process.stderr.write("fake-claude: something broke\nsecond line\n");
      process.exit(3);
    }
    default:
      return finish("done");
  }
}

main().then(() => {
  // After the result, a real `claude -p` waits for stdin to close and exits.
  process.stdin.on("end", () => process.exit(0));
  setTimeout(() => process.exit(0), 5000);
});
