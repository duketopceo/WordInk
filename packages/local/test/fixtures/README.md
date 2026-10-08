# Test fixtures

`hello-world.wav`: "Hello, world." as 16 kHz mono PCM16, about 2.5 s. Synthesized locally with
eSpeak NG 1.52.0 (no third-party recording, so no license applies beyond this repo's MIT):

```bash
espeak-ng -v en-gb -s 130 -w hw.wav "Hello, world."
ffmpeg -i hw.wav -af "adelay=300,apad=pad_dur=0.4" -ar 16000 -ac 1 -sample_fmt s16 hello-world.wav
```

Moonshine tiny (q8, WASM) transcribes it as "Hello, world.". Some other eSpeak voices come out as
"The long world." or nothing, so keep this voice if you regenerate it.
