# Desktop surfaces ladder: requirements

Date: 2026-10-10 · Status: settled · Tier: Product

## Origin

Phase 2 (Oct 2026) decided integration over a standalone app — TypeWhisper and
Voxtype already own dictation UX on their platforms, and WordInk's wedge is the
SDK + gateway. That decision still holds for the *capture* surface: we do not
compete with TypeWhisper on Windows/macOS.

A product brainstorm re-opened the question with a different shape. Three
forces:

- **Voxtype works but is invisible.** It is a daemon plus `config.toml`; there
  is no GUI gap-filler on the one platform (Linux/Omarchy) where dictation apps
  are weakest.
- **The gateway has surfaces nobody else can build.** Provider fallback,
  per-device `wdk_` tokens, shared vocabulary, and usage/rate-limit data are
  all server-side — TypeWhisper/Talkify assume each device configures its own
  provider key.
- **Agents are a new class of consumer.** The vocabulary correction loop —
  dictate → transcript mishears "kurultai" → agent adds the term → re-dictate →
  fixed — is a workflow only WordInk can offer, because only WordInk has shared
  server-side vocabulary. Today that vocabulary is a static array in
  `~/.config/wordink/gateway.json` requiring manual edits + service restart.

The brainstorm rejected a TypeWhisper-scale app (competes with our own
integration targets, duplicates mature platform work, dilutes the SDK story)
and rejected adopting `omarchy-voxtype-enhance` (a 0-star sketch; reference,
not foundation). What emerged is a **ladder**: three rungs, each independently
shippable, each de-risking the next.

## Decision

### Rung 1 — Omarchy bar plugin

A first-party Omarchy shell plugin that turns voxtype+gateway into a visible
product. (Omarchy 4.x runs Quickshell, not Waybar — plugins declare
`kinds: ["bar-widget"]` in a `manifest.json` and live under
`~/.config/omarchy/plugins/`; the user's `omarchy-plugins` monorepo is the
first-party home. Voxtype already emits state to `$XDG_RUNTIME_DIR/voxtype/state`
and `voxtype status --format json --extended`.)

- Dictation state (idle / recording / transcribing) from voxtype.
- Gateway health + active provider from the gateway.
- Today's usage vs quota where a quota applies.
- Click-through panel with start/stop dictation (`voxtype record toggle`),
  provider display, last transcript, and a jump to settings.

### Rung 2 — Gateway control surface + agent vocabulary loop

Two faces of the same new API surface.

**Runtime vocabulary API** (new gateway endpoints):

- `GET /v1/vocabulary` → current term list.
- `POST /v1/vocabulary` → add term(s), takes effect on the next transcription
  (no restart).
- `DELETE /v1/vocabulary` (JSON body `{"term": …}`) → remove. The plan moved
  deletion off a parameterized path — percent-encoding on arbitrary terms is a
  bug class, and the collection path keeps dispatch exact-match (plan KTD13).
- Persists to the operator config (or a sibling `vocabulary.json` the gateway
  owns), surviving restart.
- Auth: admin-scoped token (distinct from device tokens — a device token must
  not be able to edit operator vocabulary), or loopback-only initially.

**Agent surface**:

- `wordink vocab list|add|remove` in `sdks/python` CLI.
- `vocab_list` / `vocab_add` / `vocab_remove` tools in `wordink-mcp`.
- The correction loop: agent reads a bad transcript → `vocab_add("kurultai")`
  → re-dictate or re-transcribe → verify. Personal fine-tuning without a
  restart and without touching `gateway.json` by hand.

**Control surface (human face)**: a small web UI (docs-site stack /
`<wordink-*>` components) over the same API plus status data:

- Providers + health/fallback order.
- Device tokens: issue, revoke, last-seen, per-device usage.
- Vocabulary editor (same runtime API).
- Usage stats vs quota.

### Rung 3 — Lightweight WordInk desktop app

A first-party minimal dictation app — the SDK's reference host, not a
TypeWhisper clone:

- **Three OS, Linux first.** Wayland/Hyprland is the riskiest input path and
  is already proven on this machine (voxtype's route: compositor/evdev hotkey
  + `zwlr_virtual_keyboard_v1` injection). Windows (`RegisterHotKey` +
  `SendInput`) and macOS (CGEvent tap + AX insertion) follow; both are
  routine.
- **Full hotkey → type anywhere.** Push-to-talk/toggle, text injected at the
  caret in the focused app, clipboard fallback where injection isn't
  available.
- **Groq-speed, local-free.** Cloud path rides the gateway (Groq LPU latency
  is the pitch); the free/no-key path is `@wordink/local`'s moonshine wasm
  engine — already in-repo.
- **Post-processing.** Optional Groq LLM cleanup/correction pass — server-side
  in the gateway (matches the key-safety invariant; `legacy/` precedent).
