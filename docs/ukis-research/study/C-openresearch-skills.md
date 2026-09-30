# OpenResearch (alphaXiv `orx`) - Knowledge / Skills / Literature layer

Repo: `/home/pavle/projekti/vendor/OpenResearch` @ `ea3b968` (2026-09-30), crate `openresearch-cli` v0.2.14 (`Cargo.toml:2-7`), binary `orx`. Installed locally: `~/.local/bin/orx` 0.2.4 (outdated; latest 0.2.10 reported by the CLI itself). Study was read-only; the only side effect was two live read calls (`orx discover keyword`, `orx paper`) to sample output.

---

## 0. TL;DR architecture

```
                   +-----------------------------------------------+
  SYSTEM_PROMPT.md | "playbook": identity, project facts, evidence  |  injected per harness:
  (rendered)       | & link contract, skill routing list            |  Claude  --append-system-prompt-file
                   +-----------------------------------------------+  Codex   app-server developerInstructions
                                   |                                  OpenCode config `instructions`
                                   v                                  Cursor/Antigravity: first-turn pointer
  agent-skills/orx-*/SKILL.md  (13 native SKILL.md packages, embedded via include_str!)
     -> written fresh every turn into <worktree>/.claude/skills | .agents/skills | .opencode/skills | .cursor/skills
     -> or printed on demand: `orx skill <name>[/resource]`
                                   |
                                   v  agent calls tools = shell commands
  orx discover {keyword,embedding,openalex,biorxiv,pubmed}  -> JSON on stdout
  orx paper <id> [--full]                                   -> Markdown on stdout
  orx create-experiment / exp run|wait|wake|status|desc / runs / logs / agent spawn / ...
```

It is a **tool-via-shell** design: CLI + skill docs, no MCP tool server for research functions, no embedded LLM loop inside `orx` for research. `orx up` *hosts* third-party agent CLIs (Claude Code, Codex, OpenCode, Cursor, Antigravity) as child processes and feeds them the playbook + skills.

---

## 1. Skills: what exists, what each teaches, how they are installed

### 1.1 Three layers of "skill"

| Layer | Source | Consumer |
|---|---|---|
| Top-level overview | `SKILL.md` (repo root, 163 lines), embedded at `src/commands/skill.rs:5` | `orx skill` (no arg) prints it + module index (`skill.rs:43-48`) |
| Modular native skills | `agent-skills/orx-*/SKILL.md` (+ `references/`, `assets/`), embedded `src/local/agent_skills.rs:57-146` | written into session worktree each turn; `orx skill <name>`; `orx install-skills --full` |
| Slash workflows (prompt templates) | `src/local/skills.rs:112-131` CATALOG: `lit-review`, `reproduce-paper`, `write-paper` | `/name` in the `orx up` chat composer, expanded server-side (`skills.rs:1-5`, `135-168`) |
| Thin install shim | `CLAUDE_SKILL` / `CODEX_PROMPT` consts in `src/local/harness/mod.rs:713-773` | `orx install-skills` into global agent dirs |
| User skills | `src/local/user_skills.rs:1-16` - uploads (`orx skills add`) + mirrored skills from `~/.claude/skills`, `~/.agents/skills`, plugins | composer menu; only uploads are written into sessions |

### 1.2 Module list (`agent_skills.rs:252-286`)

`SkillSet::Local` (12, in `orx up` sessions) vs `SkillSet::Full` (13, adds `orx-create`). Chosen by `ORX_LOCAL_SESSION` env (`skill.rs:8-14`, `chat/mod.rs:8733`). Name resolution accepts `compute` or `orx-compute`; resources via `compute/hf` shorthand -> `references/hf.md` (`agent_skills.rs:278-297`). Retired names cleaned up: `orx-lit`, `orx-compute-k8s` (`:44`).

Design note in code (`agent_skills.rs:148-152`): *"Descriptions are the trigger surface ... liberal 'Use when ...' cues (false positives beat false negatives ...). Keep each <=400 chars - Codex's ambient budget is ~8k across the whole set."*

