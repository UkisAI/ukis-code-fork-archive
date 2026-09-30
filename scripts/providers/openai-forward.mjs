import { isClaudeModel } from "./catalog.mjs";
// Codex owns OpenAI authentication and refresh. Forward its request only to
// the official endpoint for that authentication mode, never to Claude.
const HOP_HEADERS = new Set([
  "host",
  "connection",
  "content-length",
  "transfer-encoding",
  "upgrade",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "x-ukis-session",
]);
export async function forwardOpenAI({
  req,
  res,
  body,
  signal,
  fetchImpl = fetch,
}) {
  const headers = new Headers();
  for (const [name, value] of Object.entries(req.headers)) {
    if (!HOP_HEADERS.has(name) && typeof value === "string")
      headers.set(name, value);
  }
  headers.set("accept-encoding", "identity");
  const base = headers.has("chatgpt-account-id")
    ? "https://chatgpt.com/backend-api/codex"
    : "https://api.openai.com/v1";
  const upstream = await fetchImpl(base + "/responses", {
    method: "POST",
    headers,
    body,
    signal,
    redirect: "error",
  });
  const responseHeaders = {};
  for (const [name, value] of upstream.headers) {
    if (!HOP_HEADERS.has(name) && name !== "content-encoding")
      responseHeaders[name] = value;
  }
  res.writeHead(upstream.status, responseHeaders);
  if (upstream.body)
    for await (const chunk of upstream.body) {
      if (!res.write(chunk))
        await new Promise((resolve) => {
          const finish = () => {
            res.off("drain", finish);
            res.off("close", finish);
            resolve();
          };
          res.once("drain", finish);
          res.once("close", finish);
        });
      if (signal.aborted) break;
    }
}

export async function routeModel(
  request,
  context,
  { claudeOnly = false, forward = forwardOpenAI } = {},
) {
  if (isClaudeModel(request.model)) {
    request.tools = request.tools?.filter((tool) => tool.type !== "web_search");
    return false;
  }
  if (claudeOnly)
    throw new Error("Launch ukis to select both OpenAI and Claude models.");
  await forward(context);
  return true;
}
