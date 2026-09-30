import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import {
  runClaudeTurn,
  prepareTools,
  subscriptionEnvironment,
  thinkingOption,
} from "./claude-turn.mjs";
import { startBridge } from "./bridge.mjs";
import { providerOptions } from "../provider-options.mjs";
import { recordingEnvironment, recordingWarning } from "../recording.mjs";

const functionTool = {
  type: "function",
  name: "read",
  parameters: {
    type: "object",
    properties: { path: { type: "string" } },
    required: ["path"],
  },
};
const customTool = {
  type: "custom",
  name: "apply_patch",
  description: "Apply a patch.",
};
const request = {
  model: "sonnet",
  reasoning: { effort: "high" },
  instructions: "Be helpful.",
  input: [
    {
      type: "message",
      role: "user",
      content: [{ type: "input_text", text: "Read and fix." }],
    },
  ],
  tools: [
    { type: "namespace", name: "files", tools: [functionTool, customTool] },
  ],
};
function fakeQuery(events, inspect = () => {}) {
  return (options) => {
    inspect(options);
    const stream = (async function* () {
      for (const event of events) yield event;
    })();
    stream.close = () => {};
    return stream;
  };
}
const event = (type, data = {}) => ({
  type: "stream_event",
  event: { type, ...data },
});

test("streams text and parallel namespaced function/freeform calls back to Codex", async () => {
  const names = [...prepareTools(request.tools).keys()];
  const events = [
    event("message_start", {
      message: { usage: { input_tokens: 10, cache_read_input_tokens: 4 } },
    }),
    event("content_block_start", {
      index: 0,
      content_block: { type: "text", text: "" },
    }),
    event("content_block_delta", {
      index: 0,
      delta: { type: "text_delta", text: "Checking." },
    }),
    event("content_block_stop", { index: 0 }),
    ...names.flatMap((name, i) => [
      event("content_block_start", {
        index: i + 1,
        content_block: { type: "tool_use", id: "call" + i, name, input: {} },
      }),
      event("content_block_delta", {
        index: i + 1,
        delta: {
          type: "input_json_delta",
          partial_json: i === 0 ? '{"path":' : '{"input":"*** Begin Patch',
        },
      }),
      event("content_block_delta", {
        index: i + 1,
        delta: {
          type: "input_json_delta",
          partial_json: i === 0 ? '"a.txt"}' : '\\n*** End Patch"}',
        },
      }),
      event("content_block_stop", { index: i + 1 }),
    ]),
    event("message_delta", {
      delta: { stop_reason: "tool_use" },
      usage: { output_tokens: 7 },
    }),
    event("message_stop"),
  ];
  const emitted = [];
  await runClaudeTurn(request, {
    signal: new AbortController().signal,
    emit: (e) => emitted.push(structuredClone(e)),
    queryImpl: fakeQuery(events, ({ options }) => {
      assert.deepEqual(options.tools, []);
      assert.deepEqual(options.settingSources, []);
      assert.equal(options.persistSession, false);
      assert.equal(options.effort, "high");
    }),
  });
  assert.equal(
    emitted.filter((e) => e.type === "response.output_text.delta")[0].delta,
    "Checking.",
  );
  const response = emitted.at(-1).response;
  assert.deepEqual(response.output.slice(1), [
    {
      type: "function_call",
      id: "call0",
      call_id: "call0",
      name: "read",
      namespace: "files",
      arguments: '{"path":"a.txt"}',
    },
    {
      type: "custom_tool_call",
      id: "call1",
      call_id: "call1",
      name: "apply_patch",
      namespace: "files",
      input: "*** Begin Patch\n*** End Patch",
    },
  ]);
  assert.deepEqual(response.usage, {
    input_tokens: 14,
    output_tokens: 7,
    total_tokens: 21,
    input_tokens_details: { cached_tokens: 4 },
  });
  assert.equal(response.end_turn, false);
});

