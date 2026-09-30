#!/usr/bin/env node
// Launch only a compiled build of this fork; preserve the caller's working directory.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { providerOptions } from "./provider-options.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const target = process.env.CARGO_TARGET_DIR
  ? path.resolve(process.env.CARGO_TARGET_DIR)
  : path.join(root, "codex-rs", "target");
const name = process.platform === "win32" ? "codex.exe" : "codex";
const candidates = process.env.UKIS_CODEX_BIN
  ? [path.resolve(process.env.UKIS_CODEX_BIN)]
  : [
      path.join(target, "release", name),
      path.join(target, "debug", name),
      path.join(root, "dist", "windows", name),
    ];
const executable = candidates.find(existsSync);
if (!executable) {
  console.error("Ukis Codex has not been built. From this checkout, run:");
  console.error("  cd codex-rs && cargo build --release --bin codex");
  process.exit(1);
}
let configuration;
try {
  const { provider, args } = providerOptions(process.argv.slice(2));
  if (
    provider !== "openai" &&
    !args.some((arg) => ["--help", "-h", "--version", "-V"].includes(arg))
  ) {
    const { configureModels } = await import("./providers/launch.mjs").catch(
      (error) => {
        if (error.code === "ERR_MODULE_NOT_FOUND")
          throw new Error(
            "Install Claude support first: cd scripts/providers && npm ci",
          );
        throw error;
      },
    );
    configuration = await configureModels(args, root, {
      claudeOnly: provider === "claude",
    });
  } else {
    if (provider === "openai") args.unshift("-c", 'model_provider="openai"');
    configuration = { args, env: process.env, close: async () => {} };
  }
  const child = spawn(executable, configuration.args, {
    stdio: "inherit",
    env: configuration.env,
  });
  for (const signal of ["SIGINT", "SIGTERM"])
    process.on(signal, () => child.kill(signal));
  child.on("error", (error) => {
    console.error(`Unable to start Ukis Codex: ${error.message}`);
    process.exitCode = 1;
  });
  child.on("close", async (code, signal) => {
    await configuration.close();
    process.exitCode = code ?? (signal === "SIGINT" ? 130 : 143);
  });
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
