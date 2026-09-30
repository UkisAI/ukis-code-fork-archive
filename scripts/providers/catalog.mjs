import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { query } from "@anthropic-ai/claude-agent-sdk";
import { subscriptionEnvironment } from "./claude-turn.mjs";

export async function discoverClaudeModels(executable, cwd) {
  const { stdout } = await promisify(execFile)(executable, ["auth", "status"], {
    env: subscriptionEnvironment(),
    windowsHide: true,
    timeout: 15000,
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
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  async function* prompt() {
    await gate;
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 15000);
  const session = query({
    prompt: prompt(),
    options: {
      pathToClaudeCodeExecutable: executable,
      cwd,
      tools: [],
      settingSources: [],
      persistSession: false,
      env: subscriptionEnvironment(),
      abortController: controller,
    },
  });
  try {
    return await session.supportedModels();
  } finally {
    clearTimeout(timer);
    release();
    session.close();
  }
}

export function claudeCatalog(rows, instructions) {
  return rows
    .filter((row) => row.value !== "default")
    .map((row, index) => {
      const efforts = row.supportsEffort
        ? (row.supportedEffortLevels ?? [])
        : [];
      return {
        slug: row.value,
        display_name: "Claude " + row.displayName,
        description: row.description,
        supported_reasoning_levels: efforts.map((effort) => ({
          effort,
          description: {
            low: "Quick responses",
            medium: "Balanced reasoning",
            high: "More thorough reasoning",
            xhigh: "Extra thorough reasoning",
            max: "Maximum reasoning",
          }[effort],
        })),
        default_reasoning_level: efforts.includes("medium") ? "medium" : null,
        shell_type: "unified_exec",
        visibility: "list",
        supported_in_api: true,
        priority: 30 + index,
        support_verbosity: false,
        apply_patch_tool_type: "freeform",
        truncation_policy: { mode: "tokens", limit: 10000 },
        context_window: 160000,
        auto_compact_token_limit: 120000,
        experimental_supported_tools: [],
        input_modalities: ["text", "image"],
        supports_reasoning_summary_parameter: false,
        model_messages: { instructions_template: instructions },
      };
    });
}

export function isClaudeModel(model) {
  return /^(claude-|sonnet(?:\[|$)|opus(?:\[|$)|haiku(?:\[|$)|fable(?:\[|$))/.test(
    model,
  );
}
