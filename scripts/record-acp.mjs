#!/usr/bin/env node
// CLI-0: record one real ACP agent session (Agent Client Protocol, JSON-RPC
// over stdio). Same fixture format as record-engine.mjs; "in" = our writes,
// "out" = the agent's lines.
// Usage (OpenCode; set OPENCODE_CONFIG_CONTENT='{"permission":{"edit":"ask","bash":"ask"}}'
// first, or it never asks):
//   node scripts/record-acp.mjs --cmd "<opencode.exe>" --acp-args=acp \
//     --cwd <scratch dir> --out fixtures/engines/opencode-2.1.0.jsonl --prompt "..." \
//     [--auth <methodId>] [--deny-match "regex"] [--cancel] [--prompt2 "..."] [--load <id>]
//     [--config mode=plan,model=<id>]   (session/set_config_option before the prompt)
// --cancel sends session/cancel when the first permission request arrives.
// Permission policy: pick the first option of kind allow_once, unless the
// request text matches --deny-match, then the first reject_once option.
import { spawn } from "node:child_process";
import { createRecorder, need, parseArgs } from "./engine-rec.mjs";

const opt = parseArgs(process.argv.slice(2));
need(opt, ["cmd", "cwd", "out", "prompt"]);
const { rec, save } = createRecorder(opt);
const denyRe = opt["deny-match"] ? new RegExp(String(opt["deny-match"])) : null;

const isShim = /\.(cmd|bat)$/i.test(opt.cmd);
const args = String(opt["acp-args"] || "").split(" ").filter(Boolean);
rec("in", { argv: [opt.cmd, ...args] }, "spawn");
const child = spawn(isShim ? "cmd.exe" : opt.cmd, isShim ? ["/d", "/s", "/c", opt.cmd, ...args] : args, {
  cwd: opt.cwd,
  stdio: ["pipe", "pipe", "pipe"],
  windowsHide: true,
});

let nextId = 0;
const pending = new Map();
const request = (method, params) =>
  new Promise((resolve) => {
    const id = ++nextId;
    pending.set(id, resolve);
    const msg = { jsonrpc: "2.0", id, method, params };
    rec("in", msg);
    child.stdin.write(JSON.stringify(msg) + "\n");
  });
const notify = (method, params) => {
  const msg = { jsonrpc: "2.0", method, params };
  rec("in", msg);
  child.stdin.write(JSON.stringify(msg) + "\n");
};
const respond = (id, result) => {
  const msg = { jsonrpc: "2.0", id, result };
  rec("in", msg);
  child.stdin.write(JSON.stringify(msg) + "\n");
};

let cancelled = false;
function onMessage(m) {
  if (m.id !== undefined && m.method === undefined) {
    pending.get(m.id)?.(m);
    pending.delete(m.id);
    return;
  }
  if (m.method === "session/request_permission") {
    const options = m.params?.options || [];
    if (opt.cancel && !cancelled) {
      cancelled = true;
      notify("session/cancel", { sessionId: m.params.sessionId });
      respond(m.id, { outcome: { outcome: "cancelled" } });
      return;
    }
    const wantDeny = denyRe && denyRe.test(JSON.stringify(m.params));
    const pick = options.find((o) => o.kind === (wantDeny ? "reject_once" : "allow_once"));
    respond(m.id, pick ? { outcome: { outcome: "selected", optionId: pick.optionId } } : { outcome: { outcome: "cancelled" } });
  }
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
    // A dead child answers nothing: fail every open request so the file is still saved.
    for (const [id, resolve] of pending) resolve({ id, error: { message: "process exited" } });
    pending.clear();
    r(c);
  }),
);
const init = await request("initialize", {
  protocolVersion: 1,
  clientCapabilities: { fs: { readTextFile: false, writeTextFile: false } },
});
if (opt.auth) await request("authenticate", { methodId: String(opt.auth) });
// --load <id> resumes an earlier session (session/load) instead of starting one.
const session = opt.load
  ? await request("session/load", { sessionId: String(opt.load), cwd: opt.cwd, mcpServers: [] })
  : await request("session/new", { cwd: opt.cwd, mcpServers: [] });
const sessionId = opt.load ? (session.error ? null : String(opt.load)) : session.result?.sessionId;
if (sessionId && opt.config) {
  // --config mode=plan,model=opencode/big-pickle : session/set_config_option for each pair.
  for (const pair of String(opt.config).split(",")) {
    const [configId, value] = pair.split("=");
    await request("session/set_config_option", { sessionId, configId, value });
  }
}
if (sessionId) {
  await request("session/prompt", { sessionId, prompt: [{ type: "text", text: String(opt.prompt) }] });
  if (opt.prompt2) await request("session/prompt", { sessionId, prompt: [{ type: "text", text: String(opt.prompt2) }] });
} else {
  rec("err", { line: "recorder: no session id, see session/new reply" });
}
child.stdin.end();
setTimeout(() => child.kill(), 3000).unref();
const code = await exited;
clearTimeout(hard);
rec("err", { line: `recorder: exit code ${code}; initialize ok=${Boolean(init.result)}` });
save();
