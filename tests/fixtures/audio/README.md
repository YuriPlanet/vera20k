# Audio fixtures

`sine440.opus`: 0.2 s of a 440 Hz sine as mono Ogg Opus (48 kHz, pre-skip 312,
final granule 9912, so 9,600 samples of audio). Made with

```
ffmpeg -f lavfi -i sine=f=440:d=0.2 -c:a libopus -b:a 16k sine440.opus
```

(ffmpeg 7.1, libavcodec 61.19.101). Read by `assets::opus_file` and the audio
override tests.
