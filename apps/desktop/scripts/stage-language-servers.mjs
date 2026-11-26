// Bundles the JSON, CSS, HTML and YAML language servers (from the app's own
// devDependencies) with esbuild into src-tauri/resources/language-servers/,
// one file each, which tauri.conf.json bundles. They run on the person's own
// `node`. Their licenses go beside them.
//
// The SchemaStore schemas they check files against (schemas.lock.json, made
// by update-schemas.mjs) are fetched at the pinned commit, checked against
// their SHA-256 and kept in node_modules/.cache, so a build needs the network
// once. The app never downloads a schema.

import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";

const here = dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));
const app = join(here, "..");
const require = createRequire(join(app, "package.json"));
const target = join(app, "src-tauri", "resources", "language-servers");
const schemasTarget = join(target, "schemas");
const lock = JSON.parse(readFileSync(join(here, "schemas.lock.json"), "utf8"));
const cache = join(app, "..", "..", "node_modules", ".cache", "schemastore", lock.commit);

const packageDir = (name) => dirname(require.resolve(`${name}/package.json`));
const extracted = packageDir("vscode-langservers-extracted");
const yaml = packageDir("yaml-language-server");
const angular = packageDir("@angular/language-server");
// Angular's language service, which its server loads: the project's own
// first, this copy when the project has none.
const angularService = dirname(
  createRequire(join(angular, "package.json")).resolve("@angular/language-service/package.json"),
);
// And the TypeScript it runs on (its own dependency): the server loads only
// these two copies, never a project's, so opening a template runs nothing
// from the workspace.
const angularTypeScript = dirname(
  createRequire(join(angular, "package.json")).resolve("typescript/package.json"),
);
const version = (dir) => JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).version;
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

// What's staged: the servers' versions, this script and the schemas' lock.
const stamp = sha256(
  JSON.stringify([
    version(extracted),
    version(yaml),
    version(angular),
    version(angularService),
    version(angularTypeScript),
    version(packageDir("esbuild")),
    readFileSync(new URL(import.meta.url)),
    lock,
  ]),
);
const marker = join(target, ".stamp");
if (existsSync(marker) && readFileSync(marker, "utf8") === stamp) {
  console.log("stage-language-servers: already staged");
  process.exit(0);
}

rmSync(target, { recursive: true, force: true });
mkdirSync(schemasTarget, { recursive: true });

const esbuild = require("esbuild");
const SERVERS = [
  {
    out: "json-server.cjs",
    entry: join(extracted, "lib/json-language-server/node/jsonServerMain.js"),
  },
  {
    out: "css-server.cjs",
    entry: join(extracted, "lib/css-language-server/node/cssServerMain.js"),
  },
  {
    out: "html-server.cjs",
    entry: join(extracted, "lib/html-language-server/node/htmlServerMain.js"),
  },
  // ESLint's: it loads the project's own eslint (in trusted folders only).
  {
    out: "eslint-server.cjs",
    entry: join(extracted, "lib/eslint-language-server/eslintServer.js"),
  },
  // An ES module: it uses import.meta, so `require` is made for what it bundles.
  {
    out: "yaml-server.mjs",
    entry: join(yaml, "out/src/server.js"),
    format: "esm",
    banner:
      "import { createRequire as __webideRequire } from 'node:module'; const require = __webideRequire(import.meta.url);",
  },
];
for (const server of SERVERS) {
  await esbuild.build({
    entryPoints: [server.entry],
    bundle: true,
    platform: "node",
    target: "node20",
    format: server.format ?? "cjs",
    outfile: join(target, server.out),
    minify: true,
    // Licenses of what's bundled, beside it.
    legalComments: "linked",
    // The language services' ES modules: their UMD builds require by a
    // computed name, which a bundle can't follow.
    mainFields: ["module", "main"],
    logLevel: "warning",
    ...(server.banner ? { banner: { js: server.banner } } : {}),
  });
}
// Angular's server is one file already; it's made smaller. Its language
// service goes in angular/node_modules, where the server looks for it.
const minify = (entry, outfile) =>
  esbuild.build({
    entryPoints: [entry],
    bundle: false,
    minify: true,
    platform: "node",
    target: "node20",
    format: "cjs",
    outfile,
    legalComments: "linked",
    logLevel: "warning",
  });
