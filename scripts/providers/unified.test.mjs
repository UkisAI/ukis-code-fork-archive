import { shouldOpenUkisWindow } from "../ukis-window.mjs";
import { readFile } from "node:fs/promises";
import { discoverOpenAIModels } from "./openai-catalog.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { claudeCatalog, isClaudeModel } from "./catalog.mjs";
import { forwardOpenAI, routeModel } from "./openai-forward.mjs";
import { startBridge } from "./bridge.mjs";

test("SDK model discovery supplies Fable and its real effort choices to the Codex picker", () => {
  const rows = [
    { value: "default", displayName: "Default" },
    {
      value: "claude-fable-5-1",
      displayName: "Fable 5.1",
      description: "Hard tasks",
      supportsEffort: true,
      supportedEffortLevels: ["low", "medium", "high", "xhigh", "max"],
    },
    { value: "haiku", displayName: "Haiku", description: "Quick tasks" },
  ];
  const models = claudeCatalog(rows, "Instructions");
  assert.deepEqual(
    models.map((m) => ({
      slug: m.slug,
      name: m.display_name,
      default: m.default_reasoning_level,
      efforts: m.supported_reasoning_levels.map((e) => e.effort),
    })),
    [
      {
        slug: "claude-fable-5-1",
        name: "Claude Fable 5.1",
        default: "medium",
        efforts: ["low", "medium", "high", "xhigh", "max"],
      },
      { slug: "haiku", name: "Claude Haiku", default: null, efforts: [] },
    ],
  );
  for (const name of ["sonnet", "opus[1m]", "fable", "claude-fable-5-1"])
    assert.equal(isClaudeModel(name), true);
  assert.equal(isClaudeModel("gpt-6-astra"), false);
});

test("one local endpoint routes successive OpenAI and Claude models and preserves effort", async (t) => {
  const calls = [];
  const bridge = await startBridge({
    sessionHeader: "x-ukis-session",
    route: (body, context) =>
      routeModel(body, context, {
        forward: async ({ req, res, body }) => {
          calls.push({
            provider: "openai",
            request: JSON.parse(body),
            authorization: req.headers.authorization,
          });
          res.writeHead(200, { "content-type": "text/event-stream" });
          res.write('data: {"type":"response.completed"}\n\n');
        },
      }),
    runTurn: async (request, context) => {
      assert.equal(context.req, undefined); // OpenAI auth is not handed to Claude.
      calls.push({ provider: "claude", request });
      context.emit({ type: "response.completed" });
    },
  });
  t.after(() => bridge.close());
  for (const model of ["gpt-6-astra", "claude-fable-5-1", "gpt-6-astra"]) {
    const response = await fetch(bridge.url + "/responses", {
      method: "POST",
      headers: {
        "x-ukis-session": bridge.token,
        "authorization": "Bearer openai-only",
      },
      body: JSON.stringify({
        model,
        stream: true,
        input: [],
        reasoning: { effort: "xhigh" },
        tools: [{ type: "web_search" }, { type: "function", name: "read" }],
      }),
    });
    assert.equal(response.status, 200);
    await response.text();
  }
  assert.deepEqual(
    calls.map((c) => c.provider),
    ["openai", "claude", "openai"],
  );
  assert.deepEqual(calls[1].request, {
    model: "claude-fable-5-1",
    stream: true,
    input: [],
    reasoning: { effort: "xhigh" },
    tools: [{ type: "function", name: "read" }],
  });
  assert.equal(calls[0].authorization, "Bearer openai-only");
  assert.equal(calls[0].request.tools[0].type, "web_search");
});

test("Claude reasoning items are dropped before an OpenAI turn, other bodies pass through unchanged", async () => {
  const forwarded = [];
  const forward = async ({ body }) => forwarded.push(body);
  const message = {
    type: "message",
    role: "user",
    content: [{ type: "input_text", text: "hi" }],
  };
  const openaiReasoning = {
    type: "reasoning",
    id: "rs_0a1b",
    summary: [],
    encrypted_content: "opaque",
  };
  const request = {
    model: "gpt-test",
    input: [
      message,
      {
        type: "reasoning",
        id: "rs_ukis_1",
        summary: [{ type: "summary_text", text: "Claude thought." }],
        encrypted_content: null,
      },
      openaiReasoning,
    ],
  };
  const body = Buffer.from(JSON.stringify(request));
  assert.equal(await routeModel(request, { body }, { forward }), true);
  assert.deepEqual(JSON.parse(forwarded[0]), {
    model: "gpt-test",
    input: [message, openaiReasoning],
  });
  const clean = { model: "gpt-test", input: [message] };
  const cleanBody = Buffer.from(JSON.stringify(clean));
  await routeModel(clean, { body: cleanBody }, { forward });
  assert.equal(forwarded[1], cleanBody);
});

