#!/usr/bin/env node
// CLI-0: record one real engine session as fixtures/engines/<name>.jsonl.
// Each line is {"dir":"in"|"out"|"err","t":<ms>,"ch"?:"mcp","msg":{...}}.
//   dir "out" = what the CLI printed, "in" = what we wrote to it.
//   ch "mcp"  = the approve tool served on loopback (MCPS-1): "in" is the
//               request the CLI sent, "out" is our reply.
// Usage (Claude Code):
//   node scripts/record-engine.mjs claude --bin <claude.exe> --cwd <scratch dir> \
//     --out fixtures/engines/claude-2.1.301.jsonl --prompt "..." \
//     [--deny-match "mkdir two"] [--interrupt] [--steer "text"] [--scrub "a@b.c=<email>"]
import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { createRecorder, need, parseArgs } from "./engine-rec.mjs";

const opt = parseArgs(process.argv.slice(3));
need(opt, ["bin", "cwd", "out", "prompt"]);
const recorder = createRecorder(opt);

// The init event lists this user's own plugins, skills and paths. A parser
// does not need them, so they are replaced by a count.
const NOISY_INIT = ["slash_commands", "skills", "plugins", "agents", "memory_paths", "scratchpad_path", "messaging_socket_path", "startup_timing", "powershell_path"];
const rec = (dir, msg, ch) => {
  if (msg?.type === "system" && msg.subtype === "init") {
    msg = { ...msg };
    for (const k of NOISY_INIT) if (k in msg) msg[k] = Array.isArray(msg[k]) ? `<${msg[k].length} items>` : "<trimmed>";
  }
  recorder.rec(dir, msg, ch);
};

// --deny-match <regex>: deny any approve request whose tool input matches it.
const denyRe = opt["deny-match"] ? new RegExp(String(opt["deny-match"])) : null;
let sawTool = false;
let finished = false;
let closeTimer;

const token = Math.random().toString(16).slice(2, 10);
const server = http.createServer((req, res) => {
  if (req.method !== "POST" || req.url !== `/${token}/mcp`) {
    res.writeHead(req.method === "GET" ? 405 : 404).end();
    return;
  }
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => {
    let m;
    try {
      m = JSON.parse(body);
    } catch {
      res.writeHead(400).end();
      return;
    }
    rec("in", m, "mcp");
    const reply = mcpReply(m);
    if (reply === null) {
      res.writeHead(202).end();
      return;
    }
    // --approve-delay-ms keeps tools/call pending, like a user who has not clicked yet.
    const delay = m.method === "tools/call" ? Number(opt["approve-delay-ms"] || 0) : 0;
    let answered = false;
    res.on("close", () => {
      if (!answered) rec("err", { line: `recorder: CLI closed the approve request (id ${m.id}) before we answered` });
    });
    setTimeout(() => {
      if (res.destroyed) return;
      answered = true;
      rec("out", reply, "mcp");
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(reply));
    }, delay);
  });
});

function mcpReply(m) {
  if (m.id === undefined) return null; // notification
  const ok = (result) => ({ jsonrpc: "2.0", id: m.id, result });
  if (m.method === "initialize") {
    return ok({
      protocolVersion: m.params?.protocolVersion || "2025-06-18",
      capabilities: { tools: {} },
      serverInfo: { name: "poiesis", version: "0.0.0" },
    });
  }
  if (m.method === "tools/list") {
    return ok({
      tools: [
        {
          name: "approve",
          description: "Ask the user to approve one tool call.",
          inputSchema: {
            type: "object",
            properties: {
              tool_name: { type: "string" },
              input: { type: "object" },
              tool_use_id: { type: "string" },
            },
            required: ["tool_name", "input"],
          },
        },
      ],
    });
  }
  if (m.method === "tools/call") {
    const a = m.params?.arguments || {};
    const verdict = denyRe && denyRe.test(JSON.stringify(a.input))
      ? { behavior: "deny", message: "Not allowed: the user declined." }
      : { behavior: "allow", updatedInput: a.input };
    return ok({ content: [{ type: "text", text: JSON.stringify(verdict) }] });
  }
  return { jsonrpc: "2.0", id: m.id, error: { code: -32601, message: "method not found" } };
}

await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "poiesis-rec-"));
const mcpCfg = path.join(tmp, "mcp.json");
fs.writeFileSync(
  mcpCfg,
  JSON.stringify({ mcpServers: { poiesis: { type: "http", url: `http://127.0.0.1:${port}/${token}/mcp` } } }),
);
const sysFile = path.join(tmp, "append.txt");
fs.writeFileSync(
  sysFile,
  "You were given one task by a desktop assistant. Work only inside the current folder. Finish with a short report of what you changed and ran.",
);

const cliArgs = [
  "-p",
  "--input-format", "stream-json",
  "--output-format", "stream-json",
  "--verbose",
  "--permission-mode", "default",
  "--append-system-prompt-file", sysFile,
  "--strict-mcp-config",
  "--mcp-config", mcpCfg,
  "--permission-prompt-tool", "mcp__poiesis__approve",
  ...(opt.resume ? ["--resume", String(opt.resume)] : []),
];
rec("in", { argv: ["claude", ...cliArgs.map((x) => (x === mcpCfg ? "<mcp.json>" : x === sysFile ? "<append.txt>" : x))] }, "spawn");

const child = spawn(opt.bin, cliArgs, { cwd: opt.cwd, stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
const send = (msg) => {
  rec("in", msg);
  child.stdin.write(JSON.stringify(msg) + "\n");
};
const userMsg = (text) => ({ type: "user", message: { role: "user", content: [{ type: "text", text }] } });

let buf = "";
child.stdout.on("data", (d) => {
  buf += d;
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    if (!line) continue;
    let m;
    try {
      m = JSON.parse(line);
    } catch {
      rec("out", { raw: line });
      continue;
    }
    rec("out", m);
    onEvent(m);
  }
});
let errBuf = "";
child.stderr.on("data", (d) => {
  errBuf += d;
  let i;
  while ((i = errBuf.indexOf("\n")) >= 0) {
    rec("err", { line: errBuf.slice(0, i).trimEnd() });
    errBuf = errBuf.slice(i + 1);
  }
});

function onEvent(m) {
  if (m.type === "assistant" && !sawTool) {
    const tu = (m.message?.content || []).find((c) => c.type === "tool_use");
    if (tu) {
      sawTool = true;
      if (opt.interrupt) send({ type: "control_request", request_id: "req_interrupt_1", request: { subtype: "interrupt" } });
      else if (opt.steer) send(userMsg(String(opt.steer)));
    }
  }
  if (m.type === "result") {
    // A steered message may start a second turn, so wait for it before closing stdin.
    finished = true;
    clearTimeout(closeTimer);
    closeTimer = setTimeout(() => child.stdin.end(), opt.steer ? 10_000 : 300);
  }
}

const hard = setTimeout(() => {
  rec("err", { line: "recorder: hard timeout, killing" });
  child.kill();
}, 240_000);
if (opt.interrupt) {
  // If the CLI ignores the control request, end the session anyway.
  setTimeout(() => {
    if (!finished) {
      rec("err", { line: "recorder: no result 20s after interrupt, killing" });
      child.kill();
    }
  }, 60_000).unref();
}

send(userMsg(String(opt.prompt)));
const code = await new Promise((r) => child.on("exit", (c) => r(c)));
clearTimeout(hard);
rec("err", { line: `recorder: exit code ${code}` });
server.close();
fs.rmSync(tmp, { recursive: true, force: true });

recorder.save();
console.log(`exit ${code}`);
