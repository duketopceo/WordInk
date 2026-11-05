/** Base class for every error this package throws on purpose. */
export class WordInkDesktopError extends Error {
  constructor(message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = new.target.name;
  }
}

/** The desktop app is not running (no discovery file, a stale one, or a closed port). */
export class NotRunningError extends WordInkDesktopError {}

/** The app refused the bearer token (HTTP 401). Reconnect to read the current one. */
export class UnauthorizedError extends WordInkDesktopError {}

/** The app speaks an `apiVersion` this client does not know. */
export class UnsupportedVersionError extends WordInkDesktopError {
  readonly apiVersion: string;
  readonly supported: readonly string[];

  constructor(apiVersion: string, supported: readonly string[]) {
    super(
      `WordInk local API version ${JSON.stringify(apiVersion)} is not supported by this client (supports ${supported.join(", ")}); update @wordink/desktop`,
    );
    this.apiVersion = apiVersion;
    this.supported = supported;
  }
}

/** The app answered with another error. `code` is its JSON error code, when it sent one. */
export class ApiError extends WordInkDesktopError {
  readonly status: number;
  readonly code: string | undefined;

  constructor(status: number, code: string | undefined) {
    super(`WordInk local API answered ${status}${code ? ` (${code})` : ""}`);
    this.status = status;
    this.code = code;
  }
}
