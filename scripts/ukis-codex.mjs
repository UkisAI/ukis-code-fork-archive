#!/usr/bin/env node
// Launch only a compiled build of this fork; preserve the caller's working directory.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const target = process.env.CARGO_TARGET_DIR
  ? path.resolve(process.env.CARGO_TARGET_DIR)
  : path.join(root, "codex-rs", "target");
const name = process.platform === "win32" ? "codex.exe" : "codex";
const candidates = process.env.UKIS_CODEX_BIN
  ? [path.resolve(process.env.UKIS_CODEX_BIN)]
  : [path.join(target, "release", name), path.join(target, "debug", name)];
const executable = candidates.find(existsSync);
if (!executable) {
  console.error("Ukis Codex has not been built. From this checkout, run:");
  console.error("  cd codex-rs && cargo build --release --bin codex");
  process.exit(1);
}
const child = spawn(executable, process.argv.slice(2), { stdio: "inherit" });
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => child.kill(signal));
}
child.on("error", (error) => {
  console.error(`Unable to start Ukis Codex: ${error.message}`);
  process.exitCode = 1;
});
child.on("exit", (code, signal) => {
  process.exitCode = code ?? (signal === "SIGINT" ? 130 : 143);
});
