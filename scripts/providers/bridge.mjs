import http from "node:http";
import { randomBytes } from "node:crypto";
import { runClaudeTurn } from "./claude-turn.mjs";

export async function startBridge({
  executable,
  cwd,
  runTurn = runClaudeTurn,
}) {
  const token = randomBytes(32).toString("hex");
  const active = new Set();
  const server = http.createServer(async (req, res) => {
    if (req.headers.authorization !== `Bearer ${token}` || req.headers.origin) {
      res.writeHead(403).end("Forbidden");
      return;
    }
    if (req.method !== "POST" || req.url?.split("?")[0] !== "/v1/responses") {
      res.writeHead(404).end("Not found");
      return;
    }
    const controller = new AbortController();
    active.add(controller);
    res.on("close", () => controller.abort());
    const timeout = setTimeout(() => controller.abort(), 10 * 60_000);
    let ping;
    const emit = (event) => {
      if (!res.destroyed)
        res.write(`event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
    };
    try {
      let size = 0;
      const chunks = [];
      for await (const chunk of req) {
        size += chunk.length;
        if (size > 16 * 1024 * 1024) {
          res
            .writeHead(413)
            .end("Conversation exceeds the 16 MiB bridge limit.");
          return;
        }
        chunks.push(chunk);
      }
      const request = JSON.parse(Buffer.concat(chunks).toString("utf8"));
      if (!request.stream || request.previous_response_id)
        throw new Error("Claude requires streamed requests with full history.");
      res.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
      });
      res.flushHeaders();
      ping = setInterval(() => {
        if (!res.destroyed) res.write(": keepalive\n\n");
      }, 15_000);
      await runTurn(request, {
        signal: controller.signal,
        emit,
        executable,
        cwd,
      });
    } catch (error) {
      if (!res.headersSent)
        res.writeHead(400, { "content-type": "application/json" });
      if (res.getHeader("content-type") === "text/event-stream") {
        emit({
          type: "response.failed",
          response: {
            error: { code: "claude_provider_error", message: error.message },
          },
        });
      } else if (!res.destroyed)
        res.write(JSON.stringify({ error: { message: error.message } }));
    } finally {
      clearInterval(ping);
      clearTimeout(timeout);
      active.delete(controller);
      res.end();
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  return {
    url: `http://127.0.0.1:${server.address().port}/v1`,
    token,
    async close() {
      for (const controller of active) controller.abort();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}
