#!/usr/bin/env node
// Launch only a compiled build of this fork; preserve the caller's working directory.
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { providerOptions } from "./provider-options.mjs";
import { findUkisBinary } from "./ukis-binary.mjs";
import { openUkisWindow, shouldOpenUkisWindow } from "./ukis-window.mjs";

if (process.argv[2] === "window") {
  process.exit(await openUkisWindow(process.argv.slice(3)));
}

if (shouldOpenUkisWindow(process.argv.slice(2))) {
  const code = await openUkisWindow([]);
  if (code === 0) process.exit(0);
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const executable = findUkisBinary(root);
if (!executable) {
  console.error("Ukis Code has not been built. From this checkout, run:");
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
      codexExecutable: executable,
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
    console.error(`Unable to start Ukis Code: ${error.message}`);
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
