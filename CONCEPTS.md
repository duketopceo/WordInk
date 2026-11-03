# Concepts

> Shared domain vocabulary for this project — entities, named processes, and status concepts with project-specific meaning. Seeded with core domain vocabulary, then accretes as ce-compound and ce-compound-refresh process learnings; direct edits are fine. Glossary only, not a spec or catch-all.

## Gateway

The OpenAI-compatible transcription endpoint `@wordink/server` exposes for desktop dictation clients: `POST /v1/audio/transcriptions`. It accepts the OpenAI multipart fields and answers in the OpenAI response and error shapes, never revealing which upstream provider served a request. It is a second trust boundary next to the browser relay: the relay authenticates browsers with cookies and an Origin check, while the gateway authenticates devices with bearer tokens — the two must not be conflated.

### Device token

A per-device bearer credential (`wdk_…`) a desktop client sends as its OpenAI "API key". Only hashes are stored; the token itself is shown once at creation and can be revoked per device, taking effect on the device's next request. Long-lived provider keys stay on the gateway and never reach a device. Every token has a scope; a record written before scopes existed resolves to `device` (fail closed).

### Admin token

A device token minted with `tokens create --admin`. Admin is a superset of device scope: it transcribes and lists models like any device token and additionally reaches the vocabulary routes (`GET|POST|DELETE /v1/vocabulary`), which answer `403 insufficient_permissions` to device tokens. Intended for agents and operators, not daily dictation clients.

### Runtime vocabulary

Terms appended to the gateway's `vocabulary` at runtime via `/v1/vocabulary`, persisted to `$XDG_STATE_HOME/wordink/vocabulary.json` with provenance (`addedBy`, `addedByTokenId`, `addedAt`, plus tombstone fields for deletes). The effective set is the union: config terms first (operator-owned, `pinned`, undeletable through the API), then runtime terms in insertion order. A runtime case-variant of a config term is a spelling override (`config_shadowed`): the runtime record wins the union's casing while presence stays config-owned — deleting the override reverts to the config spelling. Mutations take effect on the next transcription; a corrupt file degrades transcription to config-only rather than taking dictation down.

### Provider entry

One item of the gateway's ordered fallback list: a provider name plus the name of the environment variable holding its key (`keyEnv`), with optional model and URL overrides. Config names env vars, never key values — keys resolve at serve time and entries without a usable key are skipped rather than serving.

### Cooldown

The router's per-entry skip window after a retryable provider failure (rate limit, 5xx, timeout, unreachable, rejected key). A cooling entry is skipped for later requests unless every entry is cooling, in which case the one whose cooldown ends first is tried. Client-caused failures and attempts cut short by the shared deadline earn no cooldown — they prove nothing about the provider.

## Desktop app

The one WordInk dictation app for end users (`apps/desktop`, based on Hex, MIT): a hotkey, a recording indicator, and text typed into the focused app, on Linux (any Wayland desktop), Windows and macOS on Apple silicon. Groq is the default engine; Parakeet v3 is the local, no-key option. It is released as 1.0.0. It calls providers directly with the user's own key and does not need the gateway.

## wordink-dictd

The Python dictation daemon in `sdks/python` that works today on Linux, Windows and macOS. It is the interim desktop client: it is kept on each OS until the desktop app reaches parity there, then retired. It uses Groq Whisper and has a doctor command for diagnosing setup. Not a host of the Rust core.

### Hold bind and toggle bind

The two hotkeys `wordink-dictd` registers. Hold: record while the key is down, transcribe on release. Toggle: tap to start, tap again to stop. On Linux the compositor runs `wordink-dictd trigger start|stop|toggle`; on Windows and macOS the daemon registers the chords itself.

### Clean-up

The post-transcription step that turns raw speech into finished text: filler removal, course corrections ("no wait"), spoken lists, snippet expansion, tone presets and vocabulary learning. The same contract is kept in the gateway (`flow.ts`), the Python tooling and the desktop app, checked by `tests/fixtures/flow-contract.json`. Also called flow.

### Snippet

A spoken trigger that expands to fixed text during clean-up, for example "my calendly" to a booking link.

### Injector

The part of a desktop client that puts the finished text into the focused window: typed keystrokes where the platform allows, with a clipboard-paste fallback.

### Meeting mode

Long-form recording with speaker labels and a markdown export. It exists in `wordink-dictd`; the desktop app takes it from Hex's meeting runtime plus the proven gaps from `wordink-dictd`.

### Core budget

The size limit for the browser WebAssembly core: 150 KB gzip. The current build is about 35 KB gzip.