test("Claude thinking streams as a Codex reasoning item with its summary", async () => {
  const thinking = (index, deltas) => [
    event("content_block_start", {
      index,
      content_block: { type: "thinking", thinking: "", signature: "" },
    }),
    ...deltas.map((text) =>
      event("content_block_delta", {
        index,
        delta: { type: "thinking_delta", thinking: text },
      }),
    ),
    event("content_block_delta", {
      index,
      delta: { type: "signature_delta", signature: "sig" },
    }),
    event("content_block_stop", { index }),
  ];
  const events = [
    event("message_start", { message: { usage: { input_tokens: 3 } } }),
    // Display "omitted" sends an empty block; it must not become an item.
    ...thinking(0, []),
    ...thinking(1, ["Read the ", "file first."]),
    event("content_block_start", {
      index: 2,
      content_block: { type: "text", text: "" },
    }),
    event("content_block_delta", {
      index: 2,
      delta: { type: "text_delta", text: "Done." },
    }),
    event("content_block_stop", { index: 2 }),
    event("message_delta", {
      delta: { stop_reason: "end_turn" },
      usage: { output_tokens: 5 },
    }),
    event("message_stop"),
    { type: "result", is_error: false, usage: {} },
  ];
  const emitted = [];
  await runClaudeTurn(request, {
    signal: new AbortController().signal,
    emit: (e) => emitted.push(structuredClone(e)),
    queryImpl: fakeQuery(events, ({ options }) => {
      assert.deepEqual(options.thinking, thinkingOption());
    }),
  });
  const output = emitted.at(-1).response.output;
  assert.match(output[0].id, /^rs_ukis_/);
  const id = output[0].id;
  assert.deepEqual(output, [
    {
      type: "reasoning",
      id,
      summary: [{ type: "summary_text", text: "Read the file first." }],
      encrypted_content: null,
    },
    {
      type: "message",
      id: output[1].id,
      role: "assistant",
      content: [{ type: "output_text", text: "Done." }],
    },
  ]);
  // Codex needs an active item before any summary delta arrives.
  const reasoning = emitted
    .filter((e) => e.item?.id === id || e.item_id === id)
    .map(({ type, delta, text, summary_index }) => ({
      type,
      ...(delta ? { delta } : {}),
      ...(text ? { text } : {}),
      ...(summary_index === undefined ? {} : { summary_index }),
    }));
  assert.deepEqual(reasoning, [
    { type: "response.output_item.added" },
    { type: "response.reasoning_summary_part.added", summary_index: 0 },
    {
      type: "response.reasoning_summary_text.delta",
      delta: "Read the ",
      summary_index: 0,
    },
    {
      type: "response.reasoning_summary_text.delta",
      delta: "file first.",
      summary_index: 0,
    },
    {
      type: "response.reasoning_summary_text.done",
      text: "Read the file first.",
      summary_index: 0,
    },
    { type: "response.reasoning_summary_part.done", summary_index: 0 },
    { type: "response.output_item.done" },
  ]);
  assert.deepEqual(
    emitted
      .filter((e) => e.type === "response.output_item.added")
      .map((e) => e.output_index),
    [0, 1],
  );
});

test("Claude thinking display is summarized by default and configurable", () => {
  assert.deepEqual(thinkingOption({}), {
    type: "adaptive",
    display: "summarized",
  });
  assert.deepEqual(
    thinkingOption({ UKIS_CLAUDE_THINKING_DISPLAY: "omitted" }),
    { type: "adaptive", display: "omitted" },
  );
  assert.equal(
    thinkingOption({ UKIS_CLAUDE_THINKING_DISPLAY: "default" }),
    undefined,
  );
  assert.throws(
    () => thinkingOption({ UKIS_CLAUDE_THINKING_DISPLAY: "raw" }),
    /summarized, omitted or default/,
  );
});

test("Claude history drops OpenAI and Ukis reasoning items", async () => {
  const input = [
    ...request.input,
    {
      type: "reasoning",
      id: "rs_ukis_1",
      summary: [],
      encrypted_content: null,
    },
    { type: "reasoning", id: "rs_openai", summary: [], encrypted_content: "x" },
  ];
  let text;
  await runClaudeTurn(
    { ...request, input },
    {
      signal: new AbortController().signal,
      emit: () => {},
      queryImpl: ({ prompt }) => {
        const stream = (async function* () {
          for await (const message of prompt)
            text = message.message.content[0].text;
          yield { type: "result", is_error: false, usage: {} };
        })();
        stream.close = () => {};
        return stream;
      },
    },
  );
  assert.deepEqual(JSON.parse(text.slice(text.indexOf("\n") + 1)), [input[0]]);
});

test("preserves tool results and forwards image attachments without embedding base64 in transcript", async () => {
  const input = [
    ...request.input,
    { type: "function_call_output", call_id: "c1", output: "FILE CONTENT" },
    {
      type: "message",
      role: "user",
      content: [
        { type: "input_image", image_url: "data:image/png;base64,aGVsbG8=" },
      ],
    },
  ];
  let receivedPrompt;
  await runClaudeTurn(
    { ...request, input },
    {
      signal: new AbortController().signal,
      emit: () => {},
      queryImpl: ({ prompt }) => {
        const stream = (async function* () {
          for await (const message of prompt)
            receivedPrompt = message.message.content;
          yield { type: "result", is_error: false, usage: {} };
        })();
        stream.close = () => {};
        return stream;
      },
    },
  );
  const history = JSON.parse(
    receivedPrompt[0].text.slice(receivedPrompt[0].text.indexOf("\n") + 1),
  );
  assert.deepEqual(history[1], input[1]);
  assert.deepEqual(history[2].content, [
    { type: "image_reference", image_number: 1 },
  ]);
  assert.deepEqual(receivedPrompt[1], {
    type: "image",
    source: { type: "base64", media_type: "image/png", data: "aGVsbG8=" },
  });
});

