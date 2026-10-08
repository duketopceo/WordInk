// Shared upstream fakes for the gateway tests: a fetch mock that dispatches per provider key
// plus the small call-inspection helpers around it.
import { expect, vi } from "vitest";

export type Responder = (signal: AbortSignal | undefined) => Response | Promise<Response>;

export const ok = (text: string): Responder => () => Response.json({ text });

/** Never answers; rejects with the abort reason like real fetch does. */
export const hang: Responder = (signal) =>
  new Promise((_resolve, reject) => {
    signal?.addEventListener("abort", () => reject(signal.reason), { once: true });
  });

/** `status(code, headers)` responder whose error body carries `marker`, for leak assertions. */
export const statusWith =
  (marker: string) =>
  (code: number, headers: Record<string, string> = {}): Responder =>
  () =>
    Response.json({ error: { message: `upstream detail ${marker}` } }, { status: code, headers });

function keyOf(init: RequestInit | undefined): string {
  return (new Headers(init?.headers).get("authorization") ?? "").replace(/^(Bearer|Token) /, "");
}

export type FetchMock = ReturnType<typeof vi.fn<typeof fetch>>;

export function upstreamCall(fetchMock: FetchMock, i = 0): { url: string; init: RequestInit; headers: Headers } {
  const c = fetchMock.mock.calls[i];
  if (!c) throw new Error(`no upstream call #${i}`);
  const [input, init] = c;
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  return { url, init: init ?? {}, headers: new Headers(init?.headers) };
}

export function sentForm(fetchMock: FetchMock, i = 0): FormData {
  const { init } = upstreamCall(fetchMock, i);
  expect(init.body).toBeInstanceOf(FormData);
  return init.body as FormData;
}

/** A fetch mock that dispatches per provider key; queue responses with `on`, the last repeats. */
export function makeUpstream() {
  const responders = new Map<string, Responder[]>();
  const fetchMock = vi.fn<typeof fetch>(async (_input, init) => {
    const queue = responders.get(keyOf(init));
    if (!queue || queue.length === 0) throw new Error(`no responder for ${keyOf(init)}`);
    const r = queue.length > 1 ? queue.shift()! : queue[0]!;
    return r(init?.signal ?? undefined);
  });
  return {
    fetchMock,
    on: (key: string, ...rs: Responder[]): void => {
      responders.set(key, rs);
    },
    keysCalled: (): string[] => fetchMock.mock.calls.map(([, init]) => keyOf(init)),
  };
}
