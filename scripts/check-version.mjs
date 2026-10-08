// REL-4: a release is only coherent when the tag, tauri.conf.json, package.json
// and Cargo.toml all name the same version. Run by hand before tagging, and by
// the release workflow before it spends twenty minutes building.
//
//   node scripts/check-version.mjs          # the three files agree
//   node scripts/check-version.mjs v0.1.2   # ...and match this tag
import { readFileSync } from "node:fs";

const read = (p) => readFileSync(new URL(`../${p}`, import.meta.url), "utf8");
const found = {
  "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
  "package.json": JSON.parse(read("package.json")).version,
  "src-tauri/Cargo.toml": /^version\s*=\s*"([^"]+)"/m.exec(read("src-tauri/Cargo.toml"))?.[1],
};
const tag = process.argv[2];
if (tag) found[`tag ${tag}`] = tag.replace(/^v/, "");

const versions = new Set(Object.values(found));
for (const [where, v] of Object.entries(found)) console.log(`${v ?? "(missing)"}  ${where}`);
if (versions.size !== 1 || versions.has(undefined)) {
  console.error("\nVersions disagree. Bump all of them to the same number before releasing.");
  process.exit(1);
}