| Module (lines) | What it teaches |
|---|---|
| `orx-experiment-tree` (222) | Core methodology: tree of nodes, frozen vs provisional, "stacked bushes" shape, the auto-research loop (repair/refill/promote/stop), `exp wait --project` tick loop, `exp desc` notes, turn summaries |
| `orx-lit-review` (274) | Literature retrieval loop across alphaXiv/OpenAlex/bioRxiv/PubMed, difficulty-budgeted follow-ups, dedup, reading, figure cropping, quoting and citation-link rules |
| `orx-compute` (130 + 10 refs) | Universal launch contract (only `orx exp run`, commit first, fixed command), backend routing table, `CUSTOM.md` instructions, wait vs wake, sizing |
| `orx-evidence` (47) | Run logs are the only evidence; make runs print config + final metrics + summary; validate before reporting |
| `orx-git` (48) | `orx/<slug>` branches, one worktree owner per branch, commit before launch, never rebase/merge a frozen branch, diff child vs parent |
| `orx-create` (53, Full only) | `orx up` import, prefer existing implementation (find repo via `orx paper`), `create-experiment` semantics |
| `orx-agent-delegation` (51) | `orx agent spawn`: standalone briefs, branch ownership, explicit compute authorization, no nesting, **never delegate lit retrieval** |
| `orx-reports` (49) | Artifacts dir layout, descriptive names, scripts beside figures |
| `orx-figures` (221 + 6 refs + `orx_figstyle.py` 559 lines + TikZ preamble) | Publication-figure non-negotiables, captions, multi-panel, one reference per figure type, `save()` audit line |
| `orx-paper` (191) | LaTeX paper in working tree, template detection `.orx/latex-templates/`, compile-safe preamble, env-to-package table, inline `thebibliography`, numbers only from runs |
| `orx-instances` (23) | `orx instance create` for persistent standalone machines |
| `orx-customize` (28) | `orx skills add` / `orx templates add` |
| `orx-feedback` (61) | Silent product-feedback filing to alphaXiv via `orx feedback` |

### 1.3 Most valuable passages (quoted)

**Cardinal rules** (`SKILL.md:17-47`):
> 1. **Never edit a node once a run has answered it.** ... freezing is permanent: a disappointing result is still a result.
> 2. **The run command *and* the environment are a fixed contract — identical on every node.** ... do **not** vary behavior through environment variables or env-prefixed commands (`LR=3e-4 python …`). The *only* thing that may differ between nodes is the **committed code/config** on the node's git branch.
> 3. **Vary code, not knobs-in-the-command.**
> 4. **Grow the tree downward, not sideways.** ... A root with a long row of direct children and no grandchildren is the failure mode.

**Tree shape rule** (`orx-experiment-tree/SKILL.md:72-85`):
> Before you make X a child of Y, name what Y established that X builds on: You can name it -> real depth ... Descend. You can't — X and Y are co-equal options -> they're **siblings** in the same bush. Fan, don't chain. ... **width = the open options of one decision** ... **depth = decisions already resolved, stacked**.

**Provisional vs frozen + repair cap** (`orx-experiment-tree/SKILL.md:26-47`):
> Unintended behaviour is not an answer. An OOM, a timeout, a divergence from a bug, a missing dep — those are implementation and hardware details, and the node is still provisional ... **Repair cap:** two runs in a row that answer nothing on one node, then ask the user.

**Per-completion loop** (`orx-experiment-tree/SKILL.md:140-173`):
> `exp wait --project` is a sleep-until-change signal, not the source of truth. ... on every wake, re-read `orx runs <projectId>` and reconcile against the set of runs you've already handled.

Stop rule (`:194`): *"Stop when the goal is met, or after ~3 consecutive failed or regressed runs."*

**Evidence** (`orx-evidence/SKILL.md:35-46`): *"Never infer a result from run status or memory ... Truncated output is not evidence of absence."*

**Lit-review scaling** (`orx-lit-review/SKILL.md:106-112, 147-171`):
> Estimate retrieval difficulty from 1–10. This controls a budget of complete follow-up rounds: difficulty 1–3 gets 0 rounds, 4–7 gets 1, and 8–10 gets 2. ... Resolve one publication window and priority ... never widen a window or change priority during the loop.
> If the initial candidates provide solid topical coverage, stop immediately and rank 5–15 IDs. Fast and slightly less complete is better than an exploratory search.
> ... plan against a cap of two complete loops per user turn ... Refuse a fifth loop and answer from the papers already found.

Acronym-recovery trick (`:124-129`): if keyword query mixes words with acronym-like tokens (2-10 chars, >=2 uppercase), run a concurrent second keyword call with only those acronyms.

