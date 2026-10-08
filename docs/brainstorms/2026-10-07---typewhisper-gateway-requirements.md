# TypeWhisper ↔ wordink-gateway — Requirements

**Date:** 2026-10-07
**Tier:** Lightweight
**Origin:** `docs/plans/2026-10-05-1131-feat-wordink-desktop-gateway-plan.md` `work-relationships` — "TypeWhisper plugin" was the tentative next area; research resolved its open question.

## Problem

The gateway plan anticipated a dedicated TypeWhisper plugin. Code-reading `TypeWhisper/typewhisper-mac` shows that is unnecessary: the bundled **OpenAI Compatible** engine (`TypeWhisperPluginSDK/Plugins/OpenAICompatiblePlugin`) already accepts a custom base URL + API key and speaks exactly the gateway's contract — `POST {baseURL}/v1/audio/transcriptions`, `Authorization: Bearer`, multipart `file` + `model` + `response_format=json` + optional `language`/`prompt`, compressed-M4A upload with WAV fallback, expects `{text, language?, segments?}` back. The same plugin ships on Windows (`typewhisper-win/plugins/TypeWhisper.Plugin.OpenAiCompatible`) and iOS 0.3.0+.

So this leg is not a plugin. It is the two things TypeWhisper still can't do against the gateway today:

1. **Model discovery** — the plugin's model picker calls `GET {baseURL}/v1/models`; the gateway only serves `/v1/audio/transcriptions`, so it 404s. Manual model entry works, but discovery should populate the picker with the models the gateway will actually call.
2. **Docs** — a dedicated section: which engine to pick, the exact base-URL form (no `/v1` — the plugin appends it), the device token as API key, and the two unsupported modes (translate, realtime).

## Scope decision (user-settled 2026-10-07)

**Docs + `GET /v1/models`.** Chosen over docs-only (model discovery is the one visible papercut — ~30 lines + tests closes it) and over the full compat surface (`/v1/audio/translations` can't be served by Deepgram entries at all, so it would carry partial-fallback semantics for a mode TypeWhisper only calls when translate is enabled — deferred, documented as unsupported).

## Requirements

- **R1:** A TypeWhisper user can configure the bundled OpenAI Compatible engine to dictate through `wordink-gateway` using only the docs page: engine = "OpenAI Compatible", base URL = `http://<gateway-host>:<port>` with **no `/v1` suffix** (the plugin appends `/v1/...` itself), API key = a `wdk_` device token, model chosen from discovery or entered manually.
- **R2:** `GET {basePath}/v1/models` answers `200` with the OpenAI list shape `{object:"list", data:[{id, object:"model", created, owned_by}]}`, one entry per configured provider's *effective* model (`entry.model ?? DEFAULT_MODELS[provider]`). It reveals only model IDs — never provider names, keys, URLs, or vocabulary.
- **R3:** `/v1/models` is behind the same device-token gate as transcriptions: missing/invalid `Bearer` → OpenAI-shaped 401. It is rate-limited by the same per-token limiter.
- **R4:** When `gateway` is not configured, `/v1/models` 404s like the transcriptions path — no new surface in browser-relay-only mode.
- **R5:** `desktop.md` gains a TypeWhisper section: settings walkthrough, batch-transport note (auto is safe for the discovered model names; don't enter `gpt-live-transcribe`/`gpt-realtime-whisper` — those force realtime), translate mode documented as unsupported, and remote-access caveat (gateway binds loopback by default; LAN/Tailscale needs an explicit bind).
- **R6:** The docs section states parity evidence honestly: macOS + Windows verified against plugin source, iOS per the vendor's changelog; live dictation remains an operator check — no macOS/Windows/iOS device exists in this workspace.

## Actors

- **A1 — desktop app operator:** configures TypeWhisper's engine settings; holds only a device token.
- **A2 — gateway operator:** runs `wordink-gateway`, configures providers and vocabulary, issues/revokes `wdk_` tokens.

## Key flows

- **F1 — configure + dictate:** A1 installs TypeWhisper → Settings → engines → OpenAI Compatible → base URL `http://127.0.0.1:8941` → API key `wdk_…` → model list populates from `/v1/models` → dictate → audio POSTs as multipart → gateway fallback chain → `{text}` → text pastes.
- **F2 — model discovery:** the plugin GETs `/v1/models` with the token → the picker lists the effective provider models.
- **F3 — revoked token:** transcription and models both answer 401 → the plugin surfaces invalid-api-key.

## Acceptance examples

- **AE1:** `curl -s http://127.0.0.1:8941/v1/models -H "Authorization: Bearer $WDK"` returns the configured models in OpenAI list shape; the same request with a revoked/garbage token returns the same 401 the transcriptions route emits.
- **AE2:** With `gateway` unset, `GET /v1/models` → 404; CORS/Origin/`authorize` logic is untouched (token gate only).
- **AE3:** The docs page's TypeWhisper section renders with base URL, token, and model guidance; verified-by-source compatibility is stated and live-device verification is marked operator-pending.

## Scope boundaries

- No branded WordInk plugin for TypeWhisper's marketplace — functionally unnecessary and unbuildable in this workspace (compiled Swift `.bundle`, needs Xcode/macOS).
- No `/v1/audio/translations` — documented unsupported (Deepgram can't serve it; partial-fallback semantics deferred).
- No realtime/streaming transport — gateway is batch-only; docs steer to batch-safe model names.
- No changes to the browser relay, token store, or fallback router semantics.

## Open residuals (operator)

- Live dictation on a real TypeWhisper device (macOS/Windows/iOS) — none exists in this workspace.
- If a future pass wants `/v1/audio/translations`, it needs provider-capability-aware fallback design first.
