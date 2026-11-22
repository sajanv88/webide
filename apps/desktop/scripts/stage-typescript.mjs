// Copies the TypeScript language server for this platform (TypeScript 7's
// native `tsc`, which serves LSP with `--lsp --stdio`, and the `lib.*.d.ts`
// files it reads beside it) into src-tauri/resources/typescript/, which
// tauri.conf.json bundles. It comes from the app's own `typescript`
// dependency: the workspace being edited never provides the program run.
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";

const here = dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));
const require = createRequire(join(here, "..", "package.json"));
const typescript = dirname(require.resolve("typescript/package.json"));
const platform = `@typescript/typescript-${process.platform}-${process.arch}`;
const native = dirname(
  createRequire(join(typescript, "package.json")).resolve(`${platform}/package.json`),
);
const lib = join(native, "lib");
const program = join(lib, process.platform === "win32" ? "tsc.exe" : "tsc");
if (!existsSync(program)) {
  console.error(`stage-typescript: no ${program}; is ${platform} installed?`);
  process.exit(1);
}

const target = join(here, "..", "src-tauri", "resources", "typescript");
rmSync(target, { recursive: true, force: true });
mkdirSync(target, { recursive: true });
cpSync(lib, target, { recursive: true });
console.log(`stage-typescript: ${platform} → src-tauri/resources/typescript`);