Anti-hallucination (`:161-165`, `:203-205`, `:245-250`):
> Never invent or recall an ID. ... Read a paper before using it as claim-level support. ... Quote only original text you actually read ... never join separate snippets into a continuous quote.

Explain-with-visuals policy (`:17-30`): answer as "a guided reading" of original figures cropped from the real PDF (`https://arxiv.org/pdf/<id>`), captions with `alphaxiv.org/pdf/<id>?page=N` links; *"Do not substitute thumbnails, redraw results, or generate lookalikes."*

**Delegation** (`orx-agent-delegation/SKILL.md:32-33`): *"Never delegate the retrieval loop covered by `orx-lit-review`: the main agent must inspect and rank the combined literature candidates itself."*

**Paper writing** (`orx-paper/SKILL.md:155-164`): *"A fabricated citation is worse than no citation. ... Every number in a results table must come from an actual run."*

**reproduce-paper slash template** (`src/local/skills.rs:18-53`) - a full reproduction methodology: confirm compute, read paper, plan to compute window (*"keep the available GPUs occupied with scientifically useful parallel variants ... Do not invent or maintain a GPU-hour ledger"*), claim-by-claim assessment with the careful language rule: *"state that this run did not show the reported effect ... Do not characterize the claim as wrong, incorrect, failed, or 'not reproduced'"*, then a visual report, README provenance table with exact run commands, marimo notebook. Local-only variant at `:55-78`.

### 1.4 `orx install-skills` wiring (`src/commands/install_skills.rs`)

- Targets = harnesses whose `skill_target()` is `Some` (`:24-29`). `--agent claude|claude-code|codex|opencode|cursor|antigravity|agy|all|both` (`:63-79`, `:176-182`). No flag -> auto-detect by config-home existence, else all (`:80-93`).
- Writes the thin shim (overwrites) (`:35-51`):

| Agent | Primary file | Extra | Session dir (in `orx up`) |
|---|---|---|---|
| Claude Code | `~/.claude/skills/orx/SKILL.md` (`harness/claude.rs:965-972`) | - | `.claude/skills` |
| Codex | `~/.agents/skills/orx/SKILL.md` (`harness/codex.rs:915-927`) | legacy `~/.codex/prompts/orx.md` (`:934-943`) | `.agents/skills` |
| OpenCode | `$XDG_CONFIG_HOME/opencode/skills/orx/SKILL.md` (`harness/opencode.rs:500-507`) | - | `.opencode/skills` |
| Cursor | `~/.cursor/skills/orx/SKILL.md` (`harness/cursor.rs:228-235`) | - | `.cursor/skills` |
| Antigravity | `~/.gemini/antigravity-cli/skills/orx/SKILL.md` (`harness/antigravity.rs:199-206`) | - | `.agents/skills` |

- The shim (`harness/mod.rs:713-746`) is deliberately tiny: frontmatter `name: orx`, then *"The authoritative operating manual is bundled inside the CLI, so **load it at the start of every session**"* -> run `orx skill`, then `orx skill <name>`. This keeps docs versioned with the binary (no drift).
- `--full` (`main.rs:672-680`, `install_skills.rs:118-172`) additionally writes all 13 `orx-*` skill dirs (with resources) into the global skills dir (`~/.claude/skills`, `~/.agents/skills`, ...). Help text warns this is for orx-dedicated environments because always-listed skills "add noise".
- After `orx login`, installation is offered interactively only on a TTY (`:214-260`).
- Inside `orx up` sessions: `ensure_playbook()` (`src/local/opencode.rs:317-340`) writes the rendered playbook to `.openresearch/agent/autoresearch-local.md` (`:36`) and calls `ensure_session_skills()` (`agent_skills.rs:303-330`) to rewrite the 12 Local modules every turn, plus user uploads and LaTeX templates. These paths are added to `.git/info/exclude` (`opencode.rs:268-305`).
- Current machine: `~/.agents/skills/` exists (other skills) but no `orx` shim installed in either `~/.claude/skills/orx` or `~/.codex/prompts/orx.md`.

---

## 2. Literature tools

### 2.1 Commands (`main.rs:118-123`, `733-825`, `882-895`; `src/commands/discover.rs`; `src/commands/paper.rs`)

