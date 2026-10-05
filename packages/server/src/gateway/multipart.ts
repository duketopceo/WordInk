/** Audio as buffered from the client's `file` field. Re-wrapped per attempt, never streamed twice. */
export interface AudioFile {
  bytes: Uint8Array;
  /** The file part's content type; may be empty. */
  type: string;
  name: string;
}

export type ResponseFormat = "json" | "text" | "verbose_json";
export const RESPONSE_FORMATS: readonly ResponseFormat[] = ["json", "text", "verbose_json"];

/** The OpenAI transcription fields the gateway understands (R1). Anything else is ignored. */
export interface TranscriptionForm {
  file: AudioFile;
  /** Accepted and ignored for routing (R3); each provider entry carries its own model. */
  model?: string;
  prompt?: string;
  language?: string;
  responseFormat: ResponseFormat;
  temperature?: number;
}

export type ParseError = "invalid_form" | "missing_file" | "unsupported_response_format";
export type ParseResult = { ok: true; form: TranscriptionForm } | { ok: false; error: ParseError };

const LANGUAGE_RE = /^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$/;

/** A client `language` that matches the KTD9 tag pattern, otherwise `undefined` (dropped). */
export function cleanLanguage(value: string | undefined | null): string | undefined {
  return typeof value === "string" && LANGUAGE_RE.test(value) ? value : undefined;
}

/** A temperature in [0, 1], otherwise `undefined` (dropped). */
export function cleanTemperature(value: string | number | undefined | null): number | undefined {
  if (value === undefined || value === null || value === "") return undefined;
  const n = typeof value === "number" ? value : Number(value);
  return Number.isFinite(n) && n >= 0 && n <= 1 ? n : undefined;
}

/**
 * Parse an already-buffered multipart body (KTD7). The original request stream is consumed by
 * the size cap, so the bytes are re-wrapped in a `Response` to reuse the platform parser.
 */
export async function parseTranscriptionForm(bytes: Uint8Array, contentType: string | null): Promise<ParseResult> {
  let data: FormData;
  try {
    data = await new Response(bytes, { headers: { "content-type": contentType ?? "" } }).formData();
  } catch {
    return { ok: false, error: "invalid_form" };
  }

  const file = data.get("file");
  if (!(file instanceof Blob)) return { ok: false, error: "missing_file" };

  const format = text(data, "response_format") ?? "json";
  if (!(RESPONSE_FORMATS as readonly string[]).includes(format)) {
    return { ok: false, error: "unsupported_response_format" };
  }

  const form: TranscriptionForm = {
    file: {
      bytes: new Uint8Array(await file.arrayBuffer()),
      type: file.type,
      name: (file instanceof File && file.name) || "audio",
    },
    responseFormat: format as ResponseFormat,
  };
  const model = text(data, "model");
  if (model) form.model = model;
  const prompt = text(data, "prompt");
  if (prompt) form.prompt = prompt;
  const language = cleanLanguage(text(data, "language"));
  if (language) form.language = language;
  const temperature = cleanTemperature(text(data, "temperature"));
  if (temperature !== undefined) form.temperature = temperature;
  return { ok: true, form };
}

function text(data: FormData, name: string): string | undefined {
  const value = data.get(name);
  return typeof value === "string" ? value : undefined;
}

/** A fresh multipart body for one Whisper-style attempt (Groq, OpenAI). */
export function buildWhisperForm(file: AudioFile, fields: Record<string, string | undefined>): FormData {
  const form = new FormData();
  form.append("file", new Blob([file.bytes as Uint8Array<ArrayBuffer>], { type: file.type }), file.name);
  for (const [name, value] of Object.entries(fields)) {
    if (value !== undefined) form.append(name, value);
  }
  return form;
}
