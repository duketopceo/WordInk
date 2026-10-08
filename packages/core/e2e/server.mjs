// Minimal e2e server on 127.0.0.1: serves packages/core (dist/, wasm/, e2e/) and stands in for the
// Groq relay. Uploads are recorded per run id so the spec reads the real request bytes; Playwright's
// request interception can't expose a multipart body with a file part in every browser (WebKit).
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const port = Number(process.argv[2] ?? 4317);
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json",
};

/** run id -> last upload to /relay/<run>/groq/transcriptions */
const uploads = new Map();

createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", "http://x");
  const relay = url.pathname.match(/^\/relay\/([\w-]+)\/groq\/transcriptions$/);
  if (relay && req.method === "POST") {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    uploads.set(relay[1], { contentType: req.headers["content-type"] ?? "", body: Buffer.concat(chunks).toString("base64") });
    res.writeHead(200, { "content-type": "text/plain" }).end("hello from the relay\n");
    return;
  }
  const upload = url.pathname.match(/^\/__uploads\/([\w-]+)$/);
  if (upload) {
    const found = uploads.get(upload[1]);
    res.writeHead(found ? 200 : 404, { "content-type": "application/json" }).end(JSON.stringify(found ?? null));
    return;
  }
  const path = normalize(decodeURIComponent(new URL(req.url ?? "/", "http://x").pathname));
  if (path.includes("..")) {
    res.writeHead(400).end();
    return;
  }
  try {
    const body = await readFile(join(root, path));
    res.writeHead(200, { "content-type": TYPES[extname(path)] ?? "application/octet-stream" }).end(body);
  } catch {
    res.writeHead(404).end("not found");
  }
}).listen(port, "127.0.0.1");
