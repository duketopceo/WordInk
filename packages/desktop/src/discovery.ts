import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { NotRunningError, WordInkDesktopError } from "./errors.js";

/** The `apiVersion` values this client can talk to. */
export const SUPPORTED_API_VERSIONS: readonly string[] = ["2"];

/** What `local-api.json` holds, with `url` filled in from `port`. */
export interface Discovery {
  url: string;
  token: string;
  apiVersion: string;
  pid: number;
}

export interface DiscoveryOptions {
  /** Full path of the discovery file. Wins over everything else. */
  discoveryPath?: string;
  /** Environment to read, for tests. Defaults to `process.env`. */
  env?: Record<string, string | undefined>;
  /** Platform name, for tests. Defaults to `process.platform`. */
  platform?: NodeJS.Platform;
  /** Home directory, for tests. Defaults to `os.homedir()`. */
  home?: string;
}

/**
 * Where the app writes `local-api.json`: `WORDINK_LOCAL_API_FILE` if set,
 * else `HEX_APPLICATION_SUPPORT_DIR` (the app's own override), else the
 * per-OS data folder.
 */
export function discoveryPath(options: DiscoveryOptions = {}): string {
  if (options.discoveryPath) return options.discoveryPath;
  const env = options.env ?? process.env;
  if (env.WORDINK_LOCAL_API_FILE) return env.WORDINK_LOCAL_API_FILE;
  if (env.HEX_APPLICATION_SUPPORT_DIR) {
    return join(env.HEX_APPLICATION_SUPPORT_DIR, "local-api.json");
  }
  const platform = options.platform ?? process.platform;
  const home = options.home ?? homedir();
  let folder: string;
  if (platform === "darwin") {
    folder = join(home, "Library", "Application Support", "wordink");
  } else if (platform === "win32") {
    folder = join(env.APPDATA ?? join(home, "AppData", "Roaming"), "wordink");
  } else {
    folder = join(env.XDG_DATA_HOME || join(home, ".local", "share"), "wordink");
  }
  return join(folder, "local-api.json");
}

/** Reads and validates the discovery file. Never returns the token in an error. */
export async function readDiscovery(options: DiscoveryOptions = {}): Promise<Discovery> {
  const path = discoveryPath(options);
  let text: string;
  try {
    text = await readFile(path, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      throw new NotRunningError("WordInk is not running (no local-api.json found)", { cause: error });
    }
    throw new WordInkDesktopError(`could not read the WordInk discovery file at ${path}`, { cause: error });
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw new WordInkDesktopError(`the WordInk discovery file at ${path} is not valid JSON`);
  }
  const doc = (raw ?? {}) as Record<string, unknown>;
  const url =
    typeof doc.url === "string"
      ? doc.url.replace(/\/+$/, "")
      : typeof doc.port === "number"
        ? `http://127.0.0.1:${doc.port}`
        : undefined;
  if (
    !url ||
    typeof doc.token !== "string" ||
    typeof doc.apiVersion !== "string" ||
    typeof doc.pid !== "number"
  ) {
    throw new WordInkDesktopError(`the WordInk discovery file at ${path} is missing fields`);
  }
  return { url, token: doc.token, apiVersion: doc.apiVersion, pid: doc.pid };
}
