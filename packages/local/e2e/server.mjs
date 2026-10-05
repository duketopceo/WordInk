// Static server for the @wordink/local e2e page, on 127.0.0.1:
//   /e2e/, /test/fixtures/  files from packages/local
//   /ort/                   onnxruntime-web runtime files (self-hosted, so no CDN in the test)
//   /hf/<path>              read-through proxy to https://huggingface.co/<path>, cached on disk under
//                           $WORDINK_MODEL_CACHE (default ~/.cache/wordink-models) so reruns skip the download
import { createServer } from "node:http";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { realpathSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const port = Number(process.argv[2] ?? 4318);
const modelCache = process.env.WORDINK_MODEL_CACHE ?? join(homedir(), ".cache", "wordink-models");
// onnxruntime-web is a dependency of transformers.js; pnpm puts it next to it.
const transformers = realpathSync(join(root, "node_modules", "@huggingface", "transformers"));
const ortDist = join(transformers, "..", "..", "onnxruntime-web", "dist");

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json",
  ".wav": "audio/wav",
};

async function fromHub(path) {
  const file = join(modelCache, path);
  try {
    return await readFile(file);
  } catch {
    const res = await fetch(`https://huggingface.co/${path}`);
    if (!res.ok) throw Object.assign(new Error(`hub ${res.status}`), { status: res.status });
    const body = Buffer.from(await res.arrayBuffer());
    await mkdir(dirname(file), { recursive: true });
    await writeFile(`${file}.part`, body);
    await rename(`${file}.part`, file);
    return body;
  }
}

createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url ?? "/", "http://x").pathname));
  if (path.includes("..")) {
    res.writeHead(400).end();
    return;
  }
  try {
    let body;
    if (path.startsWith("/hf/")) body = await fromHub(path.slice(4));
    else if (path.startsWith("/ort/")) body = await readFile(join(ortDist, path.slice(5)));
    else if (path.startsWith("/e2e/") || path.startsWith("/test/fixtures/")) body = await readFile(join(root, path));
    else throw Object.assign(new Error("not found"), { status: 404 });
    res
      .writeHead(200, {
        "content-type": TYPES[extname(path)] ?? "application/octet-stream",
        "content-length": body.length,
        "cache-control": "no-store",
      })
      .end(body);
  } catch (err) {
    res.writeHead(err.status ?? 404).end("not found");
  }
}).listen(port, "127.0.0.1");
