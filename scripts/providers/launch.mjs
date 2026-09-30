import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { startBridge } from "./bridge.mjs";
import { subscriptionEnvironment } from "./claude-turn.mjs";

export async function configureClaude(args, root) {
  const defaultBinary = path.join(
    homedir(),
    ".local",
    "bin",
    process.platform === "win32" ? "claude.exe" : "claude",
  );
  const executable =
    process.env.UKIS_CLAUDE_BIN ||
    (existsSync(defaultBinary) ? defaultBinary : "claude");
  const { stdout } = await promisify(execFile)(executable, ["auth", "status"], {
    env: subscriptionEnvironment(),
    windowsHide: true,
    timeout: 15_000,
  }).catch(() => {
    throw new Error(
      "Claude Code is required. Install it and run: claude auth login",
    );
  });
  const auth = JSON.parse(stdout);
  if (
    !auth.loggedIn ||
    auth.authMethod !== "claude.ai" ||
    auth.apiProvider !== "firstParty"
  ) {
    throw new Error(
      "Sign into your Claude subscription first: claude auth login",
    );
  }
  const separator = args.indexOf("--");
  const options = separator < 0 ? args : args.slice(0, separator);
  const modelFlag = options.findIndex(
    (arg) => arg === "-m" || arg === "--model",
  );
  const inlineModel = options.find((arg) => arg.startsWith("--model="));
  const selected =
    modelFlag >= 0 ? options[modelFlag + 1] : inlineModel?.slice(8) || "sonnet";
  if (!selected || !/^[a-zA-Z0-9._:[\]-]+$/.test(selected))
    throw new Error("Provide a valid Claude model with -m.");
  const directory = await mkdtemp(path.join(tmpdir(), "ukis-claude-"));
  if (
    path.dirname(path.resolve(directory)) !== path.resolve(tmpdir()) ||
    !path.basename(directory).startsWith("ukis-claude-")
  ) {
    throw new Error("Unexpected temporary provider directory.");
  }
  let bridge;
  try {
    const instructions = await readFile(
      path.join(root, "codex-rs", "models-manager", "prompt.md"),
      "utf8",
    );
    const models = [...new Set([selected, "sonnet", "opus"])].map(
      (slug, priority) => ({
        slug,
        display_name: `Claude ${slug}`,
        description: "Claude Code subscription through Ukis",
        supported_reasoning_levels: [],
        shell_type: "unified_exec",
        visibility: "list",
        supported_in_api: true,
        priority,
        support_verbosity: false,
        apply_patch_tool_type: "freeform",
        truncation_policy: { mode: "tokens", limit: 10000 },
        context_window: 160000,
        auto_compact_token_limit: 120000,
        experimental_supported_tools: [],
        input_modalities: ["text", "image"],
        supports_reasoning_summary_parameter: false,
        model_messages: { instructions_template: instructions },
      }),
    );
    const catalog = path.join(directory, "models.json");
    await writeFile(catalog, JSON.stringify({ models }));
    bridge = await startBridge({ executable, cwd: directory });
    const overrides = {
      "model_provider": "ukis_claude",
      "model_catalog_json": catalog,
      "web_search": "disabled",
      "model_providers.ukis_claude.name": "Claude subscription",
      "model_providers.ukis_claude.base_url": bridge.url,
      "model_providers.ukis_claude.env_key": "UKIS_CLAUDE_BRIDGE_TOKEN",
      "model_providers.ukis_claude.wire_api": "responses",
      "model_providers.ukis_claude.requires_openai_auth": false,
      "model_providers.ukis_claude.supports_websockets": false,
      "model_providers.ukis_claude.request_max_retries": 0,
      "model_providers.ukis_claude.stream_max_retries": 0,
      "model_providers.ukis_claude.stream_idle_timeout_ms": 600000,
    };
    const flags = Object.entries(overrides).flatMap(([key, value]) => [
      "-c",
      `${key}=${JSON.stringify(value)}`,
    ]);
    if (modelFlag < 0 && !inlineModel) flags.push("-m", selected);
    return {
      args:
        separator < 0
          ? [...args, ...flags]
          : [...options, ...flags, ...args.slice(separator)],
      env: { ...process.env, UKIS_CLAUDE_BRIDGE_TOKEN: bridge.token },
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
