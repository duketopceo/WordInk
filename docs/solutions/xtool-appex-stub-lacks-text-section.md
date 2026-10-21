---
title: xtool appex trampoline stub has no __text section — zsign aborts on code-signature free space
date: 2026-10-21
category: build-tooling
module: "apps/ios xtool port"
problem_type: tooling_quirk
component: build_pipeline
severity: medium
applies_when:
  - "Building an app with an extension (appex) via xtool on Linux"
  - "xtool dev build --sign fails with 'Can't find free space of LoadCommands for CodeSignature'"
  - "Inspecting Mach-O load-command padding for code signing"
related_components:
  - "xtool Darwin Swift SDK toolset (ld64.lld wrapper)"
  - "zsign (xtool-bundled fork)"
  - "apps/ios/xtool SwiftPM port"
tags:
  - ios
  - xtool
  - codesigning
  - mach-o
  - linux
---

# xtool appex trampoline stub has no `__text` section — zsign aborts on code-signature free space

## Problem

`xtool dev build --sign` compiled `WordInkRemote` + `WordInkWidgets` cleanly for
`arm64-apple-ios17.0` on aarch64 Linux, then failed at signing with:

```
>>> Can't find free space of LoadCommands for CodeSignature!
Error: Signing failed
```

The main app binary was fine; the failing object was the extension trampoline
`.build/out/Products/Debug-iphoneos/WordInkWidgets-Extension`.

## Root cause

xtool links the appex as a stub executable whose `__TEXT` segment carries only
`__stubs` and `__got` — there is no `__text` section. xtool's bundled zsign
fork (`xtool-org/zsign`, `ZArchO::Init`) computes
`m_uLoadCommandsFreeSpace = __text.offset - sizeofcmds - headerSize`; with no
`__text` section the value stays 0 and `ReallocCodeSignSpace` refuses to append
`LC_CODE_SIGNATURE`. A `__text`-less executable is legal Mach-O — Apple toolchains
sign stubs fine — the check is just zsign's shortcut.

## Solution

Inject an empty `LC_CODE_SIGNATURE { dataoff: filesize, datasize: 0 }` load
command into the stub's padding before zsign sees the file. When the command
already exists, zsign skips the free-space check and writes the real signature
at `dataoff` (= EOF), filling in `datasize` itself.

The patch lives outside the repo as a post-link hook in the Darwin SDK's own
`ld64.lld` wrapper (which xtool already replaces with a shell trampoline for
`-r` object emission):

- `~/.config/swiftpm/swift-sdks/darwin.artifactbundle/toolset/bin/ld64.lld`
  — after `orig/ld64.lld "$@"` succeeds, runs `python3 ld64-patch-signspace.py "$@"`.
- `ld64-patch-signspace.py` — finds `-o <out>` in argv; if the output is a
  Mach-O 64 `MH_EXECUTE` with no `__text` and no existing `LC_CODE_SIGNATURE`
  and ≥16 bytes of padding after the load commands, appends the command and
  bumps `ncmds`/`sizeofcmds`.

Result: both app and appex sign with real CMS blobs (verified 2026-10-21,
xtool 1.21.0, personal team `6BP7W45U55`).

## Prevention / caveats

- The hook is machine-local and dies when the Swift SDK is reinstalled or
  updated — re-apply after `xtool sdk` changes. If `xtool dev build --sign`
  regresses to the free-space error, check the wrapper first.
- `SwiftPM linkerSettings` / `-headerpad` flags do **not** reach xtool's
  app/appex link step — xtool links its own product binaries; patching the
  wrapper is the only injection point that works.
- Upstream fix belongs in xtool/zsign (emit `__text` in the stub, or fall back
  to `__TEXT` segment end for the free-space calc). File it there, not here.
- Adding a `LC_CODE_SIGNATURE` with `dataoff` at EOF is what zsign expects;
  pointing `dataoff` anywhere else produces a signature written over real
  content.
