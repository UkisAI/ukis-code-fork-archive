import { createHash, randomUUID } from "node:crypto";
import { query, createSdkMcpServer } from "@anthropic-ai/claude-agent-sdk";
import {
  ListToolsRequestSchema,
  CallToolRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";

// Tools are declarations only. Codex, including its approval/sandbox checks,
// is the sole executor. A fresh SDK turn receives Codex's complete transcript.
export function prepareTools(specs = []) {
  const tools = new Map();
  function add(spec, namespace) {
    if (spec.type === "namespace") {
      for (const child of spec.tools) add(child, spec.name);
      return;
    }
    if (!["function", "custom"].includes(spec.type)) {
      throw new Error(
        `Claude provider does not support hosted tool ${spec.type}. Use an MCP equivalent.`,
      );
    }
    const key = createHash("sha256")
      .update(JSON.stringify([namespace, spec.name]))
      .digest("hex")
      .slice(0, 24);
    const alias = `tool_${key}`;
    tools.set(`mcp__ukis__${alias}`, {
      spec,
      namespace,
      declaration: {
        name: alias,
        description: `${namespace ? namespace + "." : ""}${spec.name}: ${spec.description ?? ""}`,
        inputSchema:
          spec.type === "custom"
            ? {
                type: "object",
                properties: {
                  input: {
                    type: "string",
                    description:
                      "The complete raw tool input, without JSON encoding or Markdown fences.",
                  },
                },
                required: ["input"],
                additionalProperties: false,
              }
            : spec.parameters,
        _meta: { "anthropic/alwaysLoad": true },
      },
    });
  }
  for (const spec of specs) add(spec);
  return tools;
}

export function subscriptionEnvironment(env = process.env) {
  const result = { ...env };
  // This provider explicitly uses Claude login, never incidental API credentials.
  for (const name of [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_PROFILE",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
  ])
    delete result[name];
  return result;
}

// Claude thinking becomes a Responses reasoning item so Codex stores it in the
// rollout like any other reasoning. The ids are minted here, never by OpenAI,
// so openai-forward.mjs drops these items before an OpenAI turn.
export const UKIS_REASONING_PREFIX = "rs_ukis_";

// Summarized is the most Claude exposes; raw chain of thought is never sent.
// UKIS_CLAUDE_THINKING_DISPLAY=omitted hides it, =default keeps the SDK choice.
export function thinkingOption(env = process.env) {
  const display = env.UKIS_CLAUDE_THINKING_DISPLAY || "summarized";
  if (display === "default") return undefined;
  if (!["summarized", "omitted"].includes(display))
    throw new Error(
      "UKIS_CLAUDE_THINKING_DISPLAY must be summarized, omitted or default.",
    );
  return { type: "adaptive", display };
}

export async function runClaudeTurn(
  request,
  { signal, emit, executable, cwd, queryImpl = query },
) {
  if (!Array.isArray(request.input))
    throw new Error("Claude requires a full conversation input.");
  if (
    request.input.some((item) =>
      ["compaction", "compaction_summary", "item_reference"].includes(
        item.type,
      ),
    )
  ) {
    throw new Error(
      "This history contains provider-specific state. Start a new Claude session instead of resuming an OpenAI-compacted session.",
    );
  }
  const tools = prepareTools(request.tools);
  const server = createSdkMcpServer({ name: "ukis", version: "1.0.0" });
  server.instance.server.registerCapabilities({ tools: {} });
  server.instance.server.setRequestHandler(
    ListToolsRequestSchema,
    async () => ({
      tools: [...tools.values()].map((tool) => tool.declaration),
    }),
  );
  // Never execute or acknowledge a tool as successful inside Claude Code.
  server.instance.server.setRequestHandler(CallToolRequestSchema, async () => ({
    isError: true,
    content: [
      { type: "text", text: "Execution belongs to the Ukis Code host." },
    ],
  }));
  const controller = new AbortController();
  const abort = () => controller.abort();
  signal.addEventListener("abort", abort, { once: true });
  if (signal.aborted) controller.abort();
  const id = `resp_${randomUUID()}`;
  const output = [];
  const blocks = new Map();
  let usage = {};
  let toolTurn = false;
  let completed = false;
  const send = (type, rest = {}) => emit({ type, ...rest });
  // Both OpenAI's encrypted reasoning and our own rs_ukis_ thinking summaries
  // are dropped: Claude cannot verify either, and the summaries are records,
  // not state Claude needs back.
  const history = request.input.filter((item) => item.type !== "reasoning");
  const images = [];
  const transcript = JSON.stringify(history, (key, value) => {
    if (value?.type === "input_image") {
      const match =
        /^data:(image\/(?:png|jpeg|gif|webp));base64,([\s\S]+)$/.exec(
          value.image_url,
        );
      if (!match)
        throw new Error(
          "Claude currently accepts attached images as data URLs only.",
        );
      images.push({
        type: "image",
        source: { type: "base64", media_type: match[1], data: match[2] },
      });
      return { type: "image_reference", image_number: images.length };
    }
    return value;
  });
  async function* prompt() {
    yield {
      type: "user",
      message: {
        role: "user",
        content: [
          {
            type: "text",
            text:
              "Continue this Ukis Code conversation. This JSON is the ordered conversation history; respect its message roles and tool results. Do not repeat completed work.\n" +
              transcript,
          },
          ...images,
        ],
      },
      parent_tool_use_id: null,
      session_id: "",
    };
  }
  const stream = queryImpl({
    prompt: prompt(),
    options: {
      pathToClaudeCodeExecutable: executable,
      cwd,
      env: subscriptionEnvironment(),
      model: request.model,
      systemPrompt: request.instructions,
      effort: ["low", "medium", "high", "xhigh", "max"].includes(
        request.reasoning?.effort,
      )
        ? request.reasoning.effort
        : undefined,
      thinking: thinkingOption(),
      tools: [],
      mcpServers: { ukis: server },
      strictMcpConfig: true,
      settingSources: [],
      persistSession: false,
      includePartialMessages: true,
      // No built-in file/shell tools; declarations request host execution.
      canUseTool: async () => ({
        behavior: "deny",
        message: "Ukis Code executes tools after its own approval checks.",
      }),
      abortController: controller,
    },
  });
  const announce = (block) => {
    block.outputIndex = output.length;
    output.push(block.item);
    send("response.output_item.added", {
      output_index: block.outputIndex,
      item: block.item,
    });
  };
  send("response.created", { response: { id, status: "in_progress" } });
  try {
    for await (const message of stream) {
      if (message.type === "stream_event") {
        const event = message.event;
        if (event.type === "message_start") usage = { ...event.message.usage };
        if (event.type === "content_block_start") {
          const block = {
            ...event.content_block,
            json: "",
            outputIndex: output.length,
            item: null,
          };
          if (block.type === "text")
            block.item = {
              type: "message",
              id: `msg_${randomUUID()}`,
              role: "assistant",
              content: [],
            };
          if (block.type === "tool_use") {
            const tool = tools.get(block.name);
            if (!tool)
              throw new Error(
                `Claude requested an unregistered tool: ${block.name}`,
              );
            block.tool = tool;
            block.item = {
              type:
                tool.spec.type === "custom"
                  ? "custom_tool_call"
                  : "function_call",
              id: block.id,
              call_id: block.id,
              name: tool.spec.name,
              ...(tool.namespace ? { namespace: tool.namespace } : {}),
              ...(tool.spec.type === "custom"
                ? { input: "" }
                : { arguments: "" }),
            };
            toolTurn = true;
          }
          // Announced on its first summary text instead: with display
          // "omitted" the block stays empty and would add a blank item.
          if (block.type === "thinking")
            block.reasoning = {
              type: "reasoning",
              id: `${UKIS_REASONING_PREFIX}${randomUUID()}`,
              summary: [],
              encrypted_content: null,
            };
          if (block.item) announce(block);
          blocks.set(event.index, block);
        }
        if (event.type === "content_block_delta") {
          const block = blocks.get(event.index);
          if (event.delta.type === "text_delta" && block?.type === "text") {
            block.text += event.delta.text;
            send("response.output_text.delta", {
              item_id: block.item.id,
              output_index: block.outputIndex,
              content_index: 0,
              delta: event.delta.text,
            });
          }
          if (
            event.delta.type === "input_json_delta" &&
            block?.type === "tool_use"
          )
            block.json += event.delta.partial_json;
          if (
            event.delta.type === "thinking_delta" &&
            block?.type === "thinking" &&
            event.delta.thinking
          ) {
            // Signatures are Anthropic-only replay state and are not kept.
            if (!block.item) {
              block.item = block.reasoning;
              announce(block);
              send("response.reasoning_summary_part.added", {
                item_id: block.item.id,
                output_index: block.outputIndex,
                summary_index: 0,
                part: { type: "summary_text", text: "" },
              });
            }
            block.thinking += event.delta.thinking;
            send("response.reasoning_summary_text.delta", {
              item_id: block.item.id,
              output_index: block.outputIndex,
              summary_index: 0,
              delta: event.delta.thinking,
            });
          }
        }
        if (event.type === "content_block_stop") {
          const block = blocks.get(event.index);
          if (block?.type === "text")
            block.item.content = [{ type: "output_text", text: block.text }];
          if (block?.type === "tool_use") {
            const input = block.json ? JSON.parse(block.json) : block.input;
            if (block.tool.spec.type === "custom") {
              if (typeof input?.input !== "string")
                throw new Error("Claude returned invalid freeform tool input.");
              block.item.input = input.input;
            } else block.item.arguments = JSON.stringify(input);
          }
          if (block?.type === "thinking" && block.item) {
            const text = block.thinking;
            block.item.summary = [{ type: "summary_text", text }];
            const part = {
              item_id: block.item.id,
              output_index: block.outputIndex,
              summary_index: 0,
            };
            send("response.reasoning_summary_text.done", { ...part, text });
            send("response.reasoning_summary_part.done", {
              ...part,
              part: { type: "summary_text", text },
            });
          }
          if (block?.item)
            send("response.output_item.done", {
              output_index: block.outputIndex,
              item: block.item,
            });
        }
        if (event.type === "message_delta") {
          usage = { ...usage, ...event.usage };
          if (event.delta.stop_reason === "max_tokens")
            throw new Error(
              "Claude reached its output token limit. Retry with a smaller task.",
            );
        }
        if (event.type === "message_stop" && toolTurn) {
          // All parallel tool declarations are complete. Close before another
          // model turn; Claude Code has no authority to execute these tools.
          completed = true;
          break;
        }
      }
      if (message.type === "result") {
        if (message.is_error)
          throw new Error(
            message.errors?.join("; ") ||
              message.result ||
              "Claude turn failed.",
          );
        usage = { ...usage, ...message.usage };
        completed = true;
        break;
      }
    }
    if (!completed || signal.aborted)
      throw new Error("Claude response was interrupted.");
    const inputTokens =
      (usage.input_tokens ?? 0) +
      (usage.cache_read_input_tokens ?? 0) +
      (usage.cache_creation_input_tokens ?? 0);
    send("response.completed", {
      response: {
        id,
        status: "completed",
        output,
        end_turn: !toolTurn,
        usage: {
          input_tokens: inputTokens,
          output_tokens: usage.output_tokens ?? 0,
          total_tokens: inputTokens + (usage.output_tokens ?? 0),
          input_tokens_details: {
            cached_tokens: usage.cache_read_input_tokens ?? 0,
          },
        },
      },
    });
  } finally {
    stream.close();
    controller.abort();
    signal.removeEventListener("abort", abort);
    await server.instance.close();
  }
}
