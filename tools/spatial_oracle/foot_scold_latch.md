# Foot ScoldSound latch: native boundaries

`foot_scold_latch.py` executes the original constructor, rules/sound readers,
raw-load and reconstruction boundaries, and selected Walk/Drive/Ship response
fragments. `foot_scold_latch.json` contains native outputs;
`foot_scold_latch.meta.json` records substitutions.
It does not establish an ordinary gameplay producer that arms Foot `+0x68A`.

From the repository, using the shared native environment:

```sh
export PYTHONPATH=.
export VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted-retail-inputs
python tools/spatial_oracle/foot_scold_latch.py --check
```

The retail directory must contain `RULESMD.INI`, `MPBattleMD.ini` and
`Hills.map`; `LANGRULE.INI` is read when present. SOUNDMD defaults to the
repository's `ini/soundmd.ini`; override with `VERA20K_SCOLD_SOUND_INI`.
Default invocation is read-only checking. Only explicit `--write` replaces
the corpus and metadata. No original executable instructions are patched.

## Established state and retail binding

- Ordinary Foot constructor `0x004D31E0` calls the Techno parent, zeroes EBX
  at `0x004D31EF`, and stores BL to `+0x68A` at `0x004D33B4`. The original
  post-parent prefix executes against storage initially filled with `0xA5`;
  the final latch is zero. Infantry's ordinary constructor calls this base
  constructor at `0x00517A5B`.
- The Rules constructor sets EBP to `-1` at `0x0066585F`, then stores it at
  Rules `+0x700` at `0x00666027`. The original audio-visual reader block
  `0x0066ABCD..0x0066AC18` uses `ReadString 0x00528A10` and
  `VocClass::FindByName 0x007514D0`; missing, empty or unknown names preserve
  the previous binding. Executed physical layers resolve stock
  `ScoldSound=MenuScold` and retain it through MPBattleMD and Hills.
- Original sound registry/read `0x007510D0` consumes the physical SOUNDMD
  `Defaults`, `MenuScold` and its actual `SoundList` key `308`. The fixture
  contains only this sound, so valid index **0 is fixture-relative**, not a
  claim about the full retail registry index. The native resolved identity
  is `MenuScold`; the original sample lookup requests `umenscol`.
- Infantry load `0x00521960` calls Foot load `0x004DB3C0`, Techno load
  `0x0070BF50`, Radio load `0x0065AB80`, Object load `0x005F5E80`, then
  Abstract load `0x00410380`. The latter reads a saved-this token and a raw
  body at `0x004103CD`, sized through Infantry `+0x30 -> 0x005232F0`
  (`0x6F0` bytes). Original execution preserves supplied saved latch bytes
  **0, 1 and 255**. Original no-init Foot constructor `0x004D3540`, called
  after loading at `0x00521A0C`, also preserves all three values. Intervening
  dynamic load suffixes were instruction-read, not emulated by this corpus.
- Foot checksum reads the byte at `0x004DBCFE` and calls the byte helper
  `0x004A1CA0` at `0x004DBD07`. Retained state should therefore preserve the
  raw byte; a boolean would lose imported `255` versus `1`. The sound guard
  itself treats both nonzero values identically.

## Response outputs and ordering

`scold_guard` contains three Walk exhausted-retry controls, entering at
`0x0075B085` after the existing retry count was zero. Nonzero bytes request
`0x00750920` with ECX = valid resolved MenuScold index 0, EDX = `0x2000`
(center), stack volume bits `0x3F800000` (1.0), then argument 0. All three
controls clear the byte at `0x0075B0A9`. Sound playback is an observed call
boundary: device output, sample playback and audio RNG are excluded.

`track_guards` contains 12 controls: Drive/Ship × exhausted/first-rejection
× bytes 0/1/255. Both families request the same sound arguments. Exhausted
branches clear after the call. First-rejection fragments **retain** the byte
at the retry continuation; that continuation may recurse before a later clear.
A shared sound helper must not automatically consume the latch.

| Consumer | Guard / clear identity |
| --- | --- |
| Walk exhausted retry | guard `75B085`, clear `75B0A9` |
| Walk class 4/5, retry disabled | clears `75BB84`, `75BB90` after override/Stop |
| Walk accepted / refused subcell | accepted alive owner clears `75BCB2`; dead owner skips it at `75BC36`; refusal clears `75BCD5` |
| Walk no head, positive applied speed | clear `75BD0F` after speed becomes zero |
| Walk arrival Mark(1) | clear `75BF77` after callback |
| Walk `+37C` true | motion false, then clear `75BF9B` |
| Walk common movement return | clear `75C1EA` |
| Walk same-cell SetCoords / SetHeight | clear `75C22F` after both callbacks |
| Drive exhausted / Ship exhausted | guards `4B2E47` / `6A2497`; clears `4B2E70` / `6A24C0` |
| Drive first rejection / Ship first rejection | guards `4B3AA1` / `6A30F0`; later clears `4B3C6E` / `6A32BD` |
| Drive fresh finalize / Ship fresh finalize | clears `4B4652` / `6A3C81` |

`paid_tails` contains 33 controls (11 named cases × bytes 0/1/255). The
zero/positive no-head speed paths, arrival, true predicate, same-cell commit
and common return clear. False predicate and post-PerCell dead, limbo and
falling exits retain the exact byte. Those lifecycle exits jump directly to
`75C1F1`, bypassing `75C1EA`. `75C22F` is a same-cell coordinate commit,
not a death or limbo exit. Mark/SetCoords/SetHeight callbacks observe the
original nonzero byte before its clear. Their effects and the `+37C` result
are supplied boundaries; the original Foot speed setter executes.

The accepted fresh-head dead-owner controls execute `75BC2A`'s motion store,
then `75BC36`'s direct exit. Motion changes from0 to1, while the exact scold
byte and prior speed0.75 remain. This fixes a Rust clear previously outside
the alive branch. These supplied lifecycle controls do not establish an ordinary
gameplay producer of a nonzero latch or a dead-owner fresh head.

## Writer audit and limits

`direct_scan` linearly decodes executable PE sections and records literal
memory displacement `0x68A`, wider nearby writes that overlap that byte,
and nearby LEA formations. There are 36 literal accesses: 35 Foot accesses
and one Session constructor access (`6975B0`, receiver global `A8B238`
from `4E8030`). The Foot set consists of eight reads/checksum accesses,
one zero constructor initialization and 26 immediate-zero writes. No direct
nonzero Foot writer was found. Wider `+688` writes belong to Building,
Rules or TechnoType objects. The nearby Foot `+686` address formation at
`4DC8E7` reaches `502460`, whose output is exactly one byte and cannot
overlap `+68A`; the other recorded LEAs address Building vectors or stack.

This scan and selected caller audit do not prove a constructor-to-consumer
invariant across all commands, aliases, generic copies or serialized input.
Raw load explicitly permits nonzero state. **Ordinary retail arming remains
unresolved; the branch is not certified globally unreachable or dormant.**
Do not invent a move-command arming writer to make the branch active.

The existing Rust `NavigationState::path_runtime` / `FootPathRuntime` is the
shared Foot state owner, independent of replaceable locomotors. Rules already
owns `scold_sound`; simulation sound events and centered audio dispatch exist.
Preserving the byte, native clears, snapshot/hash behavior and the centered
request is a small state/event dependency. It does not require a separate
audio subsystem. Existing 124-row `walk_prehead_response` supplies this byte
and can compare the newly represented state without changing its goldens.
