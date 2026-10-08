// Tiny static server for the docs e2e: `node e2e/server.mjs <port> <dir> [prefix]` serves <dir> under
// URL <prefix> (default "/") on 127.0.0.1. `/__health` answers 200 so Playwright can wait for it.
import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize, resolve } from "node:path";

const [port, dir, prefix = "/"] = process.argv.slice(2);
const root = resolve(dir);
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json",
};

createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", "http://x");
  if (url.pathname === "/__health") return res.writeHead(200).end("ok");
  if (!url.pathname.startsWith(prefix)) return res.writeHead(404).end("not found");
  const path = normalize(decodeURIComponent(url.pathname.slice(prefix.length)));
  if (path.includes("..")) return res.writeHead(400).end();
  let file = join(root, path);
  try {
    if ((await stat(file)).isDirectory()) file = join(file, "index.html");
    const body = await readFile(file);
    res.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream", "cache-control": "no-store" });
    res.end(body);
  } catch {
    res.writeHead(404).end("not found");
  }
}).listen(Number(port), "127.0.0.1");
