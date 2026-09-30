import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { readFile } from "node:fs/promises";

export async function discoverOpenAIModels(
  executable,
  bundledPath,
  { run = promisify(execFile), warn = console.error } = {},
) {
  try {
    // Native discovery owns authentication, cache freshness, and remote refresh.
    // Read its complete catalog so model instructions and capabilities survive.
    const { stdout } = await run(
      executable,
      ["debug", "models", "-c", 'model_provider="openai"'],
      { windowsHide: true, timeout: 15000, maxBuffer: 8 * 1024 * 1024 },
    );
    const catalog = JSON.parse(stdout);
    if (!Array.isArray(catalog.models) || !catalog.models.length)
      throw new Error("Native model catalog is empty.");
    return catalog;
  } catch {
    warn(
      "OpenAI model discovery is unavailable; using this build's bundled catalog.",
    );
    return JSON.parse(await readFile(bundledPath, "utf8"));
  }
}
