// wordink-gateway pair: one-tap remote onboarding. Mints a device token and
// prints a `wordink-remote://pair?url=…&token=…` deep link — on iOS as a
// camera-scannable QR — so the WordInk Remote app needs no manual fields.
import qrcode from "qrcode-generator";
import { existsSync } from "node:fs";
import { isIP } from "node:net";
import { FileTokenStore } from "../node.js";
import {
  defaultConfigPath,
  isLoopback,
  loadConfig,
  normalizePublicUrl,
  urlHostname,
} from "./gateway-config.js";

const PREFIX = "wordink-gateway:";
const PAIR_SCHEME = "wordink-remote";
const PAIR_USAGE =
  `${PREFIX} usage: wordink-gateway pair [--url <endpoint>] [--label <name>] [--no-qr]` +
  ` (a bare --url host implies https)`;

/**
 * The transport rule the iOS client enforces, mirrored so a pair link that can
 * never connect fails here instead of on the phone: https is fine anywhere;
 * http only for LAN / Tailscale (CGNAT) endpoints. Deliberately stricter than
 * the Swift original on IPv6 literals (link-local/ULA), never looser.
 */
function isLocalNetworkHost(host: string): boolean {
  const h = host.toLowerCase();
  if (h === "localhost" || h === "::1" || h.endsWith(".local")) return true;
  const ip = isIP(h);
  if (ip === 6) return false;
  if (ip !== 4) return !h.includes("."); // unqualified single-label name — LAN
  const [a, b] = h.split(".").map(Number);
  return (
    a === 10 || a === 127 || a === 192 && b === 168 ||
    a === 169 && b === 254 || a === 172 && b !== undefined && b >= 16 && b <= 31 ||
    a === 100 && b !== undefined && b >= 64 && b <= 127
  );
}

/** Terminal QR: two module rows per text row, light modules as block glyphs. */
function printQr(link: string): void {
  const qr = qrcode(0, "M");
  qr.addData(link);
  qr.make();
  const n = qr.getModuleCount();
  const rows: string[] = [];
  for (let y = -4; y < n + 4; y += 2) {
    let line = "████";
    for (let x = -2; x < n + 2; x++) {
      const top = y < 0 || y >= n || x < 0 || x >= n ? false : qr.isDark(y, x);
      const bottom = y + 1 < 0 || y + 1 >= n || x < 0 || x >= n ? false : qr.isDark(y + 1, x);
      // Dark modules stay blank so the terminal's dark background forms them.
      line += top ? bottom ? " " : "▄" : bottom ? "▀" : "█";
    }
    rows.push(line + "████");
  }
  console.log(rows.join("\n"));
}

export async function cmdPair(args: string[]): Promise<number> {
  let urlArg: string | undefined;
  let label = "iphone";
  let showQr = true;
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    // Values that look like another flag are never endpoints or labels — without
    // this, `pair --url --no-qr` mints a link to the hostname "--no-qr".
    if (arg === "--url" || arg === "--label") {
      const v = args[++i];
      if (v === undefined || v === "" || v.startsWith("--")) {
        console.error(PAIR_USAGE);
        return 1;
      }
      if (arg === "--url") urlArg = v;
      else label = v;
    } else if (arg === "--no-qr") {
      showQr = false;
    } else {
      console.error(`${PREFIX} unknown pair argument "${arg}"`);
      console.error(PAIR_USAGE);
      return 1;
    }
  }

  let endpoint: string | undefined;
  if (urlArg !== undefined) {
    endpoint = normalizePublicUrl(urlArg, "--url");
  } else {
    // An absent config just means "no publicUrl default"; a present-but-malformed
    // one is a real error and surfaces as itself rather than looking absent.
    const configPath = defaultConfigPath();
    const config = existsSync(configPath) ? await loadConfig(configPath) : undefined;
    endpoint = config?.publicUrl;
    if (endpoint === undefined) {
      console.error(
        `${PREFIX} no --url given and no "publicUrl" in ${configPath} — ` +
          `pass --url with the endpoint the phone reaches (e.g. the tailnet name).`,
      );
      return 1;
    }
  }

  const url = new URL(endpoint);
  const host = urlHostname(url);
  if (url.protocol === "http:" && !isLocalNetworkHost(host)) {
    console.error(
      `${PREFIX} http is only pairable for LAN or Tailscale endpoints — the phone rejects ` +
        `cleartext to ${host}. Use https, or reach the gateway via LAN/tailnet.`,
    );
    return 1;
  }
  if (isLoopback(host) || host === "0.0.0.0" || host === "::") {
    console.error(
      `${PREFIX} warning: ${host} is not reachable from a phone — pass --url with the ` +
        `LAN/tailnet address instead.`,
    );
    return 1;
  }

  const made = await new FileTokenStore().create(label, "device");
  const link =
    `${PAIR_SCHEME}://pair?url=${encodeURIComponent(endpoint)}&token=${encodeURIComponent(made.token)}`;
  console.log(`pair link: ${link}`);
  if (showQr) {
    console.log("\nscan with the iPhone camera to pair:\n");
    printQr(link);
    console.log("");
  }
  // The link embeds the only copy of the token (the store keeps its hash).
  console.log(
    `created id=${made.id} scope=device label="${made.label}" — the link carries the token; ` +
      `it is never shown again. Revoke with \`tokens revoke ${made.id}\`.`,
  );
  return 0;
}
