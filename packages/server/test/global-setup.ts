import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import type { TestProject } from "vitest/node";

const PKG = join(import.meta.dirname, "..");

function build(): void {
  execFileSync("pnpm", ["build"], { cwd: PKG, stdio: "pipe" });
  if (!existsSync(join(PKG, "dist", "bin", "wordink-gateway.js"))) {
    throw new Error("global-setup: pnpm build did not produce dist/bin/wordink-gateway.js");
  }
}

export default function setup(project: TestProject): void {
  build();
  // Watch mode reruns skip globalSetup, so rebuild here to keep dist/ in step with source edits.
  project.onTestsRerun(build);
}
