// Builds the list of apps Composio can connect, and copies their logos into the
// repo, so Browse apps shows every app with no network and no remote images
// (the webview allows only same-origin images).
//
//   node scripts/build-composio-catalog.mjs
//
// Reads Composio's public docs (no key): the toolkit table for slug, name and
// sign-in kind, then each toolkit's page for its one-line description and
// category. Writes:
//   shared/composio-catalog.json   read by the backend (include_str!)
//   public/app-logos/<slug>.svg    served by the app at /app-logos/<slug>.svg
// Logos are the apps' own marks, kept for the one purpose of naming the app.
// Run again to refresh; logos already on disk are not downloaded twice.
import { mkdirSync, existsSync, writeFileSync, statSync, unlinkSync } from "node:fs";

const ROOT = new URL("../", import.meta.url);
const LOGO_DIR = new URL("public/app-logos/", ROOT);
const OUT = new URL("shared/composio-catalog.json", ROOT);
const DOCS = "https://docs.composio.dev";
const LOGOS = "https://logos.composio.dev/api";

mkdirSync(LOGO_DIR, { recursive: true });

async function text(url) {
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      const r = await fetch(url);
      if (r.ok) return await r.text();
      if (r.status === 404) return null;
    } catch {
      // try again
    }
    await new Promise((r) => setTimeout(r, 500 * (attempt + 1)));
  }
  return null;
}

const table = await text(`${DOCS}/toolkits.md`);
if (!table) throw new Error("Could not read the toolkit table.");

// | [Name](/toolkits/path.md) | `SLUG` | tools | triggers | Auth | Managed |
const rows = [];
for (const line of table.split("\n")) {
  const m = /^\| \[(.+?)\]\(\/toolkits\/([^)]+?)\.md\) \| `([^`]+)` \| [^|]*\| [^|]*\| ([^|]*)\| ([^|]*)\|/.exec(line);
  if (!m) continue;
  const [, name, path, slug, auth, managed] = m;
  rows.push({ name, path, slug: slug.toLowerCase(), auth: auth.trim(), managed: managed.trim() });
}
console.log(`${rows.length} toolkits in the table`);

function describe(md) {
  if (!md) return { description: "", category: "" };
  const body = md.replace(/^---[\s\S]*?---\s*/, "");
  const lines = body.split("\n");
  const h = lines.findIndex((l) => l.startsWith("# "));
  let description = "";
  for (let i = h + 1; i < lines.length; i++) {
    const l = lines[i].trim();
    if (!l) continue;
    if (l.startsWith("- **") || l.startsWith("#")) break;
    description = l;
    break;
  }
  const category = /\*\*Category:\*\*\s*(.+)/.exec(body)?.[1]?.trim() ?? "";
  return { description, category };
}

const clip = (s, n) => (s.length <= n ? s : s.slice(0, n - 1).replace(/\s+\S*$/, "") + "…");
const sentence = (s) => (/[.!?…]$/.test(s) || !s ? s : s + ".");
const upper = (s) => (s ? s[0].toUpperCase() + s.slice(1) : s);
const ACRONYMS = { ai: "AI", crm: "CRM", ocr: "OCR", seo: "SEO", hr: "HR", sms: "SMS", url: "URL", it: "IT" };
const tidy = (category) =>
  upper(category).split(" ").map((w) => ACRONYMS[w.toLowerCase()] ?? w).join(" ");
// A few marks embed whole photographs; a megabyte of logo is not worth it.
const MAX_LOGO_BYTES = 24 * 1024;

async function pool(items, size, work) {
  let next = 0;
  await Promise.all(
    Array.from({ length: size }, async () => {
      while (next < items.length) {
        const i = next++;
        await work(items[i], i);
      }
    }),
  );
}

const apps = new Array(rows.length);
let done = 0;
await pool(rows, 16, async (row, i) => {
  const { description, category } = describe(await text(`${DOCS}/toolkits/${row.path}.md`));
  // Composio can sign in without the person's own setup unless it says "No" for
  // an OAuth app. Dynamic client registration apps set themselves up.
  const needsOwnSetup = row.managed === "No" && !row.auth.includes("DCR_OAUTH");
  apps[i] = {
    slug: row.slug,
    name: row.name,
    description: sentence(clip(description, 140)),
    category: tidy(category) || "Other",
    managed: !needsOwnSetup,
    logo: false,
  };
  if (++done % 200 === 0) console.log(`  ${done} pages`);
});

let logos = 0;
done = 0;
await pool(apps, 16, async (app) => {
  const file = new URL(`${app.slug}.svg`, LOGO_DIR);
  if (existsSync(file)) {
    if (statSync(file).size > MAX_LOGO_BYTES) unlinkSync(file);
    else {
      app.logo = true;
      logos++;
    }
    return;
  }
  for (const name of [app.slug, app.slug.replace(/^_+/, "")]) {
    try {
      const r = await fetch(`${LOGOS}/${name}`);
      const type = r.headers.get("content-type") ?? "";
      if (r.ok && type.includes("svg")) {
        const svg = await r.text();
        // A logo is a picture. Anything that could run is not kept.
        if (/<script|onload\s*=|onerror\s*=|javascript:/i.test(svg)) continue;
        if (svg.length > MAX_LOGO_BYTES) return;
        writeFileSync(file, svg);
        app.logo = true;
        logos++;
        return;
      }
    } catch {
      // next name
    }
  }
});
console.log(`${logos} logos of ${apps.length}`);

apps.sort((a, b) => a.name.localeCompare(b.name, "en", { sensitivity: "base" }));
// `logo` says whether /app-logos/<slug>.svg exists; without it the page shows a monogram.
writeFileSync(OUT, JSON.stringify({ apps }) + "\n");
console.log(`wrote ${apps.length} apps`);