| Command | Backend endpoint (from `src/client.rs`) | Notes |
|---|---|---|
| `orx discover keyword "<q>"` | `GET {ALPHAXIV_API_URL=https://api.alphaxiv.org}/search/v2/paper/discover/keyword?q=&prioritize=&publishedAfter=&publishedBefore=` (`client.rs:471-516`) | "alphaXiv full-text BM25 retrieval with match snippets" (`main.rs:777`). Snippets carry `pageNumber`. |
| `orx discover embedding "<q>"` | same path `/discover/embedding` (`client.rs:526-531`) | semantic title/abstract + similarity/popularity rerank; date upper-bound applied after vector retrieval (thin results possible) |
| `orx discover openalex "<q>"` | `GET https://api.openalex.org/works?search=&per_page=&mailto=&select=&filter=from_publication_date:..` (`client.rs:1014-1049`) | non-default priority fetches 4x pool (min 50) then reranks client-side by date/citations (`:1020-1025`, `:1051-1066`) |
| `orx discover biorxiv "<q>"` | OpenAlex filtered to source `S4306402567` (`client.rs:825`, `discover.rs:36-43`) | bioRxiv has no search API |
| `orx discover pubmed "<q>"` | NCBI E-utilities `esearch` (Best Match) + `efetch` XML (`client.rs:1207-1420`), `tool=orx`, `email=` | accepts PubMed field tags |
| `orx paper <id>` (alphaXiv) | `GET https://www.alphaxiv.org/overview/<id>.md` (report, ~10 KB) with fallback to `/abs/<id>.md` (extracted full text); `--full` goes straight to `/abs` (`paper.rs:49-87`, `client.rs:786-816`) + GitHub link from `api.alphaxiv.org/papers/v3/feed?universalId=` (`client.rs:748-784`) | Markdown on stdout: `alphaXiv: <url>`, optional `GitHub: <url>`, blank, body |
| `orx paper <W…/DOI>` | OpenAlex `/works/<id>` (`client.rs:1120-1130`) | metadata + reconstructed abstract, DOI/OA PDF links |
| `orx paper 10.1101/...` | `https://api.biorxiv.org/details/biorxiv/<doi>/na/json` (`client.rs:1175-1177`) | |
| `orx paper <PMID>` | E-utilities efetch | PubMed + PMC links |

Source auto-detection (`paper.rs:265-295`): biorxiv.org / openalex.org / pubmed URL / `pmid:` / DOI (10.1101 -> bioRxiv else OpenAlex) / `W…` id / bare <=9 digit PMID / default alphaXiv. arXiv id parser handles abs/pdf/alphaxiv/ar5iv URLs, versions, old-style `hep-th/9711200` (`paper.rs:355-410`, tests `:428-500`).

Non-agent (dashboard-only) alphaXiv calls also exist: `search_papers_fast` (`/search/v2/paper/fast`, Google-backed title lookup, `client.rs:548-575`), `resolve_paper` (`/papers/v3/{id}` + `/implementations`, author repos preferred, `client.rs:590-690`), `fetch_paper_pdf` from `export.arxiv.org/pdf/` (`client.rs:693-746`). Used by `orx up` "start from a paper" (`commands/up.rs:509, 1393-1463`).

**No Semantic Scholar.** No arXiv API search (only arXiv PDF download).

### 2.2 Auth

None. All literature endpoints are public, unauthenticated; only `user-agent: openresearch-cli/<ver>` (`client.rs:429`). Polite-pool identifiers default to `orx@alphaxiv.org` for OpenAlex `mailto` and NCBI `email` (`config.rs:57-68`). All base URLs overridable by env: `ALPHAXIV_API_URL`, `ALPHAXIV_WEB_URL`, `OPENALEX_API_URL`, `BIORXIV_API_URL`, `PUBMED_API_URL`, `NCBI_EMAIL`, `OPENALEX_MAILTO` (`config.rs:26-68`). Sources can be disabled in settings; both discover and paper refuse disabled sources (`discover.rs:84-93`, `paper.rs:39-47`), and the skill says not to work around it.

### 2.3 Output format

