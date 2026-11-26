// Pins the SchemaStore schemas the JSON and YAML servers check files against
// (Apache-2.0): the ones named below, at one commit of SchemaStore, and every
// SchemaStore schema they refer to, each with its SHA-256, in
// schemas.lock.json. stage-language-servers.mjs fetches exactly those.
//
//   node apps/desktop/scripts/update-schemas.mjs <commit>
//
// The names match ide-lsp's JSON_SCHEMAS and YAML_SCHEMAS; a name SchemaStore
// doesn't have is left out (and its files go without a schema).

import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const ROOTS = [
  // JSON
  "package",
  "tsconfig",
  "jsconfig",
  "eslintrc",
  "prettierrc",
  "babelrc",
  "stylelintrc",
  "lerna",
  "nest-cli",
  // YAML
  "github-workflow",
  "github-action",
  "dependabot-2.0",
  "pnpm-workspace",
];

const commit = process.argv[2];
if (!/^[0-9a-f]{40}$/.test(commit ?? "")) {
  console.error("update-schemas: give a SchemaStore commit (40 hex characters)");
  process.exit(1);
}
const here = dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));
const source = (name) =>
  `https://raw.githubusercontent.com/SchemaStore/schemastore/${commit}/src/schemas/json/${name}.json`;
// A reference to another SchemaStore schema, by its address.
const REFERENCE =
  /https?:\/\/(?:json|www)\.schemastore\.org\/([A-Za-z0-9._-]+?)(?:\.json)?(?=[#"?])/g;
// One by its name alone, beside it on SchemaStore: `"$ref": "ava.json"`.
const RELATIVE = /"\$ref"\s*:\s*"([A-Za-z0-9_-][A-Za-z0-9._-]*?)\.json(?:#[^"]*)?"/g;

const files = {};
const missing = [];
const queue = [...ROOTS];
while (queue.length > 0) {
  const name = queue.shift();
  if (name in files || missing.includes(name)) continue;
  const response = await fetch(source(name));
  if (response.status === 404) {
    missing.push(name);
    continue;
  }
  if (!response.ok) {
    console.error(`update-schemas: ${source(name)} answered ${response.status}`);
    process.exit(1);
  }
  const text = await response.text();
  files[name] = createHash("sha256").update(text).digest("hex");
  for (const [, referenced] of text.matchAll(REFERENCE)) queue.push(referenced);
  for (const [, referenced] of text.matchAll(RELATIVE)) queue.push(referenced);
}

const sorted = Object.fromEntries(Object.entries(files).sort(([a], [b]) => a.localeCompare(b)));
writeFileSync(
  join(here, "schemas.lock.json"),
  `${JSON.stringify({ commit, files: sorted }, null, 2)}\n`,
);
console.log(`update-schemas: ${Object.keys(sorted).length} schemas at ${commit.slice(0, 7)}`);
if (missing.length > 0) console.log(`update-schemas: not in SchemaStore: ${missing.join(", ")}`);
