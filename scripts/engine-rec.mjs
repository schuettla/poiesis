// Shared by record-engine.mjs and record-acp.mjs (CLI-0 fixture recorders).
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export function parseArgs(list) {
  const opt = {};
  for (let i = 0; i < list.length; i++) {
    if (!list[i].startsWith("--")) continue;
    // --key=value lets a value start with dashes.
    const eq = list[i].indexOf("=");
    if (eq > 0) {
      opt[list[i].slice(2, eq)] = list[i].slice(eq + 1);
      continue;
    }
    const k = list[i].slice(2);
    const next = list[i + 1];
    if (next === undefined || next.startsWith("--")) opt[k] = true;
    else (opt[k] = next), i++;
  }
  return opt;
}

export function need(opt, keys) {
  for (const k of keys) {
    if (!opt[k]) {
      console.error(`missing --${k}`);
      process.exit(2);
    }
  }
}

// Rows are {dir:"in"|"out"|"err", t, ch?, msg}. save() scrubs every string:
// the folder first so it is replaced before the home dir. Each needle is also
// matched with doubled backslashes (JSON text nested in a string) and with
// forward slashes.
export function createRecorder(opt) {
  const t0 = Date.now();
  const lines = [];
  const rec = (dir, msg, ch) => {
    const row = { dir, t: Date.now() - t0 };
    if (ch) row.ch = ch;
    row.msg = msg;
    lines.push(row);
    if (opt.verbose) console.error(`t=${row.t} ${dir}${ch ? ":" + ch : ""} ${JSON.stringify(msg).slice(0, 200)}`);
  };
  const save = () => {
    const needles = [[opt.cwd, "<cwd>"], [os.homedir(), "<home>"], [os.userInfo().username, "<user>"]];
    if (opt.scrub) {
      const [a, b] = String(opt.scrub).split("=");
      needles.unshift([a, b || "<scrubbed>"]);
    }
    const variants = needles.flatMap(([a, b]) =>
      a ? [[a, b], [a.replaceAll("\\", "\\\\"), b], [a.replaceAll("\\", "/"), b]] : [],
    );
    const scrub = (v) => {
      if (typeof v === "string") return variants.reduce((s, [a, b]) => s.split(a).join(b), v);
      if (Array.isArray(v)) return v.map(scrub);
      if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, scrub(x)]));
      return v;
    };
    fs.mkdirSync(path.dirname(opt.out), { recursive: true });
    fs.writeFileSync(opt.out, lines.map((row) => JSON.stringify(scrub(row))).join("\n") + "\n");
    console.log(`wrote ${lines.length} lines to ${opt.out}`);
  };
  return { rec, save };
}
