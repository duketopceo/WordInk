import type { IncomingMessage, ServerResponse } from "node:http";
import { Readable } from "node:stream";
import { createRelay, type ProviderKeys, type RelayConfig } from "./index.js";

/** Relay config for Node: `keys` default to GROQ_API_KEY / OPENAI_API_KEY / DEEPGRAM_API_KEY from process.env. */
export type NodeRelayConfig = Omit<RelayConfig, "keys"> & { keys?: ProviderKeys };

export interface NodeAdapterOptions {
  /**
   * Trust client-IP headers (X-Forwarded-For, CF-Connecting-IP) sent to this process. Only enable
   * behind a proxy that overwrites them. Default false: those headers are replaced with the socket
   * address, so callers can't pick their own rate-limit bucket.
   */
  trustProxy?: boolean;
}

/** A `node:http` request listener: `http.createServer(createNodeHandler({...}))`. */
export function createNodeHandler(
  config: NodeRelayConfig,
  options: NodeAdapterOptions = {},
): (req: IncomingMessage, res: ServerResponse) => void {
  const relay = createRelay({
    ...config,
    keys: config.keys ?? {
      groq: process.env.GROQ_API_KEY || undefined,
      openai: process.env.OPENAI_API_KEY || undefined,
      deepgram: process.env.DEEPGRAM_API_KEY || undefined,
    },
  });

  return (req, res) => {
    void (async () => {
      const response = await relay(toRequest(req, options.trustProxy === true));
      res.statusCode = response.status;
      response.headers.forEach((value, name) => res.setHeader(name, value));
      res.end(Buffer.from(await response.arrayBuffer()));
    })().catch(() => {
      if (!res.headersSent) res.statusCode = 500;
      res.end();
    });
  };
}

function toRequest(req: IncomingMessage, trustProxy: boolean): Request {
  const headers = new Headers();
  for (const [name, value] of Object.entries(req.headers)) {
    if (value === undefined) continue;
    for (const v of Array.isArray(value) ? value : [value]) headers.append(name, v);
  }
  if (!trustProxy) {
    headers.delete("cf-connecting-ip");
    headers.delete("x-forwarded-for");
    const ip = req.socket.remoteAddress?.replace(/^::ffff:/, "");
    if (ip) headers.set("x-forwarded-for", ip);
  }
  const url = new URL(req.url ?? "/", `http://${req.headers.host ?? "localhost"}`);
  const method = req.method ?? "GET";
  const hasBody = method !== "GET" && method !== "HEAD";
  return new Request(url, {
    method,
    headers,
    ...(hasBody ? { body: Readable.toWeb(req) as ReadableStream<Uint8Array>, duplex: "half" } : {}),
  } as RequestInit);
}
