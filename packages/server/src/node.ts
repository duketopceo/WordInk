import type { IncomingMessage, ServerResponse } from "node:http";
import { Readable } from "node:stream";
import type { TLSSocket } from "node:tls";
import { createRelay, type ProviderKeys, type RelayConfig } from "./index.js";

/** Relay config for Node: `keys` default to GROQ_API_KEY / OPENAI_API_KEY / DEEPGRAM_API_KEY from process.env. */
export type NodeRelayConfig = Omit<RelayConfig, "keys"> & { keys?: ProviderKeys };

export interface NodeAdapterOptions {
  /**
   * Trust proxy headers sent to this process: client IP (X-Forwarded-For, CF-Connecting-IP) and the
   * public scheme and host (X-Forwarded-Proto, X-Forwarded-Host), which become the relay's own
   * origin for the `Origin` check. Only enable behind a proxy that overwrites them. Default false:
   * the IP headers are replaced with the socket address, so callers can't pick their own rate-limit
   * bucket, and the origin comes from the socket (TLS or not) and the `Host` header.
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
  const url = requestUrl(req, trustProxy);
  const method = req.method ?? "GET";
  const hasBody = method !== "GET" && method !== "HEAD";
  return new Request(url, {
    method,
    headers,
    ...(hasBody ? { body: Readable.toWeb(req) as ReadableStream<Uint8Array>, duplex: "half" } : {}),
  } as RequestInit);
}

/**
 * The relay's view of the request URL: origin from the socket (or trusted proxy headers) plus the
 * request-target as a path. The target is never resolved as a URL reference, so `//evil.com/x`
 * stays a path on this origin instead of becoming `http://evil.com/x`, whose origin would then
 * match an attacker's `Origin` header.
 */
function requestUrl(req: IncomingMessage, trustProxy: boolean): URL {
  let scheme = (req.socket as TLSSocket).encrypted ? "https" : "http";
  let host = req.headers.host;
  if (trustProxy) {
    const proto = firstHeaderValue(req.headers["x-forwarded-proto"])?.toLowerCase();
    if (proto === "http" || proto === "https") scheme = proto;
    host = firstHeaderValue(req.headers["x-forwarded-host"]) ?? host;
  }
  let origin = `${scheme}://localhost`;
  try {
    if (host) origin = new URL(`${scheme}://${host}`).origin;
  } catch {
    // Unparseable Host: keep the fallback, which no browser Origin will match.
  }
  const path = req.url?.startsWith("/") ? req.url : "/";
  return new URL(origin + path);
}

function firstHeaderValue(value: string | string[] | undefined): string | undefined {
  const first = Array.isArray(value) ? value[0] : value;
  return first?.split(",")[0]?.trim() || undefined;
}
