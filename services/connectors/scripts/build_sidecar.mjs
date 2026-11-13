#!/usr/bin/env node
// Builds the connectors as a Tauri sidecar:
//   apps/desktop/src-tauri/binaries/cove-connectors-<target triple>[.exe]
// Native AOT when the platform toolchain is there (Linux: clang), otherwise a
// self-contained single file. Skipped when the sources did not change.
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync, chmodSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const project = join(root, "src", "Cove.Connectors");
const binaries = join(root, "..", "..", "apps", "desktop", "src-tauri", "binaries");

const hostTriple = /host: (\S+)/.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))?.[1];
const triple = (process.env.TAURI_ENV_TARGET_TRIPLE ?? "").trim() || hostTriple;
if (!triple) throw new Error("Could not determine the Rust target triple (is rustc installed?)");

const rids = {
  "x86_64-unknown-linux-gnu": "linux-x64",
  "aarch64-unknown-linux-gnu": "linux-arm64",
  "x86_64-pc-windows-msvc": "win-x64",
  "aarch64-pc-windows-msvc": "win-arm64",
  "x86_64-apple-darwin": "osx-x64",
  "aarch64-apple-darwin": "osx-arm64",
};
const rid = rids[triple];
if (!rid) throw new Error(`No .NET runtime identifier for ${triple}`);
const isWindows = triple.includes("windows");
const target = join(binaries, `cove-connectors-${triple}${isWindows ? ".exe" : ""}`);

function files(dir) {
  return readdirSync(dir).flatMap((name) => {
    if (name === "bin" || name === "obj") return [];
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}
const hash = createHash("sha256");
for (const file of files(project).sort()) hash.update(relative(project, file)).update(readFileSync(file));
const digest = hash.digest("hex");
const stamp = join(binaries, `.cove-connectors-${triple}.sha256`);
const isPlaceholder = existsSync(target) && statSync(target).size < 4096;
if (existsSync(target) && !isPlaceholder && existsSync(stamp) && readFileSync(stamp, "utf8") === digest) {
  console.log(`cove-connectors is up to date (${relative(process.cwd(), target)})`);
  process.exit(0);
}

const dotnetRoot = process.env.DOTNET_ROOT;
const dotnet = dotnetRoot ? join(dotnetRoot, isWindows ? "dotnet.exe" : "dotnet") : "dotnet";
const has = (cmd) => spawnSync(cmd, ["--version"], { stdio: "ignore" }).status === 0;
if (!has(dotnet)) {
  console.error("The .NET 9 SDK is required to build the connectors (https://dot.net). On Fedora: sudo dnf install dotnet-sdk-9.0");
  process.exit(1);
}
// Native AOT needs the platform linker; Linux uses clang.
const aot = isWindows || triple.includes("apple") || has("clang");
const out = join(root, "bin", "sidecar", rid);
rmSync(out, { recursive: true, force: true });
const args = ["publish", project, "-c", "Release", "-r", rid, "-o", out, "--nologo"];
// Cross-compiling with Native AOT needs the build machine's compiler package as well.
const hostRid = hostTriple ? rids[hostTriple] : undefined;
if (aot && hostRid && hostRid !== rid) args.push(`-p:CoveHostRid=${hostRid}`);
if (!aot) {
  console.warn("clang not found: building a self-contained single file instead of Native AOT.");
  args.push("-p:PublishAot=false", "-p:SelfContained=true", "-p:PublishSingleFile=true", "-p:EnableCompressionInSingleFile=true");
}
const result = spawnSync(dotnet, args, { stdio: "inherit", env: { ...process.env, DOTNET_CLI_TELEMETRY_OPTOUT: "1", DOTNET_NOLOGO: "1" } });
if (result.status !== 0) process.exit(result.status ?? 1);

mkdirSync(binaries, { recursive: true });
copyFileSync(join(out, `cove-connectors${isWindows ? ".exe" : ""}`), target);
if (!isWindows) chmodSync(target, 0o755);
writeFileSync(stamp, digest);
console.log(`Built ${relative(process.cwd(), target)} (${aot ? "Native AOT" : "self-contained"})`);