await minify(join(angular, "index.js"), join(target, "angular-server.cjs"));
const serviceTarget = join(target, "angular", "node_modules", "@angular", "language-service");
mkdirSync(join(serviceTarget, "bundles"), { recursive: true });
for (const file of [
  "package.json",
  "index.js",
  "api_bundle.js",
  "factory_bundle.js",
  "private_bundle.js",
]) {
  copyFileSync(join(angularService, file), join(serviceTarget, file));
}
await minify(
  join(angularService, "bundles", "language-service.js"),
  join(serviceTarget, "bundles", "language-service.js"),
);
// TypeScript for it: its library (made smaller) and the lib.*.d.ts files it reads.
const typeScriptTarget = join(target, "angular", "node_modules", "typescript");
mkdirSync(join(typeScriptTarget, "lib"), { recursive: true });
copyFileSync(join(angularTypeScript, "package.json"), join(typeScriptTarget, "package.json"));
copyFileSync(join(angularTypeScript, "LICENSE.txt"), join(target, "LICENSE-typescript"));
copyFileSync(
  join(angularTypeScript, "lib", "tsserverlibrary.js"),
  join(typeScriptTarget, "lib", "tsserverlibrary.js"),
);
await minify(
  join(angularTypeScript, "lib", "typescript.js"),
  join(typeScriptTarget, "lib", "typescript.js"),
);
for (const file of readdirSync(join(angularTypeScript, "lib"))) {
  if (/^lib\..*\.d\.ts$/.test(file)) {
    copyFileSync(join(angularTypeScript, "lib", file), join(typeScriptTarget, "lib", file));
  }
}
writeFileSync(
  join(target, "LICENSE-angular"),
  "@angular/language-server and @angular/language-service: MIT License, Copyright Google LLC.\nhttps://angular.dev/license\n",
);

// The YAML server's messages, read from a folder beside it.
mkdirSync(join(target, "yaml-l10n"));
copyFileSync(join(yaml, "l10n", "bundle.l10n.json"), join(target, "yaml-l10n", "bundle.l10n.json"));
copyFileSync(join(extracted, "LICENSE"), join(target, "LICENSE-vscode-langservers-extracted"));
copyFileSync(join(yaml, "LICENSE"), join(target, "LICENSE-yaml-language-server"));

const source = (name) =>
  `https://raw.githubusercontent.com/SchemaStore/schemastore/${lock.commit}/src/schemas/json/${name}.json`;
mkdirSync(cache, { recursive: true });
for (const [name, pinned] of Object.entries(lock.files)) {
  const cached = join(cache, `${name}.json`);
  let bytes = existsSync(cached) ? readFileSync(cached) : null;
  if (!bytes || sha256(bytes) !== pinned) {
    const response = await fetch(source(name));
    if (!response.ok) {
      console.error(`stage-language-servers: ${source(name)} answered ${response.status}`);
      process.exit(1);
    }
    bytes = Buffer.from(await response.arrayBuffer());
    if (sha256(bytes) !== pinned) {
      console.error(`stage-language-servers: ${name}.json doesn't match its pinned SHA-256`);
      process.exit(1);
    }
    writeFileSync(cached, bytes);
  }
  copyFileSync(cached, join(schemasTarget, `${name}.json`));
}
writeFileSync(
  join(schemasTarget, "NOTICE"),
  `Schemas from SchemaStore (https://github.com/SchemaStore/schemastore) at ${lock.commit},\nunder the Apache License 2.0.\n`,
);

writeFileSync(marker, stamp);
console.log(
  `stage-language-servers: ${SERVERS.length + 1} servers and ${Object.keys(lock.files).length} schemas → src-tauri/resources/language-servers`,
);