`orx discover` always prints pretty JSON array of `LitHit` (`discover.rs:50`, struct `client.rs:829-850`):
```json
{ "source": "alphaxiv|openalex|biorxiv|pubmed", "id": "<self-routing id for orx paper>",
  "title": "...", "abstract": "...", "publicationDate": "...",
  "votes": 163,            // alphaXiv only
  "citations": 12,         // OpenAlex/bioRxiv only
  "snippets": [{"pageNumber": 2, "snippet": "..."}] }   // alphaXiv keyword only
```
Default `--limit 15` (1..200); alphaXiv pools are fixed server-side so `--limit` can only narrow (`main.rs:803-806`). `--prioritize default|recency|historical|popular`.

Live sample (verified 2026-09-30): `orx discover keyword "GRPO" --limit 2` returned 2026 alphaXiv hits with votes + page snippets; `orx paper 2402.03300` returned `alphaXiv: ...`, `GitHub: https://github.com/deepseek-ai/DeepSeek-Math`, then a long LLM-generated "Research Report" markdown (sections: authors/context, landscape, objectives, ...). Note: the default report is **alphaXiv-generated summary**, which the skill forbids quoting (`orx-lit-review:245-246`: quote only original text, "not generated reports or summaries") - use `--full` for quotable text.

### 2.4 How the agent is told to use them

