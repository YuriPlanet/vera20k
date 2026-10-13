# Theme storage and score-volume boundaries

The existing duration and retained-or-pending query controls are unchanged.
Run `python -m tools.storage_oracle.sound_theme_metadata --write` to regenerate,
then independently run `--check` against the pinned retail executable. The meta
sidecar records its identity, inputs, executed bodies and transport boundaries.

`save_load_cases` executes `ThemeSave 0x721010`, `ThemeLoad 0x721040` and
`ThemePlay 0x720BB0`. Active `LoadContent` calls ThemeLoad at `0x67F79A`,
after sound/EVA state. Save writes exactly two signed integers: receiver
`+4` retained/Score, then `+8` Pending. Active `+0`, the output stream and
playback position are not in that record. Inputs deliberately disagree between
active and retained to distinguish them. This is a native record witness, not
a claim that the VERA snapshot format is native SAV compatible.

Load reads both words before acting. `InGameMusic` at `0xA8EBA5` gates both
Play(saved retained) and the subsequent inline Queue(saved pending). When false,
this leaf retains the process's previous Theme state. The common next Main_Tick
(`0x55D410`) still owns the separate disabled-music Stop/hold policy. Scores
present, device availability and startup suppression gate Play/Queue through
their existing owner. No fresh `[Basic] Theme=` request belongs in this leaf.
The nineteen controls cover those gates, same-song early return, no stream,
sentinels, two stream read failures, zero volume and previous fade state.

The ordinary load prefix does not cancel Theme's pending request or erase its
fading flag. `559D60` calls session cleanup `69BB40`, whose active branch stops
kind-1 sound events, sample voices, EVA and speech, and resets the separate
`87E758` volume scalar. `LoadContent 67E739` calls `ClearScene 6851F0`; at
`685213..68521F` it resets that scalar and `83D834` to `0x4000`. Scenario defaults
`683610` reset Scenario's own Theme field, subsequently replaced by ScenarioLoad,
not the process Theme at `A83D10`. These are original instruction/caller claims,
not an executed whole-load or arbitrary destructor closure. Movie cleanup and
CD/media-switch paths have separate pause/stop/resume behavior.

The added `disabled_music_fading` and `same_retained_finished_fade` controls
execute the actual ThemeLoad boundary with outgoing fading state intact. The
disabled case preserves it; the finished-fade case must stop and restart even
when the saved retained index equals the outgoing index. The latter exposes
clearing the fading flag before Play, which would incorrectly take its
same-song early return. Resetting the separate scenario volume scalar must
therefore preserve these process Theme fields.

The fixture's eight entries and stream/device booleans are supplied inputs.
Stream stop, play-file and fade-target calls are recorded transports; original
Theme instructions and CRT filename composition execute. Catalog loading,
music decoding, physical playback and time-dependent interpolation are outside
these controls. In particular, these fixtures do not change the explicit VERA
RA2 skirmish art/music policy.

`score_volume_cases` executes the original constructor `0x720960`, complete
Options setter `0x5FA4A0`, both original `Math::ftol 0x7C5F00` conversions and
Theme setter `0x721290`. Constructor `+0xC` is 255. The Options setter caps only
the upper side at 1.0, stores f32 `Options+0x40`, sends a separate `*16384`
stream scalar, and derives Theme's signed integer volume from that stored f32
times the original double 255.0. The INI reader at `0x5FAAE6` and common options
apply at `0x5FB251` call the same Theme setter after this conversion. Sixteen
native controls include adjacent f32 inputs across its first positive bucket,
negative/zero/default/full values, upper clamp and nonfinite inputs; outputs
retain raw f32 bits. Nine `volume_setter_cases` execute its signed upper-only
integer clamp independently. No rounding output comes from hand calculation.

Play checks the integer volume after admission, its same-song early return,
inline Stop/reset and sentinel handling. At `0x720C8E`, nonpositive volume
branches to Pending=request without writing retained or starting a stream.
AI checks the same signed volume earlier (`0x7209FF`), before settling a fade,
querying the stream, consuming Pending or choosing a random song. Four
`ai_volume_cases` execute that subsequent AI visit with no stream and a real
pending index; nonpositive volume preserves it, while positive volume consumes
it through Play and then writes the automatic sentinel. This bounded visit
does not enter NextSong or establish a whole-load Main RNG history.

Four `settle_cases` continue the same storage fixture through original
Still_Playing `720FD0`, Queue `720B20`, Stop `720EA0` and AI `7209D0`.
Disabled ThemeLoad first retains the supplied outgoing fade; interpolation is
then supplied as finished. Query stops the stream and returns its updated
not-playing state; Queue writes pending 7, Stop clears the slots, and AI retains
the hold sentinel. Each leaves Theme's fading byte set. Settling does not clear
that byte: Play's later reset owns the clear at `720C6D`. The output action and
the subsequent playing observation must therefore share one owner. These rows
do not execute elapsed interpolation; their real pending/hold inputs avoid
NextSong, and the supplied Main bytes remain unchanged.
