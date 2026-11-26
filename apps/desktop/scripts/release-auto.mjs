// `pnpm release:auto`: builds the installers for the system it runs on, the
// same kinds the Release workflow builds there, and gathers them with their
// SHA-256 sums in target/installers/webide-<version>-<os>-<arch>/. It builds
// for this machine's processor only; the workflow builds every one. Nothing
// is uploaded or published.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, relative } from "node:path";

const BUILDS = {
  win32: { name: "windows", bundles: ["nsis", "msi"], kinds: [".exe", ".msi"] },
  darwin: { name: "macos", bundles: ["dmg"], kinds: [".dmg"] },
  linux: {
    name: "linux",
    bundles: ["deb", "rpm", "appimage"],
    kinds: [".deb", ".rpm", ".AppImage"],
  },
};

const say = (line) => process.stdout.write(`${line}\n`);

const fail = (message) => {
  process.stderr.write(`release:auto: ${message}\n`);
  process.exit(1);
};

const build = BUILDS[process.platform];
if (!build) fail(`no installers are made for ${process.platform}; use Windows, macOS or Linux`);

const here = dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));
const root = join(here, "..", "..", "..");
const config = JSON.parse(readFileSync(join(here, "..", "src-tauri", "tauri.conf.json"), "utf8"));
const version = config.version;
const arch = process.arch;
const targetDir = process.env.CARGO_TARGET_DIR ?? join(root, "target");
const bundleDir = join(targetDir, "release", "bundle");

say(`release:auto: webide ${version} for ${build.name}-${arch} (${build.bundles.join(", ")})`);

// Files older than this are an earlier build's, and aren't gathered.
const started = Date.now();
// Fixed words only, so the command line is safe to hand to the shell, which
// Windows needs to find pnpm.cmd.
const command = `pnpm tauri build --bundles ${build.bundles.join(",")}`;
const result = spawnSync(command, { cwd: root, stdio: "inherit", shell: true });
if (result.error) fail(`could not run "${command}": ${result.error.message}`);
if (result.status !== 0) fail(`"${command}" failed (exit code ${result.status ?? result.signal})`);

const installers = build.bundles
  .map((bundle) => join(bundleDir, bundle))
  .filter((dir) => existsSync(dir))
  .flatMap((dir) => readdirSync(dir).map((file) => join(dir, file)))
  .filter((file) => build.kinds.some((kind) => file.endsWith(kind)))
  .filter((file) => statSync(file).isFile() && statSync(file).mtimeMs >= started - 1000);
if (installers.length === 0) fail(`the build made no installers in ${bundleDir}`);

const out = join(targetDir, "installers", `webide-${version}-${build.name}-${arch}`);
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const sums = installers.map((file) => {
  const name = basename(file);
  copyFileSync(file, join(out, name));
  const sum = createHash("sha256").update(readFileSync(file)).digest("hex");
  const mib = (statSync(file).size / 1024 / 1024).toFixed(2);
  say(`  ${name}  ${mib} MiB  sha256 ${sum}`);
  return `${sum}  ${name}\n`;
});
writeFileSync(join(out, "SHA256SUMS.txt"), sums.join(""));

say(`release:auto: ${installers.length} installer(s) in ${relative(root, out) || out}`);
