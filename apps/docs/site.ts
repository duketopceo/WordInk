// Shared by vite.config.ts (rendering) and the e2e specs (so the specs run the exact snippets the
// pages show): the page list, the release placeholders and the snippet includes.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export const DOCS_DIR = fileURLToPath(new URL(".", import.meta.url));
const REPO = new URL("../../", import.meta.url);

/** Docs pages, in nav order. Each is `pages/<slug>.md`, rendered into `<slug>.html`. */
export const PAGES = [
  { slug: "index", nav: "Home", title: "WordInk: dictation as a component" },
  { slug: "quickstart-html", nav: "Quickstart: HTML", title: "Quickstart: plain HTML" },
  { slug: "quickstart-react", nav: "Quickstart: React", title: "Quickstart: React" },
  { slug: "relay", nav: "Relay", title: "The credential relay" },
  { slug: "desktop", nav: "Desktop apps", title: "Desktop apps: the WordInk gateway" },
  { slug: "providers", nav: "Providers", title: "Providers" },
  { slug: "local", nav: "Local engine", title: "Local engine" },
  { slug: "transform", nav: "Post-processing", title: "Post-processing with transform" },
  { slug: "privacy", nav: "Privacy", title: "Privacy and data flows" },
  { slug: "dev-keys", nav: "Dev keys", title: "Dev keys" },
] as const;

export const CDN_FILE = "dist/wordink-web.cdn.js";

export function packageVersion(name: "web" | "local" | "core" | "react" | "server"): string {
  const pkg = JSON.parse(readFileSync(new URL(`packages/${name}/package.json`, REPO), "utf8")) as { version: string };
  return pkg.version;
}

/** `sha384-…` Subresource Integrity value for some bytes. */
export function sri(bytes: Uint8Array): string {
  return `sha384-${createHash("sha384").update(bytes).digest("base64")}`;
}

/** The SRI of the locally built CDN bundle (`pnpm build` must have run). */
export function localCdnSri(): string {
  return sri(readFileSync(new URL(`packages/web/${CDN_FILE}`, REPO)));
}

export const cdnUrl = (version: string) => `https://cdn.jsdelivr.net/npm/@wordink/web@${version}/${CDN_FILE}`;

export interface Release {
  webVersion: string;
  localVersion: string;
  webSri: string;
}

/**
 * Version pins and the CDN bundle's SRI hash for the snippets. The hash is computed from the local
 * build of this exact version; when that version is already on jsDelivr, the published bytes win
 * (they are what readers load) and a mismatch is reported, since a wrong hash blocks the script.
 */
export async function release(options: { checkCdn: boolean }): Promise<Release> {
  const webVersion = packageVersion("web");
  const localVersion = packageVersion("local");
  let webSri = localCdnSri();
  if (options.checkCdn && webVersion !== "0.0.0") {
    try {
      const res = await fetch(cdnUrl(webVersion), { signal: AbortSignal.timeout(10_000) });
      if (res.ok) {
        const published = sri(new Uint8Array(await res.arrayBuffer()));
        if (published !== webSri) {
          console.warn(`[docs] the local CDN build differs from @wordink/web@${webVersion} on jsDelivr; using the published hash.`);
        }
        webSri = published;
      }
    } catch {
      // Not published yet, or offline: the local build is the release candidate.
    }
  }
  return { webVersion, localVersion, webSri };
}

export function fillPlaceholders(text: string, r: Release): string {
  return text
    .replaceAll("__WEB_VERSION__", r.webVersion)
    .replaceAll("__LOCAL_VERSION__", r.localVersion)
    .replaceAll("__WEB_SRI__", r.webSri);
}

/** A file under `snippets/`, with placeholders filled. */
export function snippet(name: string, r: Release): string {
  return fillPlaceholders(readFileSync(new URL(`snippets/${name}`, import.meta.url), "utf8"), r).trimEnd();
}