test("reports interrupted and truncated turns without completing them", async () => {
  for (const events of [
    [],
    [event("message_delta", { delta: { stop_reason: "max_tokens" } })],
  ]) {
    const emitted = [];
    await assert.rejects(
      runClaudeTurn(request, {
        signal: new AbortController().signal,
        emit: (e) => emitted.push(e),
        queryImpl: fakeQuery(events),
      }),
    );
    assert.ok(!emitted.some((e) => e.type === "response.completed"));
  }
});

test("does not silently substitute API/cloud credentials for a subscription", () => {
  assert.deepEqual(
    subscriptionEnvironment({
      PATH: "/bin",
      ANTHROPIC_API_KEY: "secret",
      ANTHROPIC_AUTH_TOKEN: "secret",
      ANTHROPIC_BASE_URL: "https://other",
      CLAUDE_CODE_USE_BEDROCK: "1",
    }),
    { PATH: "/bin" },
  );
});

test("rejects unsupported hosted tools explicitly", () => {
  assert.throws(() => prepareTools([{ type: "web_search" }]), /hosted tool/);
});

test("launcher preserves Codex arguments and handles provider aliases", () => {
  assert.deepEqual(providerOptions(["claude", "exec", "-m", "opus", "hello"]), {
    provider: "claude",
    args: ["exec", "-m", "opus", "hello"],
  });
  assert.deepEqual(
    providerOptions(["--provider=anthropic", "--", "--provider"]),
    { provider: "claude", args: ["--", "--provider"] },
  );
  assert.deepEqual(providerOptions(["--provider", "openai", "resume"]), {
    provider: "openai",
    args: ["resume"],
  });
  assert.throws(() => providerOptions(["--provider"]), /Use --provider/);
  assert.throws(
    () => providerOptions(["claude", "--provider", "openai"]),
    /one provider/,
  );
});

test("UKIS_RECORD=1 points Codex rollout traces at ~/.ukis/traces unless a root is set", () => {
  const home = path.join(path.sep, "home", "ukis");
  const traces = path.join(home, ".ukis", "traces");
  const base = { PATH: "/bin" };
  assert.equal(recordingEnvironment(base, home), base);
  assert.equal(
    recordingEnvironment({ ...base, UKIS_RECORD: "0" }, home)
      .CODEX_ROLLOUT_TRACE_ROOT,
    undefined,
  );
  assert.deepEqual(recordingEnvironment({ ...base, UKIS_RECORD: "1" }, home), {
    ...base,
    UKIS_RECORD: "1",
    CODEX_ROLLOUT_TRACE_ROOT: traces,
  });
  const chosen = { UKIS_RECORD: "1", CODEX_ROLLOUT_TRACE_ROOT: "/data/t" };
  assert.deepEqual(recordingEnvironment(chosen, home), chosen);
  // Empty would make Codex write bundles into the session's working directory.
  assert.equal(
    recordingEnvironment(
      { UKIS_RECORD: "1", CODEX_ROLLOUT_TRACE_ROOT: "" },
      home,
    ).CODEX_ROLLOUT_TRACE_ROOT,
    traces,
  );
  const on = { UKIS_RECORD: "1" };
  assert.match(
    recordingWarning(on, ["exec", "--ephemeral", "hi"]),
    /ephemeral/,
  );
  assert.equal(recordingWarning(on, ["exec", "--", "--ephemeral"]), undefined);
  assert.equal(recordingWarning({}, ["exec", "--ephemeral"]), undefined);
});

test("HTTP boundary requires local session auth, streams events, and aborts disconnected work", async (t) => {
  let cancelled;
  const cancelledPromise = new Promise((resolve) => {
    cancelled = resolve;
  });
  const bridge = await startBridge({
    runTurn: async (body, { signal, emit }) => {
      if (body.cancel) {
        await new Promise((resolve) =>
          signal.addEventListener("abort", resolve, { once: true }),
        );
        cancelled();
      } else emit({ type: "response.completed", response: { id: "test" } });
    },
  });
  t.after(() => bridge.close());
  const url = bridge.url + "/responses";
  const headers = {
    "authorization": "Bearer " + bridge.token,
    "content-type": "application/json",
  };
  assert.equal((await fetch(url, { method: "POST", body: "{}" })).status, 403);
  assert.equal(
    (
      await fetch(url, {
        method: "POST",
        headers: { ...headers, origin: "https://example.com" },
        body: "{}",
      })
    ).status,
    403,
  );
  const response = await fetch(url, {
    method: "POST",
    headers,
    body: JSON.stringify({ stream: true }),
  });
  assert.match(await response.text(), /event: response.completed\ndata:/);
  const failure = await fetch(url, { method: "POST", headers, body: "{" });
  assert.equal(failure.status, 400);
  const controller = new AbortController();
  const pending = await fetch(url, {
    method: "POST",
    headers,
    body: JSON.stringify({ stream: true, cancel: true }),
    signal: controller.signal,
  });
  controller.abort();
  await cancelledPromise;
  assert.equal(pending.status, 200);
});
