# Read the target app's bundled "OpenAI compatible" engine before building an adapter

**Date:** 2026-10-07 · **Track:** knowledge (`best-practices`)

When a plan calls for integrating with a third-party app ("write a plugin for X", "add an adapter for Y"), the assumed work — a plugin, an SDK package, a marketplace listing — often presumes the app *can't* already speak your protocol. Desktop dictation apps, LLM front-ends, and similar tools increasingly ship a bundled **"OpenAI compatible"** or **"custom endpoint"** provider that takes a base URL and API key. If it exists, the integration collapses from a plugin project to documentation plus small compat endpoints.

## The check

Before scoping an adapter, fetch the app's bundled custom-provider source and pin four things:

1. **Request shape** — exact path, method, auth header, body encoding (`multipart` fields and their names). TypeWhisper's `OpenAiCompatiblePlugin.swift` posts `file`/`model`/`response_format`/`language`/`prompt` to `{baseURL}/v1/audio/transcriptions` — no WordInk plugin needed because the gateway already serves that contract.
2. **Response tolerance** — which fields the decoder *requires* vs tolerates (TypeWhisper needs only `text`; `language`/`segments` are optional).
3. **Auxiliary calls** — does it probe `GET /v1/models` for a picker? Does a mode switch (translate, realtime) hit paths you don't serve? Each miss is either a docs note ("don't enable translate") or a small endpoint (`/v1/models`), not a plugin.
4. **Base-URL semantics** — does the client append `/v1/...` itself (enter the host root) or expect it in the URL? Getting this wrong is the #1 support failure for OpenAI-compatible endpoints; document the exact string to paste.

## Why it pays

- Kills phantom projects: the planned "TypeWhisper plugin" would have been a Swift/Xcode `.bundle` — unbuildable on a Linux-only workspace and functionally redundant.
- Converts unverifiable work into verifiable: compatibility-by-code-reading is checkable without owning the device; live dogfooding remains a residual either way.
- Applies anywhere "plugin" is assumed: LLM tools, dictation apps, agent frameworks — check for the generic OpenAI-compatible surface first.

## The residual it does NOT remove

Reading the client proves the contract, not the runtime: TLS quirks, UI validation (`validate()` requiring non-empty fields), and version drift still need a live check on real hardware — record it as an operator residual rather than claiming the integration verified.
