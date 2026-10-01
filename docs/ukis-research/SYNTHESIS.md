# ukis-research: OpenResearch study synthesis

Date: 2026-09-30. Source studied: alphaXiv/OpenResearch `ea3b968` (v0.2.14, MIT).
Detailed reports with file:line refs: `study/A..D-*.md`.

## What OpenResearch actually is

No LLM loop of its own. It is:

1. **Rules** (~300 lines of prose, the real IP): `SKILL.md`, `agent-skills/orx-experiment-tree`,
   `orx-evidence`, `orx-lit-review`, `SYSTEM_PROMPT.md`.
2. **Primitives** (Rust CLI `orx`): experiment node = git branch `orx/<slug>`, run = sha256
   `git archive` snapshot of the branch HEAD, detached supervisor per run, `exp wait --project`
   (returns on first finished run), 9 compute backends behind one trait (local, ssh, slurm, k8s...).
3. **Autonomy plumbing**: an `<orx-goal>` block on every turn + a hidden
   "[orx] Run X finished" message injected into the idle agent when a run ends.
4. **Literature client**: alphaXiv (private API, no auth), OpenAlex, PubMed. ~600 lines of HTTP.
5. **Packaging** (skip): dashboard, telemetry to their server, login/billing, Overleaf, updater.

It drives Claude Code / Codex / OpenCode as child processes (Codex via `codex app-server`).

## Where OpenResearch is weak (our chance to be better)

| Gap | Ukis upgrade |
|---|---|
| "Frozen node" and "same run command" are prompt-only | enforce in code: refuse edits/runs that break them |
| No metrics, agent greps stdout | `metrics.jsonl` contract per run, machine comparison |
| No seed/env/deps capture, exit_code never stored | record seed, env, image, exit code per run |
| No timeout on local/ssh, no GPU queue | per-run timeout + GPU slot accounting |
| Winner picking is prose | validator picks/compares; model proposes (evidence-locked) |

## Mapping onto ukis-code (codex-rs)

The Codex agent loop (`core/src/session/turn.rs`) stays untouched. Upstream already has an
in-process extension framework (`ext/extension-api`) and `/goal` (`ext/goal`) is a working
autonomous-loop template: continue on thread idle, with a token budget.

| OpenResearch piece | ukis-code home |
|---|---|
| goal block every turn + wake on run finish | `ext/goal` pattern (`on_thread_idle`) |
| rules / methodology | skill `SKILL.md` (port MIT text, keep copyright) |
| literature tools | MCP first (works with the Claude bridge, which drops hosted web search), later native ext tool |
| experiment tree + runs + supervisor | research MCP server, later `ext/research` crate |
| ssh GPU backend | `codex exec-server` on the box + `environments.toml` |
| lit-reviewer / experimenter / analyst | agent role TOMLs for `spawn_agent` |

## Answer: does it go into the agent loop?

Not into the core loop. It hooks onto the loop through the extension API (tools, prompt context,
thread-idle continuation). Touching `core/` or the TUI slash enum = painful rebase on every
upstream sync, the fork's own AGENTS.md says to avoid it.

## Recommended phases

- **R1, zero merge cost (prove the loop):** research skill + research MCP server
  (lit search, create-experiment, run, wait, metrics) + role TOMLs + GPU box as exec-server
  environment. Launcher flag `ukis --research`.
- **R2, low merge cost (make it native):** `ext/research` crate modeled on `ext/goal` +
  `ext/web-search`. ~4 one-line upstream hunks (Cargo members/deps, one `install()`, bazel lock).
  Adds experiment-tree summary in context that survives compaction, idle-driven iteration with
  budget, enforced freezing, metric capture.
- **R3, optional:** research dashboard over the app-server protocol (experimental surfaces).

## Requirements (Pavle, 2026-09-30)

1. **Feels factory-made.** Native in ukis-code (`ext/research`), not a bolted-on install.
2. **Papers are used, not just found.** Search, read full text, cite, apply the method.
3. **The agent does not trust itself.** Coding agents ignore test rules and confidently
   declare wins ("You're absolutely right", "I fixed it", invented numbers). The harness knows
   the evaluation METHODOLOGY natively and stays benchmark-agnostic: any command (team bench
   CLI, training script) that emits the results contract plugs in. Numbers come only from
   recorded runs (usually 5 seeded repeats); deterministic code compares and decides, the
   model only proposes.
4. **Provenance per run.** Every run commits the exact command and its results; the experiment
   tree is git.
5. **Record everything.** Every user prompt, model response and tool call (args + output), plus
   raw reasoning when the model is local (hosted Claude/Codex hide it). Used later as data.

6. **Paper -> plan -> tasks, always know where you are.** When following a paper or plan, the
   agent slices it into tasks and at every moment knows what it is doing and how. It writes
   down what it did, how it did it and what changed (journal per task/run). Retrospectives
   feed back into the methodology itself, so our methodologies get better over time.

7. **Methodology repo, one folder per technique.** Methodologies are committed too. A separate
   git repo accumulates everything about each technique: source paper, claims to reproduce,
   how it was reproduced, results, variants, journal, and a versioned how-to. Draft layout:

   ```
   ukis-methods/
     eval/methodology.md            # the evaluation rules themselves, versioned (CHANGELOG)
     techniques/<slug>/             # e.g. grpo, overthinking-penalty
       README.md                    # what it is, status, best known result (from VALID runs only)
       paper/                       # arxiv id, full-text md, quoted claims with page refs
       claims.json                  # paper claims to reproduce: metric, value, setting
       reproductions/<date>-<slug>/ # contract.json, results.json, comparison.json, journal.md,
                                    # link to the experiment commit/branch in the code repo
       methodology.md               # how we do it now; retros append versioned changes
   ```

8. **A mature technique graduates into a skill.** When a technique has VALID reproductions and a
   stable methodology.md, it is exported as a skill (SKILL.md) that any agent can run. The
   framework is technique-agnostic: we ship the mold, the team fills techniques (e.g. swift,
   opsa) and methodologies themselves. Nothing is pre-authored.

9. **Team knowledge loop, not a personal tool.** 5 people now, more later, many parallel lines:
   kernel optimization, swifting, OPD, quants (GSQRSO, Quant_configs), post-training. So:
   `ukis-methods` is a shared UkisAI repo; every record carries the author and machine; the
   results contract is metric-agnostic (accuracy, tok/s, KLD, latency) because a kernel
   lane (tok/s + KLD gate) and an eval (accuracy over seeds) must fit the same validator.
   Goal: knowledge is kept and reproducible, so the team ships more models faster.

## Open decisions

1. R1 MCP server language: Rust (lift orx code directly, one toolchain with the fork) vs Node
   (matches `scripts/providers/*.mjs`). Leaning Rust, since R2 is Rust anyway.
2. Metrics contract: `metrics.jsonl` lines `{step, name, value}` written by the run, or parse
   stdout with a declared regex. Leaning jsonl.
3. First compute targets: pajamgram, Tesla rack (docker exec), halo.
4. Upstream hygiene: this checkout has no `upstream` remote although README says to keep one.
