# Opus sounds and music

VERA plays an Ogg Opus file in place of a sound or a music track of the same
name. gamemd has nothing like it: it plays WAV and AUD files and the IMA ADPCM
sounds of `audio.bag`. A package can use this to make the game's audio much
smaller (a "miniHD" install drops `audio.bag`/`audio.idx` and the music
archives and ships every sound as `.opus`), and a mod can use it to replace a
few sounds without building a new `audio.bag`.

The original game has no `.opus` files, so with retail data nothing changes:
the same files are read and decoded, the same samples play.

## Naming

The override of a name is that name with `.opus`; a name ending in `.wav` or
`.aud` loses that extension first.

| Sound | Plays from |
|---|---|
| a `Sounds=` sample in `soundmd.ini`, such as `igisea` (unit voices, effects and EVA) | `igisea.opus` |
| a `[Themes]` track in `thememd.ini`/`theme.ini` with `Sound=Drok` | `Drok.opus` |
| a country taunt, `taunts\tauam01.wav` | `taunts\tauam01.opus` |

The file goes where the game finds its other files: loose in the game folder
or the mod's folder (a taunt in its `Taunts` folder), or inside any MIX
archive the game mounts. Names are not case sensitive. An override wins over
the original file; the game looks for it once per name.

## The files

Ogg Opus as `opusenc` or `ffmpeg -c:a libopus` writes it, mono or stereo.
Surround streams are not played. The game decodes it at 48 kHz, Opus's own
rate, applies the header's output gain, and trims the encoder delay and the
end padding, so a looping sound loops without a gap. For example:

```
ffmpeg -i igisea.wav -c:a libopus -b:a 48k igisea.opus
```

A sound's pitch shift (`FShift=`) and volume work as for the original sample.
When a sound plays several of its samples back to back in one playout, each
plays at the first one's rate, as the original does with samples of mixed
rates, so replace either all of a sound's samples or none of them.

## Without audio.bag or THEME.MIX

Without `audio.idx`/`audio.bag` (neither `audiomd.mix` nor `audio.mix`),
every `Sounds=` name in `soundmd.ini` is kept and plays from its `.opus` file
(or from a loose WAV or AUD of that name, as before). A sample with neither
plays nothing. With the bag present, a `Sounds=` name must still be in it to
be kept, as in the original; its `.opus` then plays instead of the bag's
sample.

Without the music archives, a `[Themes]` entry whose `<Sound>.opus` exists is
available, plays, and lists its length in the music list. The installer check
counts a track's `.opus` as present, and does not report the missing sound
bank when the sounds ship as `.opus`.

## Custom music

The player's own music in the profile's `musics/` folder (switched on by
`musics/music.ini`, `enable=yes`) may hold `.opus` files beside `.wav` and
`.aud`.

## Online play

Sound is presentation only: what plays, and the draws it makes for random
samples, never reach the simulation, so players with and without overrides
play together.
