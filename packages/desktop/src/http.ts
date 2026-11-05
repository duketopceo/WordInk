import { request } from "node:http";
import { ApiError, NotRunningError, UnauthorizedError, WordInkDesktopError } from "./errors.js";

export interface Reply {
  status: number;
  body: Buffer;
}

export interface Call {
  method: "GET" | "POST";
  path: string;
  headers?: Record<string, string>;
  body?: Uint8Array;
}

/** One authenticated request; maps connection failures and 401 to typed errors. */
export function send(base: string, token: string, call: Call, timeoutMs: number): Promise<Reply> {
  return new Promise((resolve, reject) => {
    const headers: Record<string, string | number> = {
      Authorization: `Bearer ${token}`,
      Connection: "close",
      ...call.headers,
    };
    if (call.body) headers["Content-Length"] = call.body.byteLength;
    const req = request(new URL(call.path, base), { method: call.method, headers, timeout: timeoutMs }, (res) => {
      const chunks: Buffer[] = [];
      res.on("data", (chunk: Buffer) => chunks.push(chunk));
      res.on("error", reject);
      res.on("end", () => {
        const reply = { status: res.statusCode ?? 0, body: Buffer.concat(chunks) };
        if (reply.status === 401) {
          reject(new UnauthorizedError("WordInk refused the token; reconnect to read the current one"));
        } else {
          resolve(reply);
        }
      });
    });
    req.on("timeout", () => req.destroy(new WordInkDesktopError(`WordInk did not answer within ${timeoutMs} ms`)));
    req.on("error", (error: NodeJS.ErrnoException) => {
      if (error instanceof WordInkDesktopError) reject(error);
      else if (error.code === "ECONNREFUSED" || error.code === "ECONNRESET") {
        reject(new NotRunningError("WordInk is not running (its local API did not answer)", { cause: error }));
      } else reject(error);
    });
    req.end(call.body);
  });
}

/** The JSON error code of a non-2xx reply, if it has one. */
export function errorOf(reply: Reply): ApiError {
  let code: string | undefined;
  try {
    const parsed = JSON.parse(reply.body.toString("utf8")) as { code?: unknown };
    if (typeof parsed.code === "string") code = parsed.code;
  } catch {
    // An empty or non-JSON body has no code.
  }
  return new ApiError(reply.status, code);
}

export function json<T>(reply: Reply): T {
  if (reply.status < 200 || reply.status >= 300) throw errorOf(reply);
  return JSON.parse(reply.body.toString("utf8")) as T;
}
