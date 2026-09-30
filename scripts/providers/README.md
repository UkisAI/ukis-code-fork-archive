# Claude subscription provider

Use Claude models inside the Ukis Codex terminal and coding harness:

```powershell
# One-time dependency installation from the repository:
cd scripts/providers
npm ci
# Authenticate through the official Claude Code application:
claude auth login

# From any project folder after installing the ukis command:
ukis claude
ukis claude -m opus
ukis --provider claude -m sonnet
ukis --provider openai
```

Node 22+ and Claude Code are required. `UKIS_CLAUDE_BIN` can name a custom Claude Code executable. OpenAI launches do not load these optional dependencies. The launcher checks that Claude Code is signed into a first-party Claude subscription and excludes API-key/cloud-provider environment overrides from the SDK process.

## How it works

A session-local, authenticated HTTP listener on `127.0.0.1` translates Codex Responses requests into official Claude Agent SDK turns. Claude Code manages its own subscription authentication; Ukis never reads, copies, or forwards OAuth credentials.

The SDK receives Codex's full ordered history as a transcript and the current tool declarations through an in-process MCP server. Built-in Claude Code tools are unavailable. Claude's streamed text and requested function/freeform calls are translated back to Codex. Codex performs tool execution with its existing sandbox and approval policy, then sends results into the next model request. The SDK process closes after each model turn. The listener and temporary model catalog close when Codex exits.

The launcher supplies a Claude model catalog for this process (`sonnet`, `opus`, and any explicitly requested model). It leaves saved Codex and Claude configuration files alone. Model aliases resolve through Claude Code; availability depends on the signed-in account. Normal local Codex compaction is used with a conservative 160k context budget.

## Scope and limitations

This is an experimental compatibility provider, not a native Anthropic Messages API implementation. Full-transcript SDK turns add startup/context overhead; large histories may use more subscription capacity than Claude Code directly. Claude's native reasoning state is not carried across turns. OpenAI-specific encrypted compaction histories cannot be resumed with this provider; start a new Claude session.

Text streaming, function tools, namespaced tools, freeform `apply_patch`, tool results, attached data-URL images, cancellation, usage reporting, and SDK errors are supported. Hosted OpenAI web search is disabled for Claude; use an MCP search tool when needed. Other OpenAI-hosted tools are rejected explicitly. Remote image URLs and request bodies over 16 MiB are unsupported. Tests cover image translation; live image quality has not been verified.

`ukis claude` selects the provider for the whole process. Switch between the listed Claude models with `/model`; start a new `ukis` process to return to your usual provider. Anthropic controls subscription availability and billing: see [its current SDK/subscription guidance](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan).

## Validation

```sh
node --test scripts/providers/provider.test.mjs
```

The tests use simulated SDK streams and a real local HTTP listener; no account or model usage is needed. Live checks also verified a response through the Windows Codex executable and a read-only tool/result round trip, and a real `apply_patch` file creation in a dedicated test folder using Claude Code subscription authentication.
