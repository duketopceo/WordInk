---
"@wordink/web": patch
---

`<wordink-mic>` in hold mode now releases when the window loses focus, the tab becomes hidden or pointer capture is lost, so a keyup or pointerup delivered elsewhere no longer leaves the microphone open or blocks the next hold.
