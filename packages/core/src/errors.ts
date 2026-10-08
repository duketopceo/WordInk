/** Why dictation failed (R9). `Config` is an integration mistake, thrown before any network call. */
export type ErrorCode =
  | "MicDenied"
  | "NoSpeech"
  | "AuthFailed"
  | "RateLimited"
  | "ProviderDown"
  | "BadAudio"
  | "Config";

/** Session error codes the core reports (everything but `Config`). */
export const SESSION_ERROR_CODES: readonly ErrorCode[] = [
  "MicDenied",
  "NoSpeech",
  "AuthFailed",
  "RateLimited",
  "ProviderDown",
  "BadAudio",
];

const TEXT: Record<Exclude<ErrorCode, "Config">, { message: string; hint: string }> = {
  MicDenied: {
    message: "Microphone blocked: this site isn't allowed to use your microphone.",
    hint:
      "Allow microphone access in your browser's site settings: click the icon at the left of the address bar " +
      "(Chrome/Edge: Site settings > Microphone; Firefox: Permissions; Safari: Settings > Websites > Microphone), " +
      "choose Allow, then try again.",
  },
  NoSpeech: {
    message: "No speech detected.",
    hint: "Hold the button while you speak, and check that the right microphone is selected and not muted.",
  },
  AuthFailed: {
    message: "The speech provider rejected the credentials.",
    hint:
      "Check the provider key configured on your relay (or the dev key on localhost), and that your relay's " +
      "authorize hook accepts this user.",
  },
  RateLimited: {
    message: "Too many dictation requests right now.",
    hint: "Wait a moment and try again. Developers can raise the relay's rate limit or the provider plan.",
  },
  ProviderDown: {
    message: "The speech provider couldn't be reached.",
    hint: "Check your connection and try again. If it persists, the provider or your relay may be down.",
  },
  BadAudio: {
    message: "The speech provider couldn't process the recording.",
    hint: "Try a shorter recording, or a different microphone.",
  },
};

/** MicDenied hints for capture failures that aren't a permission refusal. */
const MIC_HINTS: Record<string, string> = {
  NotFoundError: "No microphone was found. Connect one, or check that your system can see it, then try again.",
  InsecureContextError: "Microphone access needs a secure page. Open this site over https (or localhost).",
  NotReadableError: "The microphone is in use by another app or can't be opened. Close other apps using it and try again.",
};

/** A dictation failure with a stable `code`, a human `message` and a fix `hint` (R9, AE2). */
export class WordInkError extends Error {
  override readonly name = "WordInkError";
  readonly code: ErrorCode;
  readonly hint: string;

  constructor(code: ErrorCode, message: string, hint: string, options?: { cause?: unknown }) {
    super(message, options);
    this.code = code;
    this.hint = hint;
  }
}

/** An integration mistake (bad options), thrown synchronously before any network call. */
export function configError(message: string, hint: string): WordInkError {
  return new WordInkError("Config", message, hint);
}

/**
 * The error for a session error code. `micErrorName` is the DOMException name from getUserMedia,
 * when known, to tell a missing or busy microphone from a refused permission.
 */
export function sessionError(code: ErrorCode, micErrorName?: string): WordInkError {
  if (code === "Config") return configError("Invalid configuration.", "Check the options passed to createDictation.");
  const text = TEXT[code];
  const micHint = code === "MicDenied" && micErrorName ? MIC_HINTS[micErrorName] : undefined;
  return new WordInkError(code, micHint ? "Microphone unavailable." : text.message, micHint ?? text.hint);
}
