# Ares 3.0 custom armor component controls

The adjacent JSON holds 57 original-instruction controls for generic custom
armor and the ordinary weapon path. Inputs use synthetic `paper`, `later`,
`magic` and `TestWH` definitions. No mod rules, assets or executable is included.
The metadata pins the exact Ares and supporting game binary identities, entry
points, floating-point control word and substitutions.

Run from the checkout with the dependencies in `tools/requirements-test.txt`:

```text
python -m tools.rules_oracle.ares_armor --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --check
python -m tools.rules_oracle.ares_armor --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --output <new-corpus.json>
```

Both input files are explicit and their identities enter the corpus. The Ares
DLL is hash checked; a supporting executable with a different hash produces a
different identity and cannot silently satisfy `--check`. The harness inherits the
existing Ares prerequisite guest fixture and reuses `tools.native_oracle` for
checked PE mapping and bounded execution. It adds no loader or executable
allowlist. Instructions execute at preferred VAs without Windows startup,
relocation, import binding or host DLL execution. `--check` verifies the corpus
hash against its metadata and compares newly executed results with the corpus.

## JSON consumers

`schema_version` is 1. `identity` records both file roles and SHA256;
`fpcw` is `0x0e7f`; `cases` is an array of named rows:

- `parse_<token>`: `fixture.token` and `result` with `multiplier`,
  `multiplier_bits` (16 hexadecimal digits), `force_fire`, `retaliate` and
  `passive_acquire`. The bits are read directly from guest memory.
- `registry_<name>`: `default_index` and `default_verses` with the same fields.
- `warhead_inline_inheritance` and `warhead_reread_retains_custom`:
  `result` maps armor names to the complete verses records.
- `can_fire_*` and `admit_*`: `return_address` and `accepted` establish branch
  destinations. `can_fire_nan_false` supplies raw multiplier bits without a
  nonstandard JSON NaN value.
- `weapon_select_secondary*_primary*`: fixture multipliers and native branch
  destination. Secondary is checked before primary; flags do not replace the
  zero-multiplier checks.
- `total_damage_*`: declared damage, multiplier, armor, cap and distance;
  `result` is the signed integer from original `GetTotalDamage` with the actual
  installed Ares hook. The six shorter `damage_*` rows characterize only the
  multiplier hook, including inputs unreachable through the negative-damage
  branch of that complete receiver.
- `read_armor_*`: the installed Ares armor reader hook's returned index.

Each row also records supplied INI/CRT calls where applicable. Tests should
use only the rows relevant to their mechanism, preserving these boundaries.

## Behavior bounded by these controls

The token parser at `0x100523E0` distinguishes literal `0%`, `1%`, `2%`
flag behavior from other spellings. `00%` has zero multiplier with all flags
true; `1.5%` uses the supplied CRT integer value 1 before the original percent
multiplication. Nonpercent `0.021`, supplied as binary64
`3f95810624dd2f1b`, enables all three flags after original x87 comparisons.
Decimal-to-binary conversion itself is a supplied CRT leaf, not certified by
these parser controls.

The installed warhead entry reader `0x10006460` initializes and overrides each
armor entry before moving to the next. A newly appended alias inherits an
earlier entry's already overridden multiplier and flags. A forward reference
uses its own fallback when that referenced entry does not yet exist. Existing
entries remain retained on later passes rather than being re-inherited.

`CanFire` at `0x100542A0` rejects only ordered equality to zero with
`ForceFire=false`. A quiet NaN with ForceFire disabled is admitted. Passive
acquisition (`0x10054270`) and retaliation (`0x100543E0`) read their independent
flags; selection (`0x10054360`) reads multiplier zero separately.

Eight complete-receiver cases run original `0x00489180` with a guest-only
REGISTERS adapter at the installed Ares hook site **`0x00489235`** and execute
`0x10054230`, returning to `0x00489249`. Spread, percent-at-max, rules cap,
scenario flag and distance are supplied inputs. Positive 101 at 25% produces25;
3 at 50% produces1; a cap of300 limits 123 at1000% to300. Incoming negative
damage bypasses verses at signed distance below8, so the complete receiver
returns -101 rather than the hook-only result -25. These are not whole healing
or damage-lifecycle comparisons.

## Caller integration established by instruction reading

These caller facts bound the intended consumers; they are not extra claims
that the 57-row fixture runs whole startup or the full extension reader:

- Ares skips native `Verses` parsing via `75DDCC -> 10054410 -> 75DE98`.
  The successful original tail clears warhead `+149`, disabling the old
  medium/wood target-class shortcut. The extension suffix at75DEA0/75DEAF
  calls10052EC0 ->10051D10 ->10051D80 and writes the extension vector;
  effective verses are not mirrored into original warhead `+A0`.
- Custom armor registration `679A15 ->1002C9C0 ->1002AEF0` precedes that
  pass's type bodies. All names are allocated before defaults are read.
- The armor hook replaces4753F0, called at5F94C8 by common ObjectType
  ReadINI5F92D0 and stored at ObjectType+9C. Techno and Terrain share this
  parser; it is not a Techno-only hook.

## Limits and references

INI payloads, valid registry storage, hook register states and selected type
queries are supplied. CRT numeric conversion, ASCII string/memory operations,
allocation and logging are fixture services. Original inheritance, flag
derivation, branch decisions and damage arithmetic execute; physical files,
whole Rules passes, full extension reading, gameplay scheduling, special
effects and persistence are excluded. No whole-mod or rendered parity is
claimed. The records do not contain game or mod data.

The [Ares documentation](https://ares-developers.github.io/Ares-docs/new/additionalarmortypesandverses.html),
[ArmorTypes source](https://github.com/Ares-Developers/Ares/blob/master/src/Enum/ArmorTypes.cpp)
and [Verses hooks](https://github.com/Ares-Developers/Ares/blob/master/src/Ext/WarheadType/Hooks.Verses.cpp)
are interpretation leads. Public master differs from this installed3.0 binary
in initialization/override order, fire admission and hook addresses; the
recorded binary identities and executed instructions take precedence.
