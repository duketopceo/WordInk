# Concepts

> Shared domain vocabulary for this project — entities, named processes, and status concepts with project-specific meaning. Seeded with core domain vocabulary, then accretes as ce-compound and ce-compound-refresh process learnings; direct edits are fine. Glossary only, not a spec or catch-all.

## Gateway

The OpenAI-compatible transcription endpoint `@wordink/server` exposes for desktop dictation clients: `POST /v1/audio/transcriptions`. It accepts the OpenAI multipart fields and answers in the OpenAI response and error shapes, never revealing which upstream provider served a request. It is a second trust boundary next to the browser relay: the relay authenticates browsers with cookies and an Origin check, while the gateway authenticates devices with bearer tokens — the two must not be conflated.

### Device token

A per-device bearer credential (`wdk_…`) a desktop client sends as its OpenAI "API key". Only hashes are stored; the token itself is shown once at creation and can be revoked per device, taking effect on the device's next request. Long-lived provider keys stay on the gateway and never reach a device.

### Provider entry

One item of the gateway's ordered fallback list: a provider name plus the name of the environment variable holding its key (`keyEnv`), with optional model and URL overrides. Config names env vars, never key values — keys resolve at serve time and entries without a usable key are skipped rather than serving.

### Cooldown

The router's per-entry skip window after a retryable provider failure (rate limit, 5xx, timeout, unreachable, rejected key). A cooling entry is skipped for later requests unless every entry is cooling, in which case the one whose cooldown ends first is tried. Client-caused failures and attempts cut short by the shared deadline earn no cooldown — they prove nothing about the provider.
