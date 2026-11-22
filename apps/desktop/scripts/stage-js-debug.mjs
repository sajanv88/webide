// Fetches vscode-js-debug's DAP server (the debugger for Node), checks it
// against the pinned SHA-256, and unpacks it into src-tauri/resources/js-debug/,
// which tauri.conf.json bundles. It runs on the person's own `node`.
// A copy is kept in node_modules/.cache, so a build needs the network once.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";

const VERSION = "1.140.1";
const SHA256 = "c72fab31bcbfdce6ad1f42adfd28eff8d4681dd3b03b5e2e6f9e72e6f9927faf";
const FILE = `js-debug-dap-v${VERSION}.tar.gz`;
const SOURCE = `https://github.com/microsoft/vscode-js-debug/releases/download/v${VERSION}/${FILE}`;

const here = dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));
const app = join(here, "..");
const cache = join(app, "..", "..", "node_modules", ".cache", "js-debug");
const archive = join(cache, FILE);
const target = join(app, "src-tauri", "resources", "js-debug");
const marker = join(target, ".version");

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

if (existsSync(marker) && readFileSync(marker, "utf8") === VERSION) {
  console.log(`stage-js-debug: ${VERSION} already staged`);
  process.exit(0);
}

let bytes = existsSync(archive) ? readFileSync(archive) : null;
if (!bytes || sha256(bytes) !== SHA256) {
  console.log(`stage-js-debug: downloading ${FILE}`);
  const response = await fetch(SOURCE);
  if (!response.ok) {
    console.error(`stage-js-debug: ${SOURCE} answered ${response.status}`);
    process.exit(1);
  }
  bytes = Buffer.from(await response.arrayBuffer());
  if (sha256(bytes) !== SHA256) {
    console.error(`stage-js-debug: ${FILE} doesn't match its pinned SHA-256; not using it`);
    process.exit(1);
  }
  mkdirSync(cache, { recursive: true });
  writeFileSync(archive, bytes);
}

rmSync(target, { recursive: true, force: true });
mkdirSync(target, { recursive: true });
// The archive holds js-debug/…; unpack its contents into target. Paths are
// relative to the cache, so no drive letter reaches tar (GNU tar would read
// "C:" as a remote host).
const into = relative(cache, target).split(sep).join("/");
execFileSync("tar", ["-xzf", FILE, "-C", into, "--strip-components=1"], {
  cwd: cache,
  stdio: "inherit",
});
// js-debug is CommonJS; without this, Node would take the "type": "module" of
// a package.json above it (this app's own, in a checkout) and fail to load it.
writeFileSync(
  join(target, "package.json"),
  `${JSON.stringify({ type: "commonjs" })}
`,
);
writeFileSync(marker, VERSION);
console.log(`stage-js-debug: ${VERSION} → src-tauri/resources/js-debug`);
