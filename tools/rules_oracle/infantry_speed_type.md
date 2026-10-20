# Infantry SpeedType constructor and rules dependency

The original Engineer type starts with `SpeedType=Foot` (native0). On the
physical Hills layers used here, RULESMD's `[ENGINEER]` has no `SpeedType`,
LANGRULE is absent, and MPBattleMD/Hills have no Engineer section. Native0
therefore remains the final value. The captured production Engineer had Track1;
that default was a required input correction for ordinary Engineer movement.

## Executed boundary

`infantry_speed_type.py` executes the complete original InfantryType constructor
at5236A0. It passes0 to TechnoType710AF0; the recorded write at7110E0 stores0
at type+67C. No later Infantry constructor write changes that field.

For each supplied lexical INI cache, original AbstractType410A60..410A8C/410B7D
decides whether the exact type section exists. Admitted sections execute the
original TechnoType field block7121D1..7121EB with a supplied caller frame.
Infantry's reader5240A0 calls TechnoType712170, which calls ObjectType5F92D0
and AbstractType410A60 before this field block. The block uses the caller's
rules INI and the type ID at+24; it does not use ARTMD. Unrelated intervening
type reads, reader suffixes, and the physical file/archive loader are excluded.

The exact key is `SpeedType`, addressed at844504. Current type+67C is the
default passed to ReadSpeedType476FC0. That reader converts the default back
to its name, calls original ReadString528A10 with a128-byte destination, then
uses SpeedTypeFromName48DFF0. Name matching is whole-string and case-insensitive;
INI section/key lookup is exact-case. Native trimming and the127-byte content
cap run before name matching. Empty values retain the current field; malformed,
numeric, `None`, and `<none>` values produce-1, which is stored without a clamp.

The retained table81DA58 is Foot0, Track1, Wheel2, Hover3, Winged4, Float5,
Amphibious6, FloatBeach7. Missing later keys retain explicit prior values.
The six-layer control Track→missing→empty→invalid→missing→Foot produces
1,1,1,-1,-1,0. Two raw-cache cap controls distinguish123 spaces+Foot+Track
(cut to Foot, result0) from124 spaces+Foot (cut to Foo, result-1).

## Coverage and reproduction

The corpus retains one physical constructor/layer history,26 separate parser
controls, and a six-step retained-default history. Constructor/setup uses the
existing bounded allocator and CRT seams. The measured section/field reader
asserts no substituted callable is reached and original code bytes remain
unchanged. This establishes this field's native input boundary; it does not
certify full Infantry loading, AStar routes, locomotor movement, or bridge repair.

Extract physical RULESMD.INI, MPBattleMD.ini and Hills.map into one directory;
include LANGRULE.INI only when that installation actually supplies it. Use the
normal native-oracle executable environment and select that directory through
`VERA20K_PROJECTILE_RENDER_ASSETS` (`bridge_render_inputs.assets_root()`).

```sh
VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted/retail \
  PYTHONPATH=. python -m tools.rules_oracle.infantry_speed_type --check
```

No argument also checks without writing. `--write` deliberately regenerates the
JSON and provenance sidecar and requires review of the changed native reference.
The physical file hashes and source strings are retained in the payload.
