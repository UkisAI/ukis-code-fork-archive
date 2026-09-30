import { mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { startBridge } from "./bridge.mjs";
import {
  discoverClaudeModels,
  claudeCatalog,
  isClaudeModel,
} from "./catalog.mjs";
import { routeModel } from "./openai-forward.mjs";
import { discoverOpenAIModels } from "./openai-catalog.mjs";

export async function configureModels(
  args,
  root,
  { claudeOnly = false, codexExecutable } = {},
) {
  const defaultBinary = path.join(
    homedir(),
    ".local",
    "bin",
    process.platform === "win32" ? "claude.exe" : "claude",
  );
  const executable =
    process.env.UKIS_CLAUDE_BIN ||
    (existsSync(defaultBinary) ? defaultBinary : "claude");
  const directory = await mkdtemp(path.join(tmpdir(), "ukis-models-"));
  if (
    path.dirname(path.resolve(directory)) !== path.resolve(tmpdir()) ||
    !path.basename(directory).startsWith("ukis-models-")
  ) {
    throw new Error("Unexpected temporary provider directory.");
  }
  let bridge;
  try {
    const instructions = await readFile(
      path.join(root, "codex-rs", "models-manager", "prompt.md"),
      "utf8",
    );
    const rows = await discoverClaudeModels(executable, directory).catch(
      (error) => {
        if (claudeOnly) throw error;
        console.error(
          "Claude model discovery is unavailable. Install Claude Code and run claude auth login to add it to /model.",
        );
        return [];
      },
    );
    let openai = { models: [] };
    if (!claudeOnly) {
      openai = await discoverOpenAIModels(
        codexExecutable,
        path.join(root, "codex-rs", "models-manager", "models.json"),
      );
    }
    const models = [
      ...openai.models.filter((model) => !isClaudeModel(model.slug)),
      ...claudeCatalog(rows, instructions),
    ];
    if (!models.length)
      throw new Error("No models are available. Check your provider login.");
    const catalog = path.join(directory, "models.json");
    await writeFile(catalog, JSON.stringify({ models }));
    bridge = await startBridge({
      executable,
      cwd: directory,
      sessionHeader: "x-ukis-session",
      route: (request, context) => routeModel(request, context, { claudeOnly }),
    });
    const overrides = {
      "model_provider": "ukis",
      "model_catalog_json": catalog,
      "model_providers.ukis.name": "Ukis",
      "model_providers.ukis.base_url": bridge.url,
      "model_providers.ukis.env_http_headers.x-ukis-session":
        "UKIS_PROVIDER_TOKEN",
      "model_providers.ukis.wire_api": "responses",
      "model_providers.ukis.requires_openai_auth": !claudeOnly,
      "model_providers.ukis.supports_websockets": false,
      "model_providers.ukis.request_max_retries": 0,
      "model_providers.ukis.stream_max_retries": 0,
      "model_providers.ukis.stream_idle_timeout_ms": 600000,
    };
    const flags = Object.entries(overrides).flatMap(([key, value]) => [
      "-c",
      `${key}=${JSON.stringify(value)}`,
    ]);
    const separator = args.indexOf("--");
    const options = separator < 0 ? args : args.slice(0, separator);
    if (
      claudeOnly &&
      !options.some(
        (arg) =>
          arg === "-m" || arg === "--model" || arg.startsWith("--model="),
      )
    ) {
      flags.push("-m", "sonnet");
    }
    return {
      args:
        separator < 0
          ? [...args, ...flags]
          : [...options, ...flags, ...args.slice(separator)],
      env: { ...process.env, UKIS_PROVIDER_TOKEN: bridge.token },
      async close() {
        await bridge.close();
        await rm(directory, { recursive: true, force: true });
      },
    };
  } catch (error) {
    if (bridge) await bridge.close();
    await rm(directory, { recursive: true, force: true });
    throw error;
  }
}