- Playbook routes every conceptual/research answer through `orx-lit-review`; its description is intentionally broad: *"Use before answering conceptual or architectural questions ... even when no paper, citation, or search is requested"* (`agent_skills.rs:185`).
- `SKILL.md:105-107`: *"Use before any web search for academic/research queries"*. Skill: web search is a fallback only (`orx-lit-review:11-15`).
- Loop = set query (user's own words, no guessed acronym expansions) -> difficulty budget -> initial concurrent calls per source fit -> dedupe (id, then DOI/arXiv id, then normalized title; prefer alphaXiv duplicate) -> stop early or <=2 follow-up rounds, each targeting one concrete gap -> rank 5-15 IDs -> read 3-5 load-bearing papers with `orx paper` only when synthesis is needed.
- Hooks into the rest of methodology: `orx-create` uses `orx paper` to find the author/community implementation before starting a project; `orx-paper` and `write-paper` require citations to come from `orx discover` + `orx paper`; `reproduce-paper` uses `orx paper <id>` for the report.
- The playbook's `Paper:` line injects `orx paper <id>` for paper-seeded projects (`opencode.rs:209-214`).

---

## 3. Every agent-facing CLI subcommand (`src/main.rs:62-181`)

Agent-callable (documented in skills):

| Command | Purpose |
|---|---|
| `orx skill [name[/resource]]` | Print overview + module index, a module, or a resource |
| `orx projects [--json]` | List local projects |
| `orx project view <id> [--archived]` / `project edit <id> --name --run-command` | Show tree / set fixed run command |
| `orx create-experiment <projId> --title [--description --parent --baseline --run-command]` | Add node, prints `orx/<slug>` branch |
| `orx exp status <expId> [--scheduler]` | Branch, parent, run command, latest run + commit |
| `orx exp desc <expId> [--set|--stdin]` | Read/overwrite node notes (markdown) |
| `orx exp run <expId> [--backend --flavor --host --container --manifest --image --timeout --org --provider --disk --force]` | Launch recorded commit on a backend (`main.rs:480-580`) |
| `orx exp cancel <expId>` | Cancel in-flight run |
| `orx exp wait <expId> | --project <id> [--interval --timeout]` | Block until (first) completion; `drained: no runs in flight` |
| `orx exp wake <expId>` | Register a resume-this-session wakeup (only inside `orx up`, `exp.rs:92-97`) |
| `orx exp archive|unarchive <expId> --ancestors|--only|--descendants` | Hide/restore nodes |
| `orx runs <projId> [--experiment]` | Run table, source of truth |
| `orx logs <runId>` | Local log path, size, ~500-char tail preview |
| `orx discover keyword|embedding|openalex|biorxiv|pubmed <q> [--published-after --published-before --prioritize --limit]` | One retrieval primitive, JSON |
| `orx paper <id> [--source --full]` | Fetch report/full text/metadata |
| `orx agent spawn "<task>" [--stdin --title --harness --model --no-wake]` | Spawn helper session (needs `orx up`, max 5 live, no nesting; `agent.rs:1-26`) |
| `orx compute [catalog filters] | status | show <b> | default set|clear | configure <b> | test <b> | connect <b> | ssh-config | instructions show|path|set` | Compute config/catalog (`commands/compute.rs:172-230`) |
| `orx instance create|list|delete` | Standalone OpenResearch boxes (login) |
| `orx orgs`, `orx ssh-key add|list` | Account/org (login) |
| `orx skills add <path>`, `orx templates add <path>` | Save reusable skill / LaTeX template |
| `orx feedback --kind --summary --details [--quote]` | Silent report to maintainers (official builds only) |

Human/infra: `login`, `logout`, `install-skills`, `version`, `update`, `install-cli`, `delete`, `telemetry`, `up [--remote]`, `serve`, `supervise`. Hidden internals: `plan-gate` (Claude plan-mode PreToolUse hook that auto-allows read-only verbs; list at `local/harness/plan_gate.rs:35-39`: projects, orgs, runs, logs, discover, paper, skill, version, feedback), `invocation-gate` (PreToolUse hook that prefixes Bash commands with `ORX_INVOCATION_CONTEXT` for run attribution, `commands/invocation_gate.rs:8-23`), `mcp-gate` (stdio MCP server exposing one tool `approve` used as `--permission-prompt-tool`, a **permission bridge only**, `commands/mcp_gate.rs:1-21, 100-116`), `antigravity-gate`, `publish-branch`, `remote-host`.

Standalone vs `orx up`-dependent: `discover`, `paper`, `skill`, `projects/runs/logs`, `create-experiment`, `exp run/wait/status/desc/cancel` work from any shell against the local SQLite store (`exp run` routes via `orx up` only when `ORX_LOCAL_SESSION` + port env are set, `plane/local_plane.rs:245-275`). `exp wake` and `agent spawn` need the resident `orx up` watcher.

---

## 4. `docs/` directory

Only three files, all operational, none conceptual:
- `docs/linux.md` (68) - AppImage install, FUSE, desktop entry, `orx` on PATH for agents.
- `docs/windows.md` (106) - beta; Git for Windows required as the bash/coreutils source; known gaps.
- `docs/local-models.md` (89) - OpenCode + LM Studio / oMLX / Ollama / custom OpenAI-compatible (vLLM) endpoints; loopback-only; 32K context suggested; notes that local inference still uses network for paper search etc.

The **research methodology/philosophy actually lives in** `SYSTEM_PROMPT.md` (playbook), `SKILL.md` (cardinal rules), `agent-skills/orx-experiment-tree`, `orx-lit-review`, `orx-evidence`, and the `reproduce-paper` template in `src/local/skills.rs`. Hosted docs are at openresearch.sh/docs (README:143), not in-repo. `AGENTS.md` describes the split: `orx` is local (projects, runs, logs, artifacts stay local); `openresearch.sh` owns accounts, orgs, sandboxes, managed compute.

Playbook highlights (`SYSTEM_PROMPT.md`):
- Identity across "ideation, literature review, hypothesis formulation, experiment execution, and artifact generation" (`:18-20`); each chat has its own git worktree (`:21-22`).
- `orx` is source of truth for tree/runs/logs, and *"`orx` is internal and should stay under the hood; do not mention it in user-facing responses"* (`:39-43`).
- Detailed Python/uv environment policy (`:45-70`).
- Evidence-and-links contract: claims cite `<file path=... lines=... exp=... />` and `<run id=... label=... />` raw tags; *"Read the cited run's log before reporting the result; status alone is not evidence"*; label inferences (`:72-112`). These tags are rendered by the `orx up` UI - a Codex fork would need its own convention.
- Skill routing: *"Load the relevant skill before acting in its area."* (`:114-123`), list rendered from module descriptions (`opencode.rs:243-247`).
- Only project state summary is inlined (counts), not the tree (`opencode.rs:163-197`).

---

## 5. Integration contract and implications for a Codex CLI fork

**What it is:** a *CLI + skill-docs* product with a local orchestration daemon. Research capabilities are shell commands with stable textual/JSON output; guidance is plain `SKILL.md` files in the Agent Skills format that Claude Code, Codex (`~/.agents/skills`), OpenCode, Cursor and Antigravity all read natively. `orx` does **not** embed its own LLM agent loop for research; `orx up` spawns the user's existing agent CLI as a child (Codex via `codex app-server` JSON-RPC, playbook in `developerInstructions`, sandbox policy per turn - `harness/codex.rs:1-25`; Claude via resident `claude --print --input-format stream-json` with `--append-system-prompt-file`, `--settings` PreToolUse hooks and `--mcp-config` permission bridge - `harness/claude.rs:1-20, 1053-1120`). MCP is used only for permission prompts, not for exposing research tools.

**Implications for integrating into a Codex CLI fork:**

1. **Cheapest path (zero code): skill + CLI.** Codex already reads `~/.agents/skills/<name>/SKILL.md`. Run `orx install-skills --agent codex --full` (or copy `agent-skills/orx-*` into `~/.agents/skills/`), keep `orx` on PATH. The fork's shell tool calls `orx discover ...` / `orx paper ...`. Works today, inherits upstream updates. Cost: ~8k chars of always-listed descriptions (code comment's own estimate), shell-escaping friction, and prompts reference `orx up`-only features (wake, agent spawn, `<file>`/`<run>` tags, artifacts dir).

