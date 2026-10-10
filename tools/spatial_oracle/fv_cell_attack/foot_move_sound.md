# Idle SQD MoveSound and handle lifetime

`move_sound --foot-tail` composes the existing constructor-built naval fixture,
Foot body-counter slice, original sound registry/readers, tagged handle fixture
and raw-load boundary. It executes the selected original `Foot::AI`
`4DA806..4DAB3C`, including the saved body counter, body-counter update and
MoveSound tail. It does not execute whole Foot/Unit AI or held Ship movement.

The fixture binds unchanged physical SQD data through original readers:
`RULESMD.INI`, absent `LANGRULE.INI`, `MPBattleMD.ini`, then the inner `Hills.map`
INI. The inherited naval fixture's other selected prerequisites retain their
declared XShrapnel input boundary. `SOUNDMD.INI` is fixed, with the original
SoundList keys for `SquidMove` and `GenLargeWaterDie`. Registry indices are local
to this subset; retained names identify the sounds. The metadata and payload
pin the executable, imported Python sources and every physical input read.

## Executed boundaries

The original TechnoType constructor leaves WalkRate 1, IdleRate 0 and an empty
MoveSound vector. The selected readers run WalkRate then IdleRate at
`712222..712256`, followed by MoveSound at `713459..7134D9`. Their retained
defaults, signed integers, list replacement and 128-byte string buffer execute
natively. Separate authored catalog controls register both `none` and `<none>`:
the original list resolver reserves only `<none>`, case-insensitively. The
physical catalog and authored controls remain distinct.

Stock SQD reads WalkRate 2, IdleRate 4 and `SquidMove`. The original Ship
constructor and `IsMovingNow69F330` produce the stationary answer. Ship
`Process69FC10` is a declared no-movement RET4 boundary. Counter-delta and fresh
IsMovingNow controls execute the actual tail; explicit changing callback
answers are separate supplied inputs. DirectRocker `Foot+2A8` and Pushy
`Type+692` contrasts supply their retained fields only, without proving the
reciprocal-link mechanism that creates or clears them.

On the first qualifying stock visit, the original tail calls hard stop
`405D40`, draws Main RNG `65C780`, selects the ordered vector slot by original
unsigned division, calls `7509E0`, sets the active latch and writes countdown 3.
It latches even when original `7509E0` takes the supplied playback-disabled
gate. Later qualifying counter updates refresh the countdown without another
draw or start. Nonqualifying active visits decrement the original DWORD,
including its signed wrap; inactive visits retain it. An empty list still gets
the qualifying countdown write. Main, Scenario and MapGen whole states bracket
each visit. Later audio-service RNG and actual sound output are not implied.

The accepted-playback timelines attach the shared tagged event boundary. They
measure the caller and handle lifecycle, not decoder, channel or device output.
An authored IdleRate 5 timeline exposes lapse/restart without inventing a stock
Stop-command release: stock idle SQD keeps refreshing its latch.

The additive `stock_idle_frame_zero` timeline supplies `start_frame=0` and
`frame_count=16` through the same producer. Original `4DA983` reads frame
`A8ED84`, `4DA989` divides it by IdleRate, and the qualifying path increments
the body counter at `4DA9FB`. The frame-zero visit executes those instructions
and the joined sound tail; no startup exception or calculated counter delta is
supplied. It changes body counter 0 to 1, sets the latch and countdown 3, and
makes one Main draw; Scenario and MapGen remain unchanged. This remains an
already-live SQD boundary, not native scenario frame initialization or whole
first-frame AI.

Production observations use committed frame boundaries: `SimRuntime::advance_frame`
calls the shared frame transaction, whose live object pass precedes the late
`binary_frame` increment. Thus L0 is before AI at frame 0; completed step 1
records frame 1 after executing frame-0 work. Compare a native visit's
`after` state to that committed observation, not to a visit at its displayed
frame number. The five earlier timelines retain their explicit frame inputs.

## Cleanup and queued playback

The tagged controls execute full `405D40`, `405FD0`, `406060`, the selected
FootLimbo suffix `4DB34D..4DB358`, and Foot destructor suffix
`4D366E..4D3683`. A valid one-shot Release leaves the event running and clears
the handle's sound pointer. Limbo's `405FD0` requests decay by setting flags
`0x60` for every valid event, independent of Control/LoopCount, and detaches both
pointers. Hard stop kills it. Infinite-loop, finite-loop, pending-state,
already-decaying, invalid-tag/serial/entry, null and disabled-backend controls
use explicit input fields. No device channel or sample buffer is attached.

`queued_playout` extends the same fixture with the original event pool,
`SoundEvent` allocator `405190`, intrusive list bodies and start-pass suffix
`4045A6..4046C6`. Its input is a ready event with one already-loaded sample;
sample loading and decoding are outside the boundary. Original `4048B0` takes
its already-loaded path and original `404700` decides whether to prepare it.
Ordinary/released events reach the `4054A0` playback boundary, where success is
supplied without device work. Their retained state therefore remains the
supplied ready state; admission to playback is the measured result.

A detached ready SQD instead rejects playout, tries lower-priority entries,
retries, releases its sample and returns to the pool. The authored lower-entry
contrast reads `Priority=LOW` through original `750440` and supplies bucket-0
membership after the excluded ranking pass. Native retry stops and returns
every event of that entry, including an already-dead record, before retiring
the still-rejected request. The ordinary/released contrast leaves those
records alone. This proves the NO_REPLAY rejection path, not fixed sample-pool
capacity, channel arbitration or whole audio scheduling.

Raw-load controls reuse `naval_lifetime_controls.raw_load_sound_reset`:
original AbstractLoad `410380` reads supplied Unit-size bytes, FootLoad's
`4DB60D..4DB624` suffix resets audio state, and original no-init constructors
restore the vtables. The body counter survives. COM/vector/swizzle loading and
old-world Clear are excluded. Because NoInit clears the saved locomotor
pointer, a subsequent visit explicitly reattaches the existing constructor-built
idle Ship; the receipt records this boundary.

## Reproduction

Use the supported original executable from [`native_oracle.md`](../../native_oracle.md).
Set `VERA20K_SHRAPNEL_INPUTS` to the existing retail fixture root containing
RULESMD, MPBattleMD, XShrapnel, ARTMD and SOUNDMD; set `VERA20K_ANYTOWN_INPUTS`
for inherited imports and `VERA20K_PROJECTILE_RENDER_ASSETS` to the existing
retail ART/SHP extraction with `Hills.map`. `VERA20K_MOVE_SOUND_MAP` may identify
that same physical inner-map file explicitly. A Hills.mmx MIX container is not
an INI file. Input hashes in the corpus identify the expected bytes.

From the repository root:

```sh
python -m tools.spatial_oracle.fv_cell_attack.move_sound --foot-tail --check
```

`--write --output /scratch/foot_move_sound.json` stages a newly executed corpus
and sidecar for review. The source guard rejects changes to imported Python
owners during generation. No native outputs are calculated by host Python.
The legacy `move_sound` invocation, body-cadence producer, tagged sound owner
and naval audio/load producers retain their existing schemas and defaults.
