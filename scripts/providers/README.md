# Models and effort in Ukis

Run `ukis`, then use `/model` inside the terminal to choose an OpenAI or Claude model. Selecting a model opens its supported reasoning levels. Choose **More reasoning…** for advanced levels such as **Max**, when available. Press **s** to apply a choice only to the current session.

Claude's model list and effort levels come from the official Claude Agent SDK on each launch. This includes Fable when the installed Claude Code and signed-in account advertise it. Models without effort support, such as Haiku, do not show effort choices. OpenAI models come from the compiled Codex executable's native discovery on each launch. Native discovery handles authentication, cache freshness, and remote refresh; the bundled catalog is used if discovery is unavailable. The launcher preserves complete model metadata, including instructions, capabilities, and supported efforts.

## Setup

Node 22+ and Claude Code are required for the combined model menu. Install dependencies once from this repository:

```powershell
cd scripts/providers
npm ci
claude auth login
# Return to your project folder:
ukis
```

Use the existing Codex login for OpenAI. Claude uses its own subscription login. `UKIS_CLAUDE_BIN` can name a custom Claude Code executable.

`ukis --provider openai` starts the native OpenAI provider without loading the bridge or SDK. The older `ukis claude` command remains available for a Claude-only session, but is unnecessary for switching models in ordinary `ukis` sessions.

## How it works

A session-local, authenticated HTTP listener on `127.0.0.1` routes each turn according to the selected model. OpenAI requests retain Codex's native authentication and stream to the official OpenAI endpoint. Claude requests are translated into official Claude Agent SDK turns; OpenAI authorization headers are never passed to Claude. Claude Code manages its own subscription credentials, and API-key/cloud-provider environment overrides are excluded from its SDK process.

Codex owns the conversation across model changes. The SDK receives its ordered history as a transcript and current tool declarations through an in-process MCP server. Built-in Claude Code tools are unavailable. Claude's streamed text and function/freeform calls are translated back to Codex, which executes tools with its existing sandbox and approval policy and returns their results. The SDK process closes after each model turn. The listener and temporary catalog close when Ukis exits.

The launcher supplies configuration overrides for the running process. Normal `/model` choices can still be saved by Codex; use the session-only option to avoid saving a choice. New unified sessions use local Codex compaction, with a conservative 160k context budget for Claude.

## Scope and limitations

This is an experimental compatibility provider. Full-transcript SDK turns add startup/context overhead; large histories may use more subscription capacity than Claude Code directly. Claude's native reasoning state is not carried across turns. Older sessions containing OpenAI-specific encrypted compaction cannot be continued with Claude; start a new unified session.

Text streaming, function tools, namespaced tools, freeform `apply_patch`, tool results, attached data-URL images, cancellation, usage reporting, and SDK errors are supported. Hosted OpenAI web search stays available for OpenAI and is omitted from Claude requests; use an MCP search tool with Claude. Other OpenAI-hosted tools are rejected explicitly by Claude translation. Remote image URLs and request bodies over 16 MiB are unsupported. Tests cover image translation; live image quality has not been verified.

The combined route supports standard OpenAI API-key and ChatGPT authentication; custom OpenAI-compatible gateways are outside its scope. Catalogs are loaded at launch, so reopen Ukis to pick up newly available Claude models. Anthropic controls model access, subscription availability, and billing; see [its SDK/subscription guidance](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan).

## Validation

```sh
node --test scripts/providers/provider.test.mjs scripts/providers/unified.test.mjs
```

Automated tests exercise simulated SDK streams, actual local HTTP routing, model/effort discovery mapping, authentication boundaries, cancellation, and OpenAI forwarding without using a model account. Live Windows checks verified OpenAI → Claude → OpenAI in the same conversation with its history retained, plus the actual `/model` menu and Fable's Max effort selection. Earlier Claude checks verified a read-only tool/result round trip and `apply_patch` file creation in a dedicated test folder.
