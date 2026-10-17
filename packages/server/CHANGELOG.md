# @wordink/server

## 0.2.0

### Minor Changes

- cb04d8f: The gateway gains a runtime vocabulary API — `GET|POST|DELETE {basePath}/v1/vocabulary` — so agents and operators can add, respell and remove shared ASR hint terms without editing `gateway.json` or restarting. Device tokens grow a scope (`device`|`admin`; admin is a superset that also transcribes): `tokens create --admin <label>` mints the admin token the vocabulary routes require, while a plain `tokens create` keeps minting device tokens. Runtime terms persist with provenance in `$XDG_STATE_HOME/wordink/vocabulary.json` (atomic 0600 file, tombstones for deletes) and join the config `vocabulary` on the next transcription — config terms first, runtime terms in insertion order, with a runtime case-variant acting as a spelling override that stays config-pinned. `VocabularyStore`/`VocabularyStoreError` and `FileVocabularyStore` are exported for embedders (`@wordink/server` and `@wordink/server/node` respectively); `GatewayConfig.vocabularyStore` opts the routes in.
- 0ca399e: Add Wispr Flow deterministic post-processing (`stripFillers`, `resolveCourseCorrections`, `formatSpokenLists`, `expandSnippets`, `applyTonePreset`, `applyFlowCleanup`, `buildFlowCleanupPrompt`), post-insertion dictionary edit learning (`learnVocabularyFromEdit`), optional `GatewayConfig.flow` transcription cleanup, and the `WordInkIOS` Swift package (`sdks/ios/`).

## 0.1.0

### Minor Changes

- 10a10af: The desktop gateway now also serves `GET {basePath}/v1/models` — the OpenAI model-list shape, behind the same `wdk_` device-token gate and per-token limiter as transcriptions. It reports each configured provider's effective model, so OpenAI-compatible clients with model discovery (e.g. TypeWhisper's bundled "OpenAI Compatible" engine) populate their picker instead of requiring a manual model id. `GATEWAY_MODELS_PATH` is exported next to `GATEWAY_TRANSCRIPTIONS_PATH`.
- 10a10af: Desktop gateway: an OpenAI-compatible `POST /v1/audio/transcriptions` route, enabled with the new `gateway` option. It adds provider fallback with cooldowns, per-device bearer tokens and server-side vocabulary. A `wordink-gateway` CLI serves it locally (`serve`, `check`, `tokens create|list|revoke`). Desktop dictation apps such as Voxtype can use WordInk as their backend.
- 10a10af: First release of `@wordink/server`: a fail-closed credential relay for WordInk. It forwards Groq audio and mints short-lived OpenAI and Deepgram tokens, so long-lived provider keys never reach the browser. Adapters for Cloudflare Workers and Node.

### Patch Changes

- 10a10af: Reject requests whose `Origin` is neither the relay's own origin nor in `allowedOrigins` with `403 origin_not_allowed` before `authorize` runs, so a cross-site page can't spend provider quota with a signed-in user's cookie (CSRF). Cross-origin cookie deployments must list the app's origin in `allowedOrigins`. Upstream provider calls now time out after `upstreamTimeoutMs` (default 15 000 ms) and answer `502 upstream_unreachable`, including a Groq reply whose body stalls after its headers.

  The Node adapter now builds the request URL from its own origin plus the request-target as a path, so a protocol-relative target like `//evil.com/openai/token` can no longer pose as the attacker's origin. Its origin is `https` on a TLS socket, and with `trustProxy: true` it comes from `X-Forwarded-Proto` and `X-Forwarded-Host` (ignored otherwise), so same-origin HTTPS requests pass the `Origin` check.