- **Settings + stats shell.** Mic, hotkey, gateway URL/token, post-process
  toggle, language, vocabulary view, usage vs quota. No history browser, no
  workflows, no marketplace — that is TypeWhisper's territory.
- **Hosted trial + BYOG.** Works out of the box against a hosted WordInk
  gateway (~2h/day free quota is the concept; enforcement/auth model is an
  open question below), or point at your own gateway.
- **Positioning:** "speed, simple, yours." Reference host that dogfoods the
  SDK end-to-end — it makes "embed us" credible, and it is the only part of
  the ladder that is a conventional product.

## Actors

- **A1** — Omarchy/Linux user wanting dictation that looks like a product, not
  a daemon (rung 1, rung 3).
- **A2** — agents (wisp, kurultai, Devin, Claude Code) curating vocabulary and
  transcribing via MCP/CLI (rung 2 agent surface; the correction loop).
- **A3** — gateway operator managing providers, device tokens, vocabulary,
  usage (rung 2 control surface).
- **A4** — desktop users on Linux/Windows/macOS wanting a lightweight
  dictation app, either free local or hosted-trial (rung 3).
- **A5** — app developers evaluating the SDK; the app is proof it works
  (rung 3 as reference host).

## Requirements

- **R1.1** — Waybar module reads voxtype state and gateway health without
  breaking either service; degrades to "off" when either is unreachable.
- **R1.2** — Panel exposes dictation state, provider, today's usage, last
  transcript, and a config jump; no settings mutation in v1.
- **R2.1** — `GET/POST/DELETE` vocabulary endpoints mutate the live router's
  term list (affects the next request, not just post-restart) and persist.
- **R2.2** — Vocabulary mutation is admin-scoped; device tokens can transcribe
  but cannot edit.
- **R2.3** — `wordink vocab list|add|remove` + MCP tools wrap R2.1.
- **R2.4** — Control surface covers providers, tokens, vocabulary, usage; v1
  is loopback/operator-only (no public auth).
- **R3.1** — Hotkey → inject works on Wayland/wlroots first (proven path),
  with clipboard fallback; Windows and macOS follow.
- **R3.2** — App talks only to a gateway (hosted default or BYOG); local
  moonshine is the offline/no-key engine — no direct provider keys in the
  app.
- **R3.3** — Optional server-side LLM post-processing pass, user-toggleable.
- **R3.4** — Settings + stats shell as scoped above; explicitly no history
  browser, workflows, or plugin marketplace.
- **R3.5** — Hosted trial concept: ~2h/day free transcription per device;
  auth model and quota enforcement are open questions (below), not settled
  here.

## Key flows

- **F1** (agent vocab loop) — agent gets transcript with "karate eye" →
  `vocab_add "Kurultai"` → re-dictate → "Kurultai" → done. No restart.
- **F2** (Omarchy) — user sees mic state in the bar, clicks for provider /
  usage / last transcript; dictates via existing voxtype hotkey.
- **F3** (operator) — opens control surface, issues a `wdk_` token for a new
  laptop, revokes an old phone's, checks Groq health, edits vocabulary.
- **F4** (rung 3) — user installs app on a fresh Linux box, holds hotkey,
  speaks, releases, text lands at the caret; on Windows/Mac the same flow
  post-port.

## Acceptance examples

- **AE1** — `wordink vocab add kurultai` then `wordink dictate` immediately
  recognizes the term, without a gateway restart; `gateway.json` (or
  `vocabulary.json`) shows the term persisted.
- **AE2** — Waybar module shows recording state during a voxtype session and
  "gateway down" when the service is stopped.
- **AE3** — MCP `vocab_add` call from a host takes effect on the next
  `transcribe` call.
- **AE4** — control surface issues a token; that token transcribes
  successfully and is revocable mid-session.
- **AE5** — app on Hyprland: hotkey press → speak → release → injected text
  in a focused terminal; clipboard fallback when injection is refused.

## Open questions (not settled — plan-level)

- Hosted-trial auth: anonymous per-device token auto-minted at first launch
  vs a light account. Quota enforcement point (gateway-side counter vs
  app-side).
- Post-processing default: on, off, or per-hotkey ("hold = raw, tap =
  cleaned").
- App naming/branding: "WordInk" the app vs a distinct name keeping the SDK
  brand clean.
- Whether vocab scoping eventually goes per-device or per-context (repo,
  project); v1 is operator-global.

## Out of scope

- Competing with TypeWhisper on Windows/macOS feature depth — the app stays
  thin on purpose.
- Plugin marketplace, workflow builder, history browser, snippets manager.
- Mobile (Android/iOS) — unchanged from Phase 4 deferral.
- Local inference *in the gateway* (server-side whisper.cpp hosting) — the
  local engine stays client-side (`@wordink/local`).
- npm/PyPI publishing — still gated on `NPM_PUBLISH_ENABLED` / operator.