test("OpenAI forwarding keeps native auth, status and streaming, without leaking the local token", async () => {
  for (const account of [undefined, "account-test"]) {
    const output = [];
    const res = new EventEmitter();
    res.writeHead = (status, headers) => output.push({ status, headers });
    res.write = (chunk) => {
      output.push(Buffer.from(chunk).toString());
      return true;
    };
    const headers = {
      "authorization": "Bearer native-auth",
      "host": "127.0.0.1",
      "x-ukis-session": "local-only",
      ...(account ? { "chatgpt-account-id": account } : {}),
    };
    await forwardOpenAI({
      req: { headers },
      res,
      body: Buffer.from('{"model":"gpt-test"}'),
      signal: new AbortController().signal,
      fetchImpl: async (url, options) => {
        assert.equal(
          url,
          account
            ? "https://chatgpt.com/backend-api/codex/responses"
            : "https://api.openai.com/v1/responses",
        );
        assert.equal(
          options.headers.get("authorization"),
          "Bearer native-auth",
        );
        assert.equal(options.headers.has("x-ukis-session"), false);
        assert.equal(options.headers.has("host"), false);
        assert.equal(options.redirect, "error");
        assert.equal(options.body.toString(), '{"model":"gpt-test"}');
        return new Response('data: {"type":"response.completed"}\n\n', {
          status: 200,
          headers: { "content-type": "text/event-stream" },
        });
      },
    });
    assert.equal(output[0].status, 200);
    assert.match(output.slice(1).join(""), /response.completed/);
  }
});

test("OpenAI discovery uses the native catalog and preserves model capabilities and effort metadata", async () => {
  const catalog = {
    models: [
      {
        slug: "gpt-6.1-sol",
        visibility: "list",
        supported_reasoning_levels: [
          { effort: "max", description: "Maximum reasoning" },
        ],
        model_messages: {
          instructions_template: "Model-specific instructions",
        },
        context_window: 1050000,
        future_capability: { enabled: true },
      },
    ],
  };
  const warnings = [];
  const actual = await discoverOpenAIModels("codex-test", "unused-bundle", {
    run: async (executable, args, options) => {
      assert.equal(executable, "codex-test");
      assert.deepEqual(args, [
        "debug",
        "models",
        "-c",
        'model_provider="openai"',
      ]);
      assert.ok(options.timeout > 0);
      assert.equal(options.windowsHide, true);
      return { stdout: JSON.stringify(catalog) };
    },
    warn: (message) => warnings.push(message),
  });
  assert.deepEqual(actual, catalog);
  assert.deepEqual(warnings, []);
});

test("unavailable or malformed native discovery falls back to the build's complete catalog", async () => {
  const bundledPath = new URL(
    "../../codex-rs/models-manager/models.json",
    import.meta.url,
  );
  const expected = JSON.parse(await readFile(bundledPath, "utf8"));
  for (const output of [null, "invalid-json", '{"models":[]}']) {
    const warnings = [];
    const actual = await discoverOpenAIModels("codex-test", bundledPath, {
      run: async () => {
        if (output === null) throw new Error("Timed out");
        return { stdout: output };
      },
      warn: (message) => warnings.push(message),
    });
    assert.deepEqual(actual, expected);
    assert.equal(warnings.length, 1);
  }
});

test("plain interactive ukis opens the font profile once while CLI and piped commands stay in place", () => {
  const environment = {
    platform: "win32",
    interactive: true,
    installed: true,
    env: {},
  };
  assert.equal(shouldOpenUkisWindow([], environment), true);
  for (const args of [
    ["exec", "hello"],
    ["app-server"],
    ["--help"],
    ["--version"],
    ["-m", "sonnet"],
    ["resume"],
  ]) {
    assert.equal(shouldOpenUkisWindow(args, environment), false);
  }
  for (const overrides of [
    { interactive: false },
    { installed: false },
    { platform: "linux" },
    {
      env: { UKIS_TERMINAL_PROFILE: "{4fc3ef90-34ce-5ce0-adf3-7d124d958fb8}" },
    },
  ]) {
    assert.equal(
      shouldOpenUkisWindow([], { ...environment, ...overrides }),
      false,
    );
  }
});
