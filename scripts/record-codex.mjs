#!/usr/bin/env node
// CLI-0: record one real Codex session over `codex app-server` (JSON-RPC on
// stdio, no "jsonrpc" field). Same fixture format as record-engine.mjs:
// "in" = our writes, "out" = the server's lines.
// Usage:
//   node scripts/record-codex.mjs --cmd <codex.exe> --cwd <scratch dir> \
//     --out fixtures/engines/codex-0.163.0.jsonl --prompt "..." \
//     [--deny-match "regex"] [--cancel] [--steer "text"] [--resume <threadId>] \
//     [--policy untrusted|on-request] [--sandbox read-only|workspace-write]
// Approval policy: accept every approval request unless the request JSON
// matches --deny-match, then decline. --cancel sends turn/interrupt when the
// first approval request arrives. --steer sends turn/steer after the first
// item starts. Windows sandbox readiness is only read, never set up.
import { spawn } from "node:child_process";
import { createRecorder, need, parseArgs } from "./engine-rec.mjs";

const opt = parseArgs(process.argv.slice(2));
need(opt, ["cmd", "cwd", "out", "prompt"]);
const recorder = createRecorder(opt);
const save = recorder.save;
// remoteControl/status/changed carries this machine's name and install id.
const rec = (dir, msg, ch) => {
  if (msg?.method === "remoteControl/status/changed" && msg.params) {
    msg = { ...msg, params: { ...msg.params, serverName: "<host>", installationId: "<install-id>" } };
  }
  recorder.rec(dir, msg, ch);
};
const denyRe = opt["deny-match"] ? new RegExp(String(opt["deny-match"])) : null;

rec("in", { argv: [opt.cmd, "app-server"] }, "spawn");
const child = spawn(opt.cmd, ["app-server"], { cwd: opt.cwd, stdio: ["pipe", "pipe", "pipe"], windowsHide: true });

let nextId = 0;
const pending = new Map();
const write = (msg) => {
  rec("in", msg);
  child.stdin.write(JSON.stringify(msg) + "\n");
};
const request = (method, params) =>
  new Promise((resolve) => {
    const id = ++nextId;
    pending.set(id, resolve);
    write({ id, method, params });
  });
const notify = (method, params) => write(params ? { method, params } : { method });

let threadId = null;
let turnId = null;
let firstApproval = true;
let steered = false;
let done;
const completed = new Promise((r) => (done = r));

function onMessage(m) {
  if (m.id !== undefined && m.method === undefined) {
    pending.get(m.id)?.(m);
    pending.delete(m.id);
    return;
  }
  if (m.id !== undefined && m.method) return onServerRequest(m);
  if (m.method === "turn/started") turnId = m.params?.turn?.id ?? turnId;
  if (m.method === "item/started" && opt.steer && !steered && turnId) {
    steered = true;
    request("turn/steer", {
      threadId,
      expectedTurnId: turnId,
      input: [{ type: "text", text: String(opt.steer), text_elements: [] }],
    });
  }
  if (m.method === "turn/completed") done(m.params?.turn?.status);
}

function onServerRequest(m) {
  const reply = (result) => write({ id: m.id, result });
  const wantDeny = denyRe && denyRe.test(JSON.stringify(m.params));
  if (m.method === "item/commandExecution/requestApproval" || m.method === "item/fileChange/requestApproval") {
    if (opt.cancel && firstApproval) {
      firstApproval = false;
      request("turn/interrupt", { threadId, turnId: m.params?.turnId ?? turnId });
      reply({ decision: "cancel" });
      return;
    }
    firstApproval = false;
    reply({ decision: wantDeny ? "decline" : "accept" });
    return;
  }
  // Anything else (permissions, user input, auth refresh): refuse, so the run never blocks.
  write({ id: m.id, error: { code: -32601, message: "recorder does not handle " + m.method } });
}

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
    onMessage(m);
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

const hard = setTimeout(() => {
  rec("err", { line: "recorder: hard timeout, killing" });
  child.kill();
}, 240_000);
const exited = new Promise((r) =>
  child.on("exit", (c) => {
    for (const [id, resolve] of pending) resolve({ id, error: { message: "process exited" } });
    pending.clear();
    done("exited");
    r(c);
  }),
);

await request("initialize", {
  clientInfo: { name: "poiesis", title: null, version: "0.0.0" },
  capabilities: { experimentalApi: false },
});
notify("initialized");
await request("windowsSandbox/readiness", undefined);
const startParams = {
  cwd: opt.cwd,
  approvalPolicy: String(opt.policy || "untrusted"),
  sandbox: String(opt.sandbox || "workspace-write"),
  approvalsReviewer: "user",
};
const thread = opt.resume
  ? await request("thread/resume", { threadId: String(opt.resume), ...startParams })
  : await request("thread/start", startParams);
threadId = thread.result?.thread?.id ?? null;
if (threadId) {
  const started = await request("turn/start", {
    threadId,
    input: [{ type: "text", text: String(opt.prompt), text_elements: [] }],
  });
  turnId = started.result?.turn?.id ?? turnId;
  await completed;
} else {
  rec("err", { line: "recorder: no thread id, see the thread reply" });
}
child.stdin.end();
setTimeout(() => child.kill(), 3000).unref();
const code = await exited;
clearTimeout(hard);
rec("err", { line: `recorder: exit code ${code}` });
save();
