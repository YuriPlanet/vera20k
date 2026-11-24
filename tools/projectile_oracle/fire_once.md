# FireAt FireOnce continuation

`fire_once.py` executes the original active-retail FireAt common tail and the
concrete target and navigation setters it calls. Its corpus is a bounded native
comparison for stock IVAN/IvanBomber and ENGINEER/DefuseKit continuations. It does
not execute command admission, the preceding shot, bomb attach/defuse or Team AI.

The [sidecar](fire_once.meta.json) records entry points, reader layers, physical
file identities, source seals, field layouts, substitutions and exclusions.
The [shared runner](../native_oracle.md) verifies the executable and bounds each
native invocation. Original text and actual class vtables must remain unchanged.

## Native call chain

FireAt `0x006FF8F1` reads Weapon `FireOnce` at `+0x135`. A false value skips the
cleanup. A true value tests the source's Abstract Foot flag at `+0x14`. When its
Team `+0x5D4` is nonnull, it calls `0x006E9050` with NULL, rereads that membership
pointer, and stores one to Team `+0x80`. It then calls the source's concrete
target setter at virtual slot `+0x3C8` with NULL, including for a non-Foot source.

`0x006E9050` is **Assign_Mission_Target**, not an immediate script step. When the
Team's old mission target is nonnull and differs from the new target, it visits
the linked members in order. A matching combat target or navigation target
queues Guard (5). A matching navigation target is cleared through the concrete
Unit/Infantry destination setter, with force argument one. A matching combat
target is cleared through the concrete target setter. The current mission is
unchanged. Focus is replaced only if it equals the old target or is NULL; an
unrelated focus survives. The NULL assignment preserves the LeavingMap flag.
The later FireAt `+0x80` write requests script advancement by its existing owner.

The ordinary Infantry target setter `0x0051B1F0` invokes unforced Ready when a
changed living target is released, clears the firing latch, resets path state
and delegates to shared Techno setter `0x006FCDB0`. The corpus retains the final
repeated firer setter after team cleanup. Archive target is observed separately.

Both refused BulletFire (`0x006FF01A`, supplied AL=0) and failed ballistic cleanup
(`0x006FF93C`) destroy the supplied Bullet, set the result to NULL and jump to the
same common tail at `0x006FF749`. Each failed row must visit original destructor
`0x00466560`. The Bullet comes from original factory `0x006C5090`, including the
original COM AddRef; the original `0x0046AFF0` Release decrements `+0x1C`.
These supplied failure results do not assert that stock Inviso Ivan shots fail
ballistic launch.

## Corpus and consumer

The [13 cases](fire_once.json) cover FireUp and Ready source action boundaries,
FireOnce false, no team, both stock weapons, old/NULL/unrelated team focus, no old
mission target, an already pending team, both failed continuations, and a supplied
non-Foot flag. The stock source and HTNK target types, weapon/projectile/warhead
readers, physical Infantry sequences, constructors, locomotor factories and
Unlimbo execute through the unchanged [FootMissions owner](../spatial_oracle/anytown_damage/foot_missions.md).
Team constructor/Add and earlier launch are explicit supplied boundaries.
The inherited House's live `IsHuman+0x1EC` and `PlayerControl+0x1ED` bytes are
recorded in `inputs.house`; this supplied House is human and player-controlled.

`before` and `after` contain the global frame, team state and each member's literal
position, mission/queued mission, target/destination/archive, action, path and
timer words. The sidecar's `field_mapping` gives offsets, integer widths and
coordinate units. Timer word one is a raw unused auxiliary observation: native
DoAction `0x0051DA3A..0x0051DA3E` copies caller scratch into Sequence `+0x104`.
Rust compares timer start/duration (words zero/two), plus Sequence repeat rate
(word three). `rng_before` and
`rng_after` contain the complete main, Scenario and mapgen streams; `rng_calls`
records draw entries. All covered continuations leave all three streams unchanged.
The `calls` and `writes` arrays preserve instruction order, including the queued
Guard and pending flag. Pointer labels associate NULL, old HTNK, other MTNK and
member roles without requiring native addresses in Rust. The stable role
`unit_unrelated` names an E1 Infantry; consumers use `type_id` for its class.

A Rust consumer restores the selected row's `before` through existing owners,
including its authoritative timer/action/path and RNG state, invokes the shared FireOnce
completion, and compares its affected `after` fields and complete RNG states.
Launch result is a supplied input to the continuation, not an inferred golden.
The native ordered calls remain evidence for source/team mutation order;
corpus equality does not establish unexecuted production or whole-engine behavior.

## Reproduction

Use the original retail executable and the physical inputs already required by
the existing Anytown/selected-reader fixtures. `VERA20K_SHRAPNEL_INPUTS` provides
the original RULESMD, selected mode, ARTMD, SOUNDMD and TEMPERATMD inputs;
`VERA20K_ANYTOWN_INPUTS` supplies the XMP03T4 map and map/theater crop inputs.
`VERA20K_FIRE_ONCE_INPUTS` is a directory containing original `GI.SHP`, `IVAN.SHP`
and `ENGINEER.SHP`, extracted from `ra2.mix/conquer.mix` with the existing asset
CLI. The sidecar pins every listed physical file; no retail bytes are committed.

```sh
export VERA20K_GAMEMD_EXE='/path/to/retail/gamemd.exe'
export VERA20K_SHRAPNEL_INPUTS='/path/to/vera20k_shrapnel_inputs/extract'
export VERA20K_ANYTOWN_INPUTS='/path/to/vera20k_anytown_inputs/extract'
export VERA20K_FIRE_ONCE_INPUTS='/path/to/fire_once_shapes/extract'
python -m tools.projectile_oracle.fire_once --check
```

`--check` compares without writing. `--write` explicitly replaces the corpus and
sidecar; review every change before accepting it. `--help` does not load the
binary or execute the fixture. The corpus records the reader's observed native
FireOnce default false and stock layered true values. LANGRULE is absent in the
selected install. The false control changes only the INI key `FireOnce=no` and
passes it through original Weapon reader `0x00772080`.

Aircraft, buildings, nonordinary Infantry deployment and subordinate/spawned/
temporal target lifecycles, other FireOnce weapons, off-map assignments, script
scheduling, persistence, audio-device output and rendering remain outside this
fixture. Its substituted heap/CRT/OS/COM transports, file buffers, visual/audio
sinks and supplied Team/FireAt continuation are retained explicitly in metadata.
