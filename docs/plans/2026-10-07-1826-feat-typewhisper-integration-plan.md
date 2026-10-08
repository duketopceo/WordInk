# Plan: TypeWhisper integration — `/v1/models` + docs

**Branch:** `feat/p2-typewhisper` (stacked on `feat/p1-closeout` → PR chain tail)
**Origin:** `docs/brainstorms/2026-10-07---typewhisper-gateway-requirements.md` (R/A/F/AE IDs trace from it)
**Parent plan:** `docs/plans/2026-10-05-1131-feat-wordink-desktop-gateway-plan.md`

## Goal Capsule

- **Objective:** a TypeWhisper user configures the bundled "OpenAI Compatible" engine against `wordink-gateway` using nothing but the docs page — model discovery works, the exact settings are written down, and the two unsupported modes are named.
- **Product authority:** Luke (duketopceo).
- **Blocked on:** nothing. Live TypeWhisper verification is an operator residual — no macOS/Windows/iOS device exists in this workspace.

## Key Decisions

- **KTD1 — No WordInk plugin.** TypeWhisper's bundled `OpenAiCompatible` plugin (macOS, Windows `plugins/TypeWhisper.Plugin.OpenAiCompatible`, iOS 0.3.0+) already posts `multipart` to `{baseURL}/v1/audio/transcriptions` with `Bearer` auth, `file` + `model` + `response_format=json` + optional `language`/`prompt`, decodes `{text, language?, segments?}`, and maps 401/429/413 the way the gateway answers them. Verified against source (`OpenAICompatiblePlugin.swift` `performBatchTranscriptionRequest`). A branded `.bundle` would add nothing and is unbuildable here (Swift/Xcode). (user-approved scope: docs + models endpoint)
- **KTD2 — `/v1/models` is a second token-gated GET route under the gateway mount.** Dispatch follows the existing exact-path pattern in `createRelay`'s gateway branch (`index.ts`); the handler shares the gateway's `bearerToken`/`verify`/limiter path — same 401 shape, same per-token budget. No CORS, no `authorize`, no Origin logic — it is a device endpoint, not a browser one. When `gateway` is absent the path falls through to the normal 404.
- **KTD3 — The models list reports effective models only.** One entry per configured provider's `model ?? DEFAULT_MODELS[provider]`, de-duplicated, in the OpenAI list shape `{object:"list", data:[{id, object:"model", created, owned_by}]}`. `owned_by` is a neutral string (e.g. `"wordink"`) — provider names, upstream URLs, keys, and vocabulary never appear (R7 posture carries over).
- **KTD4 — Docs teach the exact plugin settings, including its defaults' traps.** Base URL **without** `/v1` (the plugin appends `/v1/audio/transcriptions` itself — entering `…/v1` produces `/v1/v1/…` 404s); transport left on Auto is safe because none of the discovered model IDs are TypeWhisper's realtime names (`gpt-live-transcribe`, `gpt-realtime-whisper`); translate mode is **unsupported** (no `/v1/audio/translations`; Deepgram can't serve it — deferred). Remote devices need a non-loopback `host` in the gateway config (cleartext — Tailscale/LAN only).

## Requirements (traced)

- R1 (docs-only path works end to end) → U2
- R2 (`/v1/models` shape + content) → U1
- R3 (same auth/limiter as transcriptions) → U1
- R4 (404 when gateway absent) → U1
- R5 (TypeWhisper docs section: settings, batch note, translate unsupported, remote bind) → U2
- R6 (parity stated honestly; live check marked operator-pending) → U2

## Actors

- **A1 — desktop app operator:** TypeWhisper user; holds a `wdk_` device token only.
- **A2 — gateway operator:** runs `wordink-gateway`; owns provider config and tokens. Unaffected — no new config surface.

## Implementation Units

### U1. `GET {basePath}/v1/models` endpoint

**Files:** `packages/server/src/gateway/index.ts` (handler + path constant), `packages/server/src/index.ts` (dispatch line beside the transcriptions mount), `packages/server/test/gateway/route.test.ts`, `.changeset/` entry for `@wordink/server`.

**Approach:**
1. Export a `GATEWAY_MODELS_PATH = "/v1/models"` constant; in `createRelay`, dispatch `path === ${basePath}${GATEWAY_MODELS_PATH}` to the gateway handler when configured (same fall-through-to-404 as transcriptions when it is not).
2. Inside the handler, split on method+path: `GET` → models; existing `POST` transcriptions untouched. `POST /v1/models` (and any other method) → 405 OpenAI error shape with `allow: GET`, mirroring the transcriptions 405 pattern.
3. Models response: `{object:"list", data:[…]}` — one `{id, object:"model", created:<unix ts or 0>, owned_by:<neutral>}` per unique effective model (dedup across entries sharing a model id). Order = provider order, stable.
4. Auth/limiter identical to the transcriptions path (same `bearerToken` + `verify` + per-token `limiter` call), so a bad/absent/revoked token is indistinguishable there.

**Test scenarios:**
- Happy: configured providers `[groq default, openai gpt-4o-transcribe]` → 200, `object:"list"`, ids `["whisper-large-v3-turbo","gpt-4o-transcribe"]`; entry with `model` override → override id, not the default (covers R2).
- Edge: two entries resolving to the same model → listed once; empty providers list already unreachable (config validation requires ≥1) — assert documented behavior anyway if reachable.
- Auth: missing header, garbage token, revoked token → the exact 401 shape transcriptions returns; the limiter counter is consumed on `/v1/models` calls (covers R3).
- Method: `POST /v1/models` → 405 + `allow: GET`; `PUT`/`DELETE` likewise.
- Integration: no `gateway` config → `/v1/models` 404s and browser-relay routes still work (covers R4); response contains no provider names/URLs/keys (covers KTD3, R7 posture).

### U2. TypeWhisper section in `apps/docs/pages/desktop.md`

**Files:** `apps/docs/pages/desktop.md`; `packages/server/README.md` one-line pointer if the models endpoint is listed there.

**Approach:**
1. New "TypeWhisper (macOS, Windows, iOS)" section: engine = **OpenAI Compatible** (bundled plugin — no WordInk plugin needed); base URL `http://<host>:8941` **without** `/v1`; API key = device token; model via discovery (works through `/v1/models`) or manual entry — any listed name is fine, the gateway picks the provider.
2. Traps named: don't enter a realtime model name (`gpt-live-transcribe`/`gpt-realtime-whisper` force realtime transport, which the gateway does not serve); translate mode is unsupported (no `/v1/audio/translations`); for a device on another machine set `host` to a LAN/Tailscale address — cleartext, private networks only.
3. Honesty line: compatibility verified against TypeWhisper source (macOS + Windows plugin, iOS changelog); live-device dictation is operator-pending. Update the existing "A dedicated TypeWhisper plugin is planned" line — it is not; the built-in engine covers it.
4. `GET /v1/models` documented in the Responses/endpoints area of the same page.

**Verification:** docs-only — `pnpm --filter docs build` (or the docs e2e spec if nav-adjacent assertions exist); review diff.

## Verification Contract

```bash
pnpm --filter @wordink/server typecheck
pnpm --filter @wordink/server test
pnpm typecheck && pnpm build
# docs: build or targeted e2e for the edited page
```

## Definition of Done

- `/v1/models` answers the OpenAI list shape behind the device-token gate; all U1 test scenarios pass.
- `desktop.md` TypeWhisper section renders with the exact settings and both traps documented.
- Changeset for `@wordink/server` committed; PR stacked on the closeout branch, CI green.
- Plan and brainstorm artifact committed on the branch.

## Scope Boundaries

- **In:** `/v1/models`, TypeWhisper docs section.
- **Out (deferred):** `/v1/audio/translations` (needs provider-capability-aware fallback; Deepgram lacks the endpoint); realtime transport; a marketplace plugin; live-device verification (operator residual — no macOS/Windows/iOS here).
- **Out (never):** provider detail in the models response; new auth mechanisms; changes to the browser relay routes, token store, or fallback router.

## Risks

- **TypeWhisper sends something unexpected on `/v1/models`** (query params, different accept): the response ignores them; the handler only needs path+auth. Low.
- **Docs drift from plugin behavior:** mitigated by stating the source-verified version (plugin 1.1.x lineage) and the operator-check residual, not by pinning to a moving target.
- **Models list used to probe configured providers:** mitigated by R2/R3 — token-gated, neutral `owned_by`, model IDs only (already semi-public: the client picks one and the gateway ignores it).
