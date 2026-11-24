# Bridge landing child Report release

The selected DBRIS landing children include TWLT026 and TWLT036. Their actual
ART Report identities are `ExplosionShard` and `Explosion06`; SMOKEY2 has no
Report. Both registered reports are one-shot sounds, without `Control=loop`.
`AnimClass::Destroy4255B0` releases the Report handle at `4255D5` through
`SoundEvent::Release406060` before optional StopSound playback at `425618`.
This leaves a one-shot playing. Calling `StopAndClear405D40` instead cuts it off.

## Executed evidence

`bridge_child_sound.py` executes original `ReadSoundListINI7510D0`,
`FindOrCreate4063B0`, and the complete `VocClass::ReadINI750440` body against
physical SOUNDMD strings. The selected list retains actual keys `280` and `291`
and their source order; the other registry entries are excluded. Original
defaults, constructors, numeric readers, tokenization and registry insertion
execute. Original `AnimType427530/427D00` then binds the three Report fields
against the native-created registry. Fixture-relative numeric sound indexes
are not claims about the full retail registry's absolute indexes.

| Report | Native Control | Native Type | Volume linear | Priority | Limit | Samples |
| --- | --- | --- | --- | --- | --- | --- |
| Explosion06 | 16 | 1056 | 13107 | 2 | 3 | gexp06a |
| ExplosionShard | 18 | 1056 | 4095 | 1 | 2 | gexpshaa, gexpshaa |

For each entry the harness prepares a valid tagged handle and playing event,
then executes original `406060` and `405D40` separately. Release leaves all
event bytes unchanged (state 3, flags 8, serial 41) and clears the handle's
sound pointer. Hard stop changes state to 4, flags to 9, serial to zero and
clears both pointers. No original instruction is replaced. Allocation and
physical INI/archive traversal are supplied boundaries. The exact physical bag
sample names are accepted at `AudioIndex::FindSample4015C0`; sample decoding,
device channels and mixing do not run. The playing fixture has no attached
sample buffers or device channel. Thus this proves the release/stop decision,
not audible waveform equivalence or channel admission under contention.

The SOUNDMD input is 99392 bytes, SHA-256
`0a8e85381aef1a0f97074c953bfe99504da00c6220fae1a023a1afd857023232`, from
`expandmd01.mix` entry `7A742519`. The local retail `ini/soundmd.ini` has the
same identity. ART and SHP provenance are established independently in
[bridge_anim_inputs.md](bridge_anim_inputs.md).
The saved [bag input observations](bridge_child_sound.bag-inputs.json) identify
both selected samples in the winning AUDIOMD audio bag. These are asset header
observations, not native decoding or playback results; no sample duration is
used as a native timer or as a comparison golden.

## Connected behavior and bounds

The production regression
`app::match_runtime::sound_dispatch::bridge_child_sound_tests::retail_landing_child_reports_release_instead_of_cutting_samples`
reads physical retail RULESMD, ARTMD and SOUNDMD. It compares the actual report
identities and all selected sound fields above, including timing/shift fields,
against native results. It constructs each real child, reaches natural expiry,
and checks ordered app publication and the existing sound arbiter's playout
lifecycle. The test's SHP frame counts come from the independent original ART
and image-reader corpus. Native execution does not establish the test's entire
child AI schedule; natural expiry is production composition coverage.

The common correction belongs to Anim destruction and scalar destruction,
which both call native Release. Intentional hard stops remain separate:
sound replacement uses the existing PlayAt interrupt, and Gattling stage-up
and Foot MoveSound stopping keep their existing hard-stop events.
Optional Anim StopSound follows Report release in producer order. No new
simulation event schema, audio RNG, timer, or persistence state is introduced.

A separate Start residual remains: original `424CE0` reaches `405D40` when
Report is absent, whereas current `anim_start` has only the Some(report) arm.
A Next transition from a sounding type to a silent type can therefore leave
the prior cue until final destruction. None of the selected DBRIS, SMOKEY2,
TWLT026 or TWLT036 types has Next; this custom/other-type transition is not
entered by the selected chain and is not certified by the release corpus.

Original selected child AI `423AC0`, Start `424CE0`, Middle `424F00`, Destroy
`4255B0` and scalar destructor `4228E0` were inspected. Actual SMOKEY2 has no
Damage, Scorch, Crater, Next, RandomRate, RandomLoopDelay or particle producer.
TWLT026/036 have Scorch and Crater: at ground-relative height below 30, Middle
enters existing `anim_middle` and `smudge_dispatch` logic, including Scenario
draws, placement and possible Reduce_Tiberium(6). The common native
`anim_middle` and `smudge_can_place` corpora establish those bounded leaves;
the bridge primary-only flight fixture does not execute later child AI or
certify its joined whole-frame RNG/order. Child-specific missing simulation
logic was not established by this inspection. Populated expiry observers and
all custom animation branches remain outside this selected sound proof.

Playback RNG uses native MainRng `886B88` in original `4055C0` and `4047B0`,
not Scenario. These draws and audio service timing are outside this new
execution corpus; the correction reuses the existing audio owner.

## Reproduce

Extract `SOUNDMD.INI` using the existing release asset tool to
`target/asset/bridge-child-sound`; retain the independent ART/SHP extraction
described by `bridge_anim_inputs.md`. Both defaults honor `CARGO_TARGET_DIR`.

```sh
PYTHONPATH=. VERA20K_GAMEMD_EXE=/path/to/gamemd.exe \
VERA20K_BRIDGE_CHILD_SOUND_ASSETS=target/asset/bridge-child-sound/extract \
VERA20K_BRIDGE_ANIM_ASSETS=target/asset/bridge-anim-inputs/extract \
python tools/rules_oracle/bridge_child_sound.py --check
```

The local reproduction uses `/tmp/bridge-debris-child/extract` and
`/tmp/bridge-debris-input-assets/extract`. Generation and an independent
`--check` invocation passed; the parent independently reproduced that check.
Before the production correction, the focused regression failed at actual
TWLT026 natural expiry: ordered app output was AnimationStarted then
AnimationStopped, where the native release result requires AnimationReleased.
The retained failing library binary is `/tmp/bridge-landing-owned-libtests`,
SHA-256 `f82850a9a6458fb997a17c2703a36c5bd9e63ad1ac46108a9a10b69fea251695`;
log `/tmp/bridge-child-sound-meaningful-red.log`. The first attempted run had
only found an uppercase fixture-name comparison and is not the meaningful red.
After correction, the focused Cargo `--lib` regression passed with retail
fixtures required. The retained corrected binary
`/tmp/bridge-child-sound-owned-libtests` has SHA-256
`2c9733ad143f193308faf0f841073e1a2cef53d7d924523f8d58421cd233116b`.
Its connected Anim44, building-art17, audio108, dispatcher13 and debris5
checks all passed (187 total; one existing ignored retail-load test). Logs
are `/tmp/bridge-child-sound-{anim,building-art,audio,dispatch,debris}-green.log`;
the compiled regression log is `/tmp/bridge-child-sound-green.log`.
The parent chain records the release rebuild and production validation results.
