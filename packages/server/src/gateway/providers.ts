import { buildWhisperForm, cleanLanguage, cleanTemperature, type AudioFile, type ResponseFormat } from "./multipart.js";
import { keyterms, mergePrompt } from "./vocabulary.js";

export type { AudioFile, ResponseFormat } from "./multipart.js";

export const GROQ_AUDIO_URL = "https://api.groq.com/openai/v1/audio/transcriptions";
export const OPENAI_AUDIO_URL = "https://api.openai.com/v1/audio/transcriptions";
export const DEEPGRAM_LISTEN_URL = "https://api.deepgram.com/v1/listen";

export type ProviderName = "groq" | "openai" | "deepgram";

/** One entry of the operator's ordered provider list (KTD2). Config names the key's env var only. */
export interface ProviderEntry {
  provider: ProviderName;
  keyEnv: string;
  /** Provider model to call. Defaults per provider (KTD4). */
  model?: string;
}

/** A provider entry with its key resolved from the environment. */
export interface ResolvedEntry {
  provider: ProviderName;
  key: string;
  model?: string;
}

export const DEFAULT_MODELS: Readonly<Record<ProviderName, string>> = {
  groq: "whisper-large-v3-turbo",
  openai: "gpt-4o-transcribe",
  deepgram: "nova-3",
};
/** `gpt-4o-transcribe` only does `json`/`text`, so OpenAI `verbose_json` uses this (KTD4). */
export const OPENAI_VERBOSE_MODEL = "whisper-1";

export interface TranscribeOptions {
  /** The client's prompt; merged with `vocabulary` for Whisper-style providers. */
  prompt?: string | undefined;
  /** Gateway-wide operator vocabulary (KTD6). */
  vocabulary?: readonly string[] | undefined;
  /** Dropped unless it matches the KTD9 tag pattern. */
  language?: string | undefined;
  /** Default `json`. `text` is fetched as `json`; the caller formats it. */
  responseFormat?: ResponseFormat | undefined;
  temperature?: number | undefined;
}

/** The OpenAI `verbose_json` fields the gateway can fill; provider-specific fields are dropped (R7). */
export interface VerboseTranscript {
  text: string;
  language?: string;
  duration?: number;
  segments?: { id: number; start: number; end: number; text: string }[];
}

/**
 * `client` failures (400, 413, 415) are the request's fault and must not fall through; every
 * other failure is `retryable` (KTD5). `status` is absent when no HTTP response arrived
 * (network error, timeout). Upstream error bodies are never carried (they can quote keys).
 */
export type TranscribeResult =
  | { ok: true; text: string; verbose?: VerboseTranscript }
  | { ok: false; kind: "retryable" | "client"; status?: number; retryAfter?: number };

const CLIENT_ERRORS = new Set([400, 413, 415]);

/** One batch transcription attempt against one provider entry. Never throws. */
export async function transcribe(
  entry: ResolvedEntry,
  audio: AudioFile,
  options: TranscribeOptions,
  signal?: AbortSignal,
): Promise<TranscribeResult> {
  const verbose = options.responseFormat === "verbose_json";
  const language = cleanLanguage(options.language);
  const vocabulary = options.vocabulary ?? [];

  let url: string;
  let init: RequestInit;
  if (entry.provider === "deepgram") {
    const query = new URLSearchParams({ model: entry.model ?? DEFAULT_MODELS.deepgram });
    if (language) query.set("language", language);
    query.set("smart_format", "true");
    for (const term of keyterms(vocabulary)) query.append("keyterm", term);
    url = `${DEEPGRAM_LISTEN_URL}?${query}`;
    init = {
      method: "POST",
      headers: { authorization: `Token ${entry.key}`, "content-type": audio.type || "application/octet-stream" },
      body: audio.bytes as Uint8Array<ArrayBuffer>,
    };
  } else {
    const model =
      entry.provider === "openai" && verbose ? OPENAI_VERBOSE_MODEL : (entry.model ?? DEFAULT_MODELS[entry.provider]);
    const temperature = cleanTemperature(options.temperature);
    url = entry.provider === "groq" ? GROQ_AUDIO_URL : OPENAI_AUDIO_URL;
    init = {
      method: "POST",
      headers: { authorization: `Bearer ${entry.key}` },
      body: buildWhisperForm(audio, {
        model,
        prompt: mergePrompt(vocabulary, options.prompt),
        language,
        response_format: verbose ? "verbose_json" : "json",
        temperature: temperature === undefined ? undefined : String(temperature),
      }),
    };
  }
  if (signal) init.signal = signal;

  let res: Response;
  let body: unknown;
  try {
    res = await fetch(url, init);
    if (!res.ok) {
      await res.body?.cancel().catch(() => {});
      return failure(res);
    }
    body = await res.json();
  } catch {
    return { ok: false, kind: "retryable" };
  }

  const parsed = entry.provider === "deepgram" ? fromDeepgram(body, language) : fromWhisper(body);
  if (!parsed) return { ok: false, kind: "retryable", status: res.status };
  return verbose ? { ok: true, text: parsed.text, verbose: parsed } : { ok: true, text: parsed.text };
}

function failure(res: Response): TranscribeResult {
  const kind = CLIENT_ERRORS.has(res.status) ? "client" : "retryable";
  const header = res.headers.get("retry-after");
  return header && /^\d+$/.test(header)
    ? { ok: false, kind, status: res.status, retryAfter: Number(header) }
    : { ok: false, kind, status: res.status };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function fromWhisper(body: unknown): VerboseTranscript | null {
  if (!isRecord(body) || typeof body.text !== "string") return null;
  const out: VerboseTranscript = { text: body.text };
  if (typeof body.language === "string") out.language = body.language;
  if (typeof body.duration === "number") out.duration = body.duration;
  if (Array.isArray(body.segments)) {
    out.segments = body.segments.filter(isRecord).flatMap((s) =>
      typeof s.id === "number" && typeof s.start === "number" && typeof s.end === "number" && typeof s.text === "string"
        ? [{ id: s.id, start: s.start, end: s.end, text: s.text }]
        : [],
    );
  }
  return out;
}

function fromDeepgram(body: unknown, language: string | undefined): VerboseTranscript | null {
  if (!isRecord(body) || !isRecord(body.results) || !Array.isArray(body.results.channels)) return null;
  const channel: unknown = body.results.channels[0];
  if (!isRecord(channel) || !Array.isArray(channel.alternatives)) return null;
  const alt: unknown = channel.alternatives[0];
  if (!isRecord(alt) || typeof alt.transcript !== "string") return null;
  const out: VerboseTranscript = { text: alt.transcript };
  if (language) out.language = language;
  if (isRecord(body.metadata) && typeof body.metadata.duration === "number") out.duration = body.metadata.duration;
  return out;
}
