# Original country starting veterancy

`country_veterancy.py` reuses the shared `BulletReader` native runtime and
`native_oracle` checked runner. All fixture names, cameo pointers and object
priors are synthetic. No game asset, MO rule or configuration is included.

```powershell
$env:VERA20K_GAMEMD_EXE = 'C:/your-retail-install/gamemd.exe'
python -m tools.rules_oracle.country_veterancy --check
```

The recorded executable SHA-256 is
`7cd005d263fde203d9c84548200a057a8df61d724da3c6bd1e521eeb61cd0747`.
The payload records the actual selected executable hash, independently of the
shared runner's legacy fixed identity label. Ghidra was unavailable; evidence
uses original file-backed instructions through `tools.native_inspect` and
execution through Unicorn 2.1.4. No original instruction was patched.

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
`ONE,none,NONE,<none>, TWO,one` produces `ONE`, ` TWO`, `ONE`. A later original
base reader uses that exact stored ID for section admission: ` TWO` does not
admit `[TWO]`. This is a shared type-identity property, not a country-specific
normalization rule. The six `stored_id_admission_controls` execute original
`410A60` for both section names on each of the three native family types.

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

## Original cameo selector remains separate

Whole original `GetCameo712040..7120C6` reads the current House at `A83D4C`,
Barracks/WarFactory infiltration and actual type virtual `+2C` category leaves.
It selects nonnull `AltCameo+710` only for trainable Infantry after Barracks,
trainable Units after WarFactory, or a trainable Building with nonnull
`UndeploysInto` after WarFactory. Its Unit/Building cameo branch has no Naval
gate. Aircraft returns normal `Cameo+6F0`. Country lists do not affect this body.

The 20 whole-selector controls use original type vtables, rather than supplied
category returns. Ares' separate `CameoIsElite1003D800` country/extension checks
must use their own evidence; they do not change the original selector contract.

## Execution limits and validation

All three complete initial RNG objects are retained once in `rng_state_hex`.
Every row's measured before and after bytes must equal them; `rng_unchanged`
records that assertion. Per-row original Main next-four values, action timer
sentinel `[995,0,25]`, and observed timer writes are retained. All 66 rows keep
all three streams unchanged and record no timer writes. The selected rank and
cameo bodies contain no RNG or detach call. This says nothing about the whole
object constructors' separate RNG/timer/detach chronology.

Native behavior is established within these reader, pointer-match and selector
boundaries. Rust regression checks must exercise the production retained-list
and shared construction owners against these rows. Parity demonstrated by
those comparisons is bounded to their declared inputs, rather than all creation
paths or a complete native scenario.

An additional scratch-only stock check executes the selected list reads for
all 14 `[Countries]` entries in the machine's original `RULESMD.INI`
(`3d341ef8a13a4b5ab24af2eef48ac94931ac2bb87d950fe3330a07e2d25672ef`):
none sets any of the three keys, and all three native lists remain empty.
No `LANGRULE.INI` is present there. Mode/map physical layers were unavailable
to that check; earlier `selection_navigation` retail controls separately retain
empty Americans/Yuri lists across their declared Battle/map layers. These
retail observations do not make authored country lists unreachable.
