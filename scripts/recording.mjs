import { homedir } from "node:os";
import path from "node:path";

// Codex always writes rollouts to $CODEX_HOME/sessions (prompts, replies,
// tool calls with output, reasoning). UKIS_RECORD=1 adds rollout-trace
// bundles: the exact request of every inference plus raw tool payloads.
// Codex reads CODEX_ROLLOUT_TRACE_ROOT itself (codex-rs/rollout-trace/src/
// thread.rs), so enabling it needs no Rust change. It is opt-in because it
// rewrites the full context on every inference and grows with session length.
export function recordingEnvironment(env = process.env, home = homedir()) {
  if (env.UKIS_RECORD !== "1") return env;
  // A root the user already chose wins; an empty value would make Codex write
  // bundles into whatever directory the session runs in.
  if (env.CODEX_ROLLOUT_TRACE_ROOT) return env;
  return {
    ...env,
    CODEX_ROLLOUT_TRACE_ROOT: path.join(home, ".ukis", "traces"),
  };
}

// --ephemeral turns off the rollout file, which is the primary record.
export function recordingWarning(env, args) {
  const separator = args.indexOf("--");
  const options = separator < 0 ? args : args.slice(0, separator);
  if (env.UKIS_RECORD === "1" && options.includes("--ephemeral"))
    return "UKIS_RECORD=1: --ephemeral skips the session rollout, so this session is only partly recorded.";
}
