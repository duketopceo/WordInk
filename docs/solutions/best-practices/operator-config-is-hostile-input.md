---
title: Operator config is hostile input — `in`, truthiness, and array-accepting checks are validation bypasses
date: 2026-10-05
category: best-practices
module: "@wordink/server gateway"
problem_type: best_practice
component: service_layer
severity: medium
applies_when:
  - "Validating operator-supplied JSON: config files, request fields, or state files the operator can touch"
  - "Indexing a record or env map with an untrusted key: provider names, env var names, header values"
  - "Branching on whether an operation failed, where failure has more than one cause (stat, fetch, parse)"
  - "Exporting operator-supplied environment variable names from a shell wrapper"
related_components:
  - "wordink-gateway CLI"
  - "gateway token store"
  - "gateway fallback router"
tags:
  - config-parsing
  - input-validation
  - prototype-chain
  - env-vars
  - fail-closed
  - gateway
---

# Operator config is hostile input — `in`, truthiness, and array-accepting checks are validation bypasses

## Context

A multi-reviewer pass on the desktop-gateway branch (plan: `docs/plans/2026-10-05-1131-feat-wordink-desktop-gateway-plan.md`) found no single large bug but one *class*: three reviewers independently flagged places where operator-controlled input — a JSON config file, environment lookups keyed by config strings, the token-state file, upstream HTTP responses — was validated with idioms that look correct but quietly accept values outside the intended domain. Every instance was in the new `packages/server` gateway surface (`packages/server/src/bin/wordink-gateway.ts`, `packages/server/src/internal.ts`, `packages/server/src/gateway/file-tokens.ts`, `packages/server/src/gateway/router.ts`, `packages/server/src/gateway/providers.ts`).

The same class had already bitten this codebase once: the browser relay's Origin check passed its own review and only fell when an adversarial pass attacked the URL *construction* underneath it — a protocol-relative request-target read as same-origin (session history). Loose shape checks pass friendly review; hostile input is what finds them.

## Guidance

Four idioms that are validation bypasses at a trust boundary, and the replacement for each. All four shipped fixes carry red-before-fix tests.

1. **`key in obj` accepts inherited properties.** `provider in DEFAULT_MODELS` passed for `"constructor"`, `"toString"`, and `"__proto__"`. Use `Object.hasOwn(obj, key)` for any membership test keyed by parsed input.
2. **Truthiness on an env/record lookup accepts inherited members and wrong types.** `env[entry.keyEnv]` made `keyEnv: "__proto__"` report "present" and would have sent `Authorization: Bearer undefined` upstream. Require the domain type: `typeof v === "string" && v.length > 0`.
3. **`typeof x === "object" && x !== null` accepts arrays.** A `[]` config root passed the object check and failed later with a misleading "providers must be an array". `isRecord` in `packages/server/src/internal.ts` now also requires `!Array.isArray(x)` — shared helpers are where this fix belongs, not each call site.
4. **"It failed" is not one state.** A non-ENOENT `stat` error is not proof a file is absent (blind-overwrite risk); a `fetch` whose response arrived but failed `json()` is not "no response" (the status still matters for cooldown/logging); an attempt killed by the shared deadline is not a provider failure (it earned no cooldown). Model failure as a tri-state — success / cleanly absent / unknown — and fail closed on unknown.

Adjacent rules from the same pass:

- **Validate parsed values, not string prefixes.** `host.startsWith("127.")` accepts a DNS name that can resolve off-loopback; `node:net`'s `isIP` does the real check. A `url` override that receives a credential must be https, or http only on verified loopback.
- **Do not cache a possibly-transient failure as a terminal state.** The token store had latched "unreadable" under the file's (ino, mtime, size) key, so a permission flip away and back kept failing every device token. Cache successes only; let failures retry.
- **Read-modify-write on a shared file needs a change check before the write.** `create`/`revoke` re-stat and retry once rather than silently overwriting a concurrent change. A lockfile was deliberately rejected — the writers are CLI invocations and a stale lock is its own failure mode.
- **Shell wrappers that export operator-named variables must validate the identifier** and refuse names that steer the runtime (`NODE_OPTIONS`, `LD_*`, `PATH`, `HOME`, `SHELL`, `IFS`, `ENV`, `BASH_ENV`, `CDPATH`, `XDG_*`). A skipped line must warn, not abort — under `set -e` an `export` failure becomes a service restart loop.

## Why This Matters

Individually these read as style nits; together they were the difference between the auth boundary holding and leaking. `provider: "constructor"` would have resolved a model from the prototype chain; `env["__proto__"]` would have put a garbage Bearer token on the wire; a remote `http://` provider URL would have sent the operator's key over cleartext; treating an unstatable token file as absent could have blind-overwritten real device tokens. The gateway exists to hold provider keys and per-device tokens — its config surface *is* the trust boundary (AGENTS.md: "long-lived provider keys never go to the browser", the server "fails closed until `authorize` is supplied"). The review established that this principle extends inward to everything the operator touches.

## When to Apply

- Any new `loadConfig`-style parser for operator JSON: a new CLI subcommand's config, the token-file format if it evolves, plugin manifests.
- Any lookup keyed by untrusted strings: `env[...]`, `map[userInput]`, `obj[header]`.
- Any "did the thing fail?" branch where failure has more than one cause: stat, fetch, subprocess, parse.
- Any new provider or route added to the gateway — the validation surfaces above all need re-examining, and the maintainability note from review applies: several checks live in more than one place already.

## Examples

```ts
// Before: accepts "constructor", "toString", "__proto__"
if (typeof provider !== "string" || !(provider in DEFAULT_MODELS)) { ... }
// After
if (typeof provider !== "string" || !Object.hasOwn(DEFAULT_MODELS, provider)) { ... }
```

```ts
// Before: env["__proto__"] is truthy → reported "present", ships `Bearer undefined`
const key = env[entry.keyEnv];
if (key) resolved.push({ ...entry, key });
// After (packages/server/src/bin/wordink-gateway.ts resolveKeys)
if (typeof key === "string" && key.length > 0) resolved.push({ ...entry, key });
```

```ts
// Before: any stat error read as "file missing" — rename could blind-overwrite an existing file
// After (packages/server/src/gateway/file-tokens.ts statKey): tri-state, unknown fails closed
return err.code === "ENOENT" ? null : "unstatable";
```

```ts
// After (packages/server/src/gateway/router.ts): an attempt clipped by the shared deadline proves
// nothing about the provider, so it earns no cooldown
const clipped = budget < attemptMs && result.status === undefined && now() - attemptStart >= budget;
```

## Related

- Plan of record: `docs/plans/2026-10-05-1131-feat-wordink-desktop-gateway-plan.md` (KTD2 keyEnv indirection, KTD5 cooldown/deadline policy).
- Operator-facing contract: `apps/docs/pages/desktop.md` (schema table, url host rules).
- Fail-closed precedent: `packages/server/README.md` security notes; AGENTS.md architecture invariants.
