<p align="center"><img src="branding/ukisai.svg" alt="UkisAI" width="230" /></p>

# Ukis Code

UkisAI's terminal coding agent, forked from [OpenAI Codex](https://github.com/openai/codex).

This first version brings the UkisAI website's interlocking chain logo and pink / violet / blue palette to the terminal. The welcome screen and empty conversation background share the animated mark, including click-to-replay, fading, reduced-motion handling, and small-terminal layout behavior.

The coding engine, authentication, permissions, configuration, and protocol names remain compatible with upstream Codex. This repository contains the CLI and app-server source; a Ukis desktop interface is outside this first version.

![UkisAI terminal logo preview](branding/terminal-preview.png)

The preview above uses cells from the compiled production logo renderer.

## Build and run

Use the Rust toolchain pinned in `codex-rs/rust-toolchain.toml` and the platform prerequisites described in [upstream development instructions](docs/install.md).

```sh
cd codex-rs
cargo build --release --bin codex --bin codex-code-mode-host
cd ..
cd scripts/providers && npm ci && cd ../..
node scripts/ukis-code.mjs
```

Run the launcher from the directory you want the agent to work in, using its absolute path if needed. It uses this checkout's compiled binary. Set `UKIS_CODE_BIN` to an explicit custom build path, or use `CARGO_TARGET_DIR` when building into a different directory. It does not install or fall back to an unmodified OpenAI package.

Windows uses the same Node launcher after building `codex.exe` with the MSVC Rust toolchain and Visual Studio C++ build tools. Linux and macOS builds use `codex`.

Use your existing Codex sign-in and `~/.codex` configuration. This fork does not create a separate account or configuration profile. Model use follows your existing provider/account setup.

## macOS and Linux command

The **Ukis macOS and Linux** workflow builds and tests four native packages: macOS Apple Silicon, macOS Intel, Linux x64, and Linux ARM64. Download the matching artifact from a successful run in [GitHub Actions](https://github.com/UkisAI/ukis-code/actions/workflows/ukis-platforms.yml). These are development builds, not signed macOS releases. The current test platforms are macOS 15 and Ubuntu 24.04; Linux packages require glibc 2.39 or newer and are not Alpine/musl builds.

Install Node.js 22 or newer. Linux also needs the ALSA and libcap runtime libraries (`sudo apt install libasound2t64 libcap2` on Ubuntu 24.04). Extract the artifact's `.tar.gz` into a permanent directory, then run these commands inside the extracted folder:

```sh
node scripts/install-ukis-command.mjs
export PATH="$HOME/.local/bin:$PATH"
ukis --version
```

Add that PATH line to your shell profile if needed. Then run `ukis` from any project directory. The same installer works from a source checkout after building. It preserves arguments and your working directory, and refuses to replace an unrelated existing `ukis` command. Keep the extracted directory in place; rerun the installer if you move it. You can also run `bin/ukis` directly from a downloaded package.

Packages include the model-provider dependencies. For Claude subscription access, install Claude Code separately and run `claude auth login`; OpenAI uses your Codex login. Both providers appear in `/model` when available. On macOS and Linux, `ukis` runs in your current terminal. To use IBM Plex Mono, install the font files under `branding/fonts/ibm-plex-mono` and select it in your terminal's font settings.

## Windows command

The manual **Ukis branding validation** workflow can build a Windows executable and its sandbox helpers. Download the `ukis-code-windows-x64` artifact into `dist/windows/`; the Node launcher detects it automatically.

To install `ukis` in a user command directory already on PATH:

```powershell
.\scripts\install-ukis-command.ps1
```

Run `ukis` from any project folder. The installer provides the single Ukis Code command; arguments and the current working directory are passed through to the CLI.

## Terminal font

The website's **IBM Plex Mono** is available as a dedicated Windows Terminal profile:

```powershell
powershell -File scripts/install-ukis-terminal.ps1
ukis window
```

Plain `ukis` always runs in your current terminal. Use `ukis window` explicitly to open the current project in the IBM Plex Mono profile. To use that font without opening another window, select IBM Plex Mono in your current terminal profile's settings. Avenir and Denton remain website typography; the CLI uses a fixed character grid. See [font source and installation details](branding/fonts/ibm-plex-mono/README.md).

The background logo appears in a fresh, empty conversation when the terminal is tall enough to fit it between the header and prompt. Standard 24-line windows are too short for the full-size mark; maximize the terminal or increase its height. Draft text, conversation activity, reduced motion, and disabled welcome effects also hide it.

## Models and reasoning effort

Run `ukis`, then type `/model` to switch between OpenAI and Claude in the same conversation. The menu includes Fable when available through your Claude account. After selecting a model, choose its reasoning effort; advanced levels such as Max appear under **More reasoning…**. Model choices and supported effort levels are discovered from Claude Code on each launch.

Install the provider dependencies with `npm ci` from `scripts/providers`, and sign into your Claude subscription with `claude auth login`. OpenAI uses your existing Codex login. See [setup, behavior, and limitations](scripts/providers/README.md).

This experimental integration uses the official Claude Agent SDK while Codex handles the conversation, tools, approvals, and sandbox. `ukis --provider openai` starts the native OpenAI provider; the older `ukis claude` command is still available for Claude-only sessions.

## Repeat work with /loop

Use a fixed interval or let the selected model choose when to check again:

~~~text
/loop 5m check whether CI passed
/loop check the deployment and stop when it finishes
/loop list
/loop stop 1
/loop stop
~~~

An interval makes a fixed loop; without one, the loop is adaptive. Seconds round up to whole minutes. Fixed intervals such as 7m and 90m keep their exact duration. Adaptive iterations choose a delay of 1–60 minutes through the scheduling tool, or stop when finished. If a model forgets to schedule, Ukis retries once after 20 minutes and then stops.

Bare /loop uses .ukis/loop.md in the current project, then loop.md in your Codex home, then a maintenance prompt scoped to your existing work. Defaults are reloaded each iteration and limited to 8 KiB. An explicit prompt takes precedence.

Loops run one at a time, wait for your turn, draft, and queued messages, and use the current model, effort, sandbox, and approvals. Missed intervals become one run. A failed or interrupted iteration stops its loop. Esc with an empty composer cancels adaptive loops; /loop stop cancels future runs of all loops. An iteration already running can be interrupted separately.

Up to 50 loops can exist in the current conversation, with a seven-day expiry and one final run when idle. Schedules live in memory: leaving the conversation or closing Ukis removes them, and resume does not restore them. Adaptive controls require a local app-server connection permitted by your MCP configuration; fixed schedules also work with remote connections.

Behavior is informed by [Anthropic's scheduled-task documentation](https://code.claude.com/docs/en/scheduled-tasks).

## Development

```sh
just test -p codex-tui
just fmt
```

Brand assets and implementation details are documented in [branding/README.md](branding/README.md). [UPSTREAM.md](UPSTREAM.md) preserves OpenAI's original README; its download links install upstream Codex, not this branded build.

Keep an `upstream` remote pointing to `https://github.com/openai/codex.git` when bringing in updates. Brand changes stay in the TUI presentation layer; optional provider integration lives in the launcher and `scripts/providers`.

## Attribution

Forked from OpenAI Codex under Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). UkisAI branding is maintained by UkisAI. This is an independent fork, not an official OpenAI product.
