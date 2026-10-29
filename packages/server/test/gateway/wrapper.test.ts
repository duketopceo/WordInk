import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Black-box coverage of examples/gateway-systemd/wordink-gateway-run: a fake
// `omaseal` and a fake `wordink-gateway` on PATH make what the wrapper exported
// (and skipped) observable.
const WRAPPER = join(import.meta.dirname, "../../../../examples/gateway-systemd/wordink-gateway-run");

const NODE_GATEWAY_SCRIPT =
  '#!/usr/bin/env node\nconsole.log(`ARGV=${process.argv.slice(2).join(" ")}`);\nfor (const name of ["GROQ_API_KEY", "OPENAI_API_KEY", "NODE_OPTIONS", "LD_PRELOAD", "XDG_HACK", "PATH"]) console.log(`${name}=${process.env[name] ?? ""}`);\n';

function runWrapper(
  map: string | null,
  gatewayMode = 0o755,
  gatewayScript = NODE_GATEWAY_SCRIPT,
  opts: { prependEmptyDir?: boolean; gatewayInCwd?: boolean } = {},
): { stdout: string; stderr: string; status: number | null } {
  const dir = mkdtempSync(join(tmpdir(), "wordink-run-"));
  try {
    const binDir = join(dir, "bin");
    const configDir = join(dir, "config");
    mkdirSync(binDir, { recursive: true });
    // `omaseal get <service> <account>` -> key-for-<service>-<account>, or fails for "missing".
    writeFileSync(
      join(binDir, "omaseal"),
      '#!/bin/sh\n[ "$1" = get ] || exit 1\ncase "$2" in missing) exit 1 ;; esac\nprintf "key-for-%s-%s\\n" "$2" "$3"\n',
      { mode: 0o755 },
    );
    // A package bin can be a non-executable JS file behind a symlink.
    writeFileSync(
      join(opts.gatewayInCwd ? dir : binDir, "wordink-gateway"),
      gatewayScript,
      { mode: gatewayMode },
    );
    writeFileSync(join(binDir, "node"), `#!/bin/sh\nexec "${process.execPath}" "$@"\n`, { mode: 0o755 });
    let searchPath = `${binDir}:/usr/bin:/bin`;
    if (opts.prependEmptyDir) {
      const emptyDir = join(dir, "empty");
      mkdirSync(emptyDir, { recursive: true });
      searchPath = `${emptyDir}:${searchPath}`;
    }
    if (opts.gatewayInCwd) {
      searchPath = `${binDir}:/usr/bin:`;
    }
    if (map !== null) {
      mkdirSync(join(configDir, "wordink"), { recursive: true });
      writeFileSync(join(configDir, "wordink", "gateway-keys"), map);
    }
    const r = spawnSync("sh", [WRAPPER], {
      cwd: dir,
      env: {
        PATH: searchPath,
        HOME: dir,
        XDG_CONFIG_HOME: configDir,
      },
      encoding: "utf8",
      timeout: 15_000,
    });
    return { stdout: r.stdout, stderr: r.stderr, status: r.status };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

describe("wordink-gateway-run", () => {
  it("exports mapped keys via omaseal, including a final line without a newline", () => {
    const r = runWrapper("GROQ_API_KEY=groq/default\nOPENAI_API_KEY=openai/main");
    expect(r.status).toBe(0);
    expect(r.stdout).toContain("ARGV=serve");
    expect(r.stdout).toContain("GROQ_API_KEY=key-for-groq-default");
    expect(r.stdout).toContain("OPENAI_API_KEY=key-for-openai-main");
  });

  it("reaches exec even when the map file is absent", () => {
    const r = runWrapper(null);
    expect(r.status).toBe(0);
    expect(r.stdout).toContain("ARGV=serve");
    expect(r.stdout).toContain("GROQ_API_KEY=\n");
  });

  it("runs a non-executable JavaScript package bin through Node", () => {
    const r = runWrapper("GROQ_API_KEY=groq/default", 0o644);
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout).toContain("ARGV=serve");
    expect(r.stdout).toContain("GROQ_API_KEY=key-for-groq-default");
  });

  it("executes a non-JavaScript executable gateway directly, not through Node", () => {
    const r = runWrapper(null, 0o755, '#!/bin/sh\necho "ARGV=$*"\n');
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout).toContain("ARGV=serve");
  });

  it("resolves the gateway from a later PATH directory", () => {
    const r = runWrapper(null, 0o755, undefined, { prependEmptyDir: true });
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout).toContain("ARGV=serve");
  });

  it("resolves the gateway from the current directory for a trailing PATH colon", () => {
    const r = runWrapper(null, 0o755, undefined, { gatewayInCwd: true });
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout).toContain("ARGV=serve");
  });

  it("skips comments, blanks, invalid names and refused runtime names without aborting", () => {
    const r = runWrapper(
      [
        "# comment",
        "",
        "GROQ_API_KEY=groq/default",
        "BAD NAME=x/y",
        "1BAD=x/y",
        "BIN=x/y",
        "NODE=x/y",
        "NODE_OPTIONS=x/y",
        "LD_PRELOAD=x/y",
        "XDG_HACK=x/y",
        "PATH=x/y",
        "OPENAI_API_KEY=missing/acct",
        "",
      ].join("\n"),
    );
    expect(r.status).toBe(0);
    expect(r.stdout).toContain("ARGV=serve"); // exec still reached despite the warnings
    expect(r.stdout).toContain("GROQ_API_KEY=key-for-groq-default");
    expect(r.stdout).toContain("NODE_OPTIONS=\n"); // refused, never exported
    expect(r.stderr).toMatch(/refusing to set BIN/);
    expect(r.stderr).toMatch(/refusing to set NODE/);
    expect(r.stdout).toContain("LD_PRELOAD=\n");
    expect(r.stdout).toContain("XDG_HACK=\n");
    expect(r.stdout).toMatch(/PATH=[^\n]*\/usr\/bin/); // PATH was not replaced with a "key"
    expect(r.stdout).toContain("OPENAI_API_KEY=\n"); // omaseal lookup failed -> skipped, not "key-for-…"
    expect(r.stderr).toMatch(/refusing to set NODE_OPTIONS/);
    expect(r.stderr).toMatch(/refusing to set LD_PRELOAD/);
    expect(r.stderr).toMatch(/refusing to set XDG_HACK/);
    expect(r.stderr).toMatch(/refusing to set PATH/);
    expect(r.stderr).toMatch(/not a valid environment name/);
    expect(r.stderr).toMatch(/no missing\/acct for OPENAI_API_KEY/);
  });
});