2. **Native tools in the fork for the literature layer** is the highest-value, lowest-risk port: the lit layer is ~600 lines of stateless HTTP (`client.rs:429-870, 1014-1420`) + id parsing (`paper.rs:265-410`) over public unauthenticated endpoints, JSON in/out. Reimplement as native tools (`lit_discover{source,query,after,before,prioritize,limit}`, `lit_paper{id,full}`) returning the `LitHit` shape, and pair with the `orx-lit-review` text as the tool-usage guidance (or trimmed into the system prompt). Benefits: structured args (no quoting), parallel tool calls for the "concurrent initial round", no telemetry, no binary dependency. Keep in mind alphaXiv endpoints (`/search/v2/paper/discover/*`, `/overview/<id>.md`, `/abs/<id>.md`, `/papers/v3/feed`) are undocumented private-ish APIs of alphaXiv - they can change or rate-limit; OpenAlex/PubMed/bioRxiv are stable public APIs.

3. **Experiment-tree layer**: better consumed as CLI + skills than re-implemented. It depends on the SQLite store, git worktrees, supervisors and 9 compute backends. A fork could call `orx create-experiment / exp run / exp wait --project / runs / logs` via shell, and replace `exp wake` / `agent spawn` (which need `orx up`) with fork-native mechanisms. The *methodology prompts* (cardinal rules, stacked bushes, repair cap, per-completion loop, evidence rules) are reusable independent of orx.

4. **MCP option**: wrapping `orx discover/paper` as an MCP server would make it harness-agnostic, but upstream does not ship one; note Pavle's environment already has an `arxiv` MCP server (search/read/citation graph) that overlaps partially (no alphaXiv votes/snippets, no OpenAlex/PubMed).

5. **Gotchas when shelling out**: official builds send telemetry (`skill_invoked` per `orx skill` call, `telemetry.rs:1050-1055`; `orx feedback` is designed to be filed *silently* - `orx-feedback/SKILL.md:57-61`). Disable with `orx telemetry off` or `--no-telemetry`. Version-nag warning is printed before output (seen live), so parse JSON from stdout robustly. `orx skill` output differs inside vs outside an `orx up` session (Local vs Full set).

Recommended split for a Codex fork: **native tools for literature (port the Rust logic, reuse the lit-review prompt), skill+CLI for experiment orchestration, no MCP needed.**

---

## 6. Licensing

`LICENSE`: **MIT**, "Copyright (c) 2026 alphaXiv" (`LICENSE:1-3`); `Cargo.toml:7` `license = "MIT"`. MIT covers everything in the repo, including `SKILL.md`, `SYSTEM_PROMPT.md`, `agent-skills/**` prompts, `orx_figstyle.py`, and the Rust client code. Reuse/modify/sublicense/commercial use allowed; only obligation is to keep the copyright + permission notice in copies or substantial portions (e.g. add alphaXiv MIT notice to ported lit client code or vendored skill text). Not covered by the license: the **alphaXiv service/API** itself (terms of `api.alphaxiv.org` / `www.alphaxiv.org` are separate; the MIT code grants no API usage rights), OpenAlex (CC0 data, polite-pool etiquette: set your own `mailto`), NCBI E-utilities usage policy (set own `tool`/`email`, rate limits ~3 req/s without key). If porting, replace the default `orx@alphaxiv.org` contact identifiers with your own. No trademark grant for "OpenResearch"/"alphaXiv" names.
