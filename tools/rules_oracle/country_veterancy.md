# Original country starting veterancy

`country_veterancy.py` reuses the shared `BulletReader` native runtime and
`native_oracle` checked runner. All fixture names and object priors are
synthetic. No game asset, MO rule or configuration is included.

```sh
python -m tools.rules_oracle.country_veterancy --check
```

`VERA20K_GAMEMD_EXE` or `RA2_DIR` selects the original executable. Evidence uses
original file-backed instructions through `tools.native_inspect` and execution
through Unicorn; no original instruction is patched.

## Native reader and storage

Country constructor `5113F0` initializes empty vectors at `+14C` Infantry,
`+168` Units and `+184` Aircraft. Each has items `+4`, count `+10`, initial
capacity 0 and capacity increment 10. Rules' Country loop calls actual Country
vtable `7EAB58` slot `+64` at `679A2D`, targeting reader `511850`.
`AbstractType410A60` first admits the exact stored Country ID at `+24` as the
section name; a missing or differently cased section leaves the lists unchanged.

| Exact key | Reader block | Native family binding |
| --- | --- | --- |
| `VeteranInfantry` | `511D16..511E20` | `524CB0`, InfantryType |
| `VeteranUnits` | `511E20..511F56` | `7480D0`, UnitType |
| `VeteranAircraft` | `511F56..51208C` | `41CEF0`, AircraftType |

Each uses original `ReadString528A10` with capacity 128 and the runtime empty
default at `889F64`. It reads at most 127 characters. A zero return retains
the current vector, including missing, stored-empty and whitespace-only values.
A nonzero result builds and replaces the whole list. Comma-only input therefore
clears it. Original `strtok7C9CC2` uses only comma (`817F70 = 2C00`), preserving
internal token whitespace, duplicates and order. It does not trim each token.

The original family factories reject case-insensitive `none` and `<none>`,
reuse case-insensitive existing IDs and otherwise construct that family's type.
Their stored IDs are limited to 24 characters. For example
`ONE,none,NONE,<none>, TWO,one` produces `ONE`, ` TWO`, `ONE`; the shared type
registry keeps those exact stored IDs.

The 12 `reader_controls` are sequential full `511850` reads on one retained
Country and registries. They cover replacement, missing/empty/whitespace
retention, clearing, sentinels, duplicate identity, delimiter whitespace,
128-byte read truncation, 24-byte stored IDs and key/section case. They do not
replay physical INI parsing or Rules::Process layer scheduling. Production's
existing layer owner supplies rules, optional language, mode and map reads.

## Original creation consumers

All three class paths read actor `+21C` House, then House `+34` Country directly.
They scan the appropriate Country vector for the actual class Type pointer.
There is no parent-Country indirection in these gates. A match calls
`SetVeteran750090(1)` on actor `+150`, writing raw binary32 bits `3F800000`.
It replaces the prior rank, including an existing elite `40000000`.

| Original path | Matching region | Reached caller |
| --- | --- | --- |
| Infantry InitFromType | `517CE7..517D20` | ctor `517A50` calls `517CC0` at `517B3F` |
| Unit constructor | `735604..73563D` | ctor `7353C0` contains the gate |
| Unit InitFromType | `746827..746860` | initializer `746810` after base `6F3F40` |
| Aircraft InitFromType | `413F98..413FD2` | ctor `413D20` calls `413F80` at `413E2E` |

Country membership is independent of `Trainable` and `Naval`. After that,
Infantry's Barracks infiltration (`House+2BF`) separately promotes a trainable
type. Unit's WarFactory infiltration (`House+2C0`) separately promotes a
trainable, non-Naval type. Matching country and spy gates execute two setter
calls, rather than an `else if`. Aircraft has the country gate and no spy gate.
Building `442CD6..442D1B` has only the existing WarFactory, Trainable,
non-Naval and nonnull `UndeploysInto+408` spy gate; this evidence establishes
no original country-building list.

The 28 `rank_controls` execute these selected original regions using native
reader/factory-produced Country and Type pointers. Prior actor/House fields
are supplied. The Aircraft interval includes its actual `PUSH EBX`; the matching
pop lies after unrelated type setup. Whole typed object construction, COM,
Unlimbo, House/spy lifecycle and full scenario chronology are excluded.

Map percent writes occur later in the map readers, before Unlimbo. Existing
`input_oracle/selection_navigation` owns their original `atoi`/`7500E0`
numeric goldens and caller intervals; this fixture does not duplicate that owner.

## Execution limits and validation

The generator asserts that every reader and rank control leaves all three RNG
streams byte-identical, writes no action-timer byte and keeps the sentinel
`[995,0,25]`. The selected rank bodies contain no RNG or detach call. This says
nothing about the whole object constructors' separate RNG/timer/detach
chronology. The corpus keeps only what the Rust comparisons read: each reader
control's sections and resulting lists, and each rank control's inputs and
raw rank before and after.

Native behavior is established within these reader and pointer-match
boundaries. Rust regression checks must exercise the production retained-list
and shared construction owners against these rows. Parity demonstrated by
those comparisons is bounded to their declared inputs, rather than all creation
paths or a complete native scenario.
